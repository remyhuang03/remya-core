mod ai;
mod events;
mod paths;
mod privileged;
mod requests;
mod shared_theme;
mod state;
use gio::glib::{self, variant::ToVariant};
use std::sync::{Arc, Mutex};
const NAME: &str = "top.remya.Core1";
const PATH: &str = "/top/remya/Core1";
const INTERFACE: &str = "top.remya.Core1";
fn signal(bus: &gio::DBusConnection, topic: &str, value: &serde_json::Value) {
    let _ = bus.emit_signal(
        None,
        PATH,
        INTERFACE,
        "Changed",
        Some(&(topic, value.to_string()).to_variant()),
    );
}
fn return_error(invocation: gio::DBusMethodInvocation, error: &str) {
    let (kind, message) = error
        .split_once(':')
        .filter(|(k, _)| ["InvalidArgument", "Unavailable"].contains(k))
        .unwrap_or(("OperationFailed", error));
    invocation.return_dbus_error(&format!("{INTERFACE}.{kind}"), message);
}
/// Capture change payloads with the operation's state on the worker, so
/// broadcasting them on the main loop never needs the system-feature lock.
fn changed_payloads(
    state: &state::State,
    method: &str,
    args: &[String],
    before: (serde_json::Value, serde_json::Value),
) -> Vec<(&'static str, serde_json::Value)> {
    let mut signals = Vec::new();
    let theme = state.theme_state();
    if theme != before.0 {
        signals.push(("theme", theme));
    }
    if method == "Reload"
        || args
            .first()
            .is_some_and(|id| id == "appearance" || id == remya_appearance::APPLY_TO_SYSTEM)
    {
        signals.push(("appearance", state.appearance.state()));
    }
    signals.push(("date-time", state.date_state()));
    signals.push(("display", state.display.state()));
    signals.push(("hostname", state.hostname_state()));
    if method == "Reload"
        || args
            .first()
            .is_some_and(|id| id == "night-light" || remya_display::is_night_light_setting(id))
    {
        signals.push(("night-light", state.night_light.state()));
    }
    if method == "Reload"
        || args
            .first()
            .is_some_and(|id| id == "network" || remya_network::is_network_setting(id))
    {
        signals.push(("network", state.network.state()));
    }
    if method == "Reload"
        || args
            .first()
            .is_some_and(|id| id.starts_with("defaults.") || id == "default-apps")
    {
        signals.push(("default-apps", state.default_apps.state()));
    }
    if method == "Reload"
        || args
            .first()
            .is_some_and(|id| id == "startup" || remya_startup::IDS.contains(&id.as_str()))
    {
        signals.push(("startup", state.startup.state()));
    }
    signals.push(("background", state.background_state()));
    let accent = state.accent_state();
    if accent != before.1 {
        signals.push(("accent", accent));
    }
    if args.first().is_some_and(|id| id.starts_with("installed.")) {
        signals.push(("installed-apps", serde_json::json!({})));
    }
    if args
        .first()
        .is_some_and(|id| remya_system_information::account::IDS.contains(&id.as_str()))
        && let Ok(account) = state.account.read()
    {
        signals.push(("account-information", account));
    }
    if method == "Reload"
        || args
            .first()
            .is_some_and(|id| id == "input" || remya_input::is_setting(id))
    {
        signals.push(("input", state.input.state()));
    }
    if method == "Reload"
        || args.first().is_some_and(|id| {
            id == remya_date_time::language::FEATURE || remya_date_time::language::is_setting(id)
        })
    {
        signals.push(("language", state.language.state()));
    }
    if method == "Reload"
        || args
            .first()
            .is_some_and(|id| id == "desktop" || remya_input::is_desktop_setting(id))
    {
        signals.push(("desktop", state.desktop.state()));
    }
    if method == "Reload"
        || args
            .first()
            .is_some_and(|id| id == "themes" || remya_appearance::packs::is_setting(id))
    {
        signals.push(("themes", state.themes.state()));
    }
    if method == "Reload"
        || args
            .first()
            .is_some_and(|id| id == "text-size" || remya_appearance::text_size::is_setting(id))
    {
        signals.push(("text-size", state.text_size.state()));
    }
    if method == "Reload"
        || args.first().is_some_and(|id| {
            id == "power"
                || id == "battery-saver"
                || (id != remya_power::KEEP_AWAKE && remya_power::is_setting(id))
        })
    {
        signals.push(("power", state.power.state()));
    }
    if method == "Reload"
        || args
            .first()
            .is_some_and(|id| id == "sound" || remya_sound::is_setting(id))
    {
        signals.push(("sound", state.sound.state()));
    }
    if method == "Reload"
        || args.first().is_some_and(|id| {
            id == "cameras" || id.starts_with("cameras:") || remya_camera::is_setting(id)
        })
    {
        signals.push(("cameras", state.cameras.state()));
    }
    if args.first().is_some_and(|id| remya_updates::is_setting(id)) {
        signals.push(("updates", state.updates.state()));
    }
    if args
        .first()
        .is_some_and(|id| remya_fonts::IDS.contains(&id.as_str()))
    {
        signals.push(("fonts", serde_json::json!({})));
    }
    signals
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
fn run() -> Result<(), String> {
    if std::env::args().nth(1).as_deref() == Some("--privileged") {
        return privileged::serve();
    }
    let bus = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE)
        .map_err(|e| e.to_string())?;
    let debug = std::env::args()
        .skip_while(|a| a != "--debug-root")
        .nth(1)
        .map(Into::into);
    let initial = state::State::new(paths::Roots::detect(debug)?);
    let (ai_changed, ai_changes) = std::sync::mpsc::channel();
    initial.requests.ai.set_notifier(ai_changed);
    initial.requests.ai.set_secret_bus(bus.clone());
    let ai = initial.requests.ai.clone();
    let ai_bus = bus.clone();
    std::thread::spawn(move || {
        for () in ai_changes {
            if let Ok(value) = ai.get(ai::PROVIDERS) {
                signal(&ai_bus, ai::PROVIDERS, &value);
            }
        }
    });
    let requests = initial.requests.clone();
    let apps = requests.apps.clone();
    let appimages = initial.appimages.clone();
    let storage = initial.storage.clone();
    let bluetooth = initial.bluetooth.clone();
    let state = Arc::new(Mutex::new(initial));
    let xml = include_str!("../../../packages/remya_client/api/core1.xml");
    let info = gio::DBusNodeInfo::for_xml(xml)
        .map_err(|e| e.to_string())?
        .lookup_interface(INTERFACE)
        .unwrap();
    let handler = state.clone();
    let main_loop = glib::MainLoop::new(None, false);
    let quit = main_loop.clone();
    let _registration = bus
        .register_object(PATH, &info)
        .method_call(move |bus, _, _, _, method, args, invocation| {
            let args = (0..args.n_children())
                .map(|i| args.child_value(i).get::<String>().unwrap())
                .collect::<Vec<_>>();
            let state = handler.clone();
            let requests = requests.clone();
            let quit = quit.clone();
            let method = method.to_owned();
            glib::MainContext::default().spawn_local(async move {
                let worker = state.clone();
                let work_method = method.clone();
                let work_args = args.clone();
                if let Some(result) = gio::spawn_blocking(move || {
                    let result = requests.execute(&worker, &work_method, &work_args);
                    if work_method == "Reload"
                        || (work_method == "Get" && work_args[0] == "configuration")
                    {
                        requests.apps.lock().unwrap().rescan();
                    }
                    result
                })
                .await
                .expect("independent request worker panicked")
                {
                    match result {
                        Ok(value) => {
                            invocation.return_value(Some(&(value.to_string(),).to_variant()));
                            if ["Set", "Replace"].contains(&method.as_str()) {
                                if args[0] != remya_applications::UNINSTALL {
                                    signal(&bus, &args[0], &value);
                                }
                                if ai::SET_IDS.contains(&args[0].as_str())
                                    && args[0] != ai::PROVIDERS
                                {
                                    signal(&bus, ai::PROVIDERS, &value);
                                }
                                if args[0] == remya_applications::UNINSTALL {
                                    signal(&bus, "installed-apps", &serde_json::json!({}));
                                }
                            }
                        }
                        Err(error) => return_error(invocation, &error),
                    }
                    return;
                }
                let worker = state.clone();
                let work_method = method.clone();
                let work_args = args.clone();
                let (result, signals) = gio::spawn_blocking(move || {
                    let mut state = worker.lock().unwrap();
                    let notify =
                        ["Set", "Resolve", "Manage", "Reload"].contains(&work_method.as_str());
                    let before = notify.then(|| (state.theme_state(), state.accent_state()));
                    let result = state.execute(&work_method, &work_args);
                    if let Err(error) = &result
                        && work_method == "Set"
                        && work_args[0].starts_with("date-time.")
                    {
                        state.date_error = Some(error.clone());
                    }
                    let mut signals = before
                        .map(|before| changed_payloads(&state, &work_method, &work_args, before))
                        .unwrap_or_default();
                    if let Err(error) = &result
                        && work_method == "Set"
                        && shared_theme::field(&work_args[0]).is_some()
                    {
                        signals.push(("error", serde_json::json!({"message":error})));
                    }
                    (result, signals)
                })
                .await
                .expect("service worker panicked");
                // A rejected page must not open anything or request authorization.
                let opened_page = match (result.is_ok(), method.as_str()) {
                    (true, "Open") if args[0] != "repository" => Some(args[0].as_str()),
                    (true, "Set") => state::linked_page(&args[0]),
                    _ => None,
                };
                let succeeded = result.is_ok();
                match result {
                    Ok(value) => {
                        invocation.return_value(Some(&(value.to_string(),).to_variant()));
                        if method == "Set"
                            && ([remya_power::KEEP_AWAKE, remya_display::night_light::GAMMA]
                                .contains(&args[0].as_str())
                                || remya_network::proxy::IDS.contains(&args[0].as_str())
                                || [
                                    remya_network::airplane::ENABLED,
                                    remya_network::airplane::WIFI,
                                ]
                                .contains(&args[0].as_str()))
                        {
                            signal(&bus, &args[0], &value);
                        }
                        if method == "Set"
                            && (args[0] == remya_display::manager::BRIGHTNESS
                                || args[0].starts_with("display-brightness:"))
                        {
                            signal(&bus, "display-brightness", &value);
                        }
                        if method == "Set" && remya_printers::IDS.contains(&args[0].as_str()) {
                            signal(&bus, "printers", &value);
                        }
                    }
                    Err(error) => return_error(invocation, &error),
                }
                for (topic, value) in signals {
                    signal(&bus, topic, &value);
                }
                if let Some(page) = opened_page {
                    signal(&bus, "navigation", &serde_json::json!({"page":page}));
                    let needed = state.lock().unwrap().begin_authorization();
                    if needed {
                        signal(&bus, "date-time", &serde_json::json!({"authorizing":true}));
                        let result = gio::spawn_blocking(privileged::Session::start)
                            .await
                            .expect("authorization worker");
                        state.lock().unwrap().finish_authorization(result);
                    }
                    signal(&bus, "date-time", &state.lock().unwrap().date_state());
                }
                if opened_page.is_some() {
                    signal(&bus, "hostname", &state.lock().unwrap().hostname_state());
                }
                if method == "Quit" && succeeded {
                    let _ = bus.flush_future().await;
                    state.lock().unwrap().shutdown();
                    quit.quit();
                }
            });
        })
        .build()
        .map_err(|e| e.to_string())?;
    let result = bus
        .call_sync(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            "RequestName",
            Some(&(NAME, 4u32).to_variant()),
            None,
            gio::DBusCallFlags::NONE,
            5000,
            gio::Cancellable::NONE,
        )
        .map_err(|e| e.to_string())?;
    if result.get::<(u32,)>() != Some((1,)) {
        return Ok(());
    }
    {
        apps.lock().unwrap().rescan();
        let mut state = state.lock().unwrap();
        state.ensure_idle();
        state.refresh();
    }
    let polling = state.clone();
    let poll_bus = bus.clone();
    let mut ticks = 0u32;
    events::watch(state.clone(), bus.clone());
    events::watch_sound(state.clone(), bus.clone());
    if let Err(error) = events::watch_backlight(state.clone(), bus.clone()) {
        eprintln!("Backlight events unavailable: {error}");
    }
    let _network_subscriptions = events::watch_network(state.clone(), bus.clone());
    let _power_sleep = events::watch_power_sleep(state.clone());
    let power_state = state.clone();
    let power_bus = bus.clone();
    std::thread::spawn(move || {
        loop {
            {
                let mut s = power_state.lock().unwrap();
                let before = s.power.state();
                s.power.tick();
                if s.power.state() != before {
                    signal(&power_bus, "power", &s.power.state());
                }
            }
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    });
    glib::timeout_add_seconds_local(1, move || {
        ticks += 1;
        if bluetooth.take_dirty() {
            signal(&poll_bus, "bluetooth", &bluetooth.state());
        }
        if let Ok(state) = polling.try_lock() {
            if !state.uninstalling {
                state.updates.tick();
            }
            if state.updates.take_dirty() {
                signal(&poll_bus, "updates", &state.updates.state());
            }
        }
        if storage.take_dirty() {
            signal(&poll_bus, "storage", &storage.state("storage"));
            signal(&poll_bus, "cleanup", &storage.state("cleanup"));
        }
        if appimages.take_dirty() {
            signal(&poll_bus, "appimages", &appimages.state());
        }
        if ticks.is_multiple_of(60) {
            appimages.auto_tick();
            storage.auto_tick();
        }
        let mut expired = false;
        if let Ok(mut state) = polling.try_lock() {
            state.reap_gui();
            if state.reap_authorization() {
                signal(&poll_bus, "hostname", &state.hostname_state());
            }
            expired = state.display.pending_expired();
        }
        if expired {
            let state = polling.clone();
            let bus = poll_bus.clone();
            std::thread::spawn(move || {
                let mut s = state.lock().unwrap();
                // Another caller may have kept or reverted it first; this is then a no-op.
                if s.display.pending_expired() {
                    let result = s.display.revert_pending();
                    if let Err(message) = result {
                        signal(&bus, "error", &serde_json::json!({"message": message}));
                    }
                    signal(&bus, "display", &s.display.state());
                }
            });
        }
        let state = polling.clone();
        let bus = poll_bus.clone();
        if ticks.is_multiple_of(30) {
            let apps = apps.clone();
            std::thread::spawn(move || {
                apps.lock().unwrap().rescan();
                let mut s = state.lock().unwrap();
                let before = s.theme_state();
                let appearance = s.appearance.state();
                let errors = s.theme_error.clone();
                let accent = s.accent_state();
                let network = s.network.state();
                let night_light = s.night_light.state();
                let sound = s.sound.state();
                let cameras = s.cameras.state();
                let startup = s.startup.state();
                s.refresh();
                if s.startup.state() != startup {
                    signal(&bus, "startup", &s.startup.state());
                }
                if s.cameras.state() != cameras {
                    signal(&bus, "cameras", &s.cameras.state());
                }
                if s.sound.state() != sound {
                    signal(&bus, "sound", &s.sound.state());
                }
                if s.night_light.state() != night_light {
                    signal(&bus, "night-light", &s.night_light.state());
                }
                if s.network.state() != network {
                    signal(&bus, "network", &s.network.state());
                }
                if s.accent_state() != accent {
                    signal(&bus, "accent", &s.accent_state());
                }
                if s.appearance.state() != appearance {
                    signal(&bus, "appearance", &s.appearance.state());
                }
                if s.theme_state() != before {
                    signal(&bus, "theme", &s.theme_state());
                }
                if s.theme_error != errors {
                    signal(&bus, "error", &serde_json::json!({"message":s.theme_error}));
                }
                signal(&bus, "date-time", &s.date_state());
                signal(&bus, "hostname", &s.hostname_state());
                signal(&bus, "display", &s.display.state());
                signal(&bus, "background", &s.background_state());
            });
        }
        glib::ControlFlow::Continue
    });
    main_loop.run();
    bus.unregister_object(_registration)
        .map_err(|e| e.to_string())?;
    drop(state);
    Ok(())
}
