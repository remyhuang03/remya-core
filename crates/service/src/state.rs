use crate::paths::Roots;
use crate::privileged::{Request, Session};
use crate::shared_theme::{self, CONTRAST, MODE, STYLE};
use gio::glib::{Variant, variant::ToVariant};
use gio::prelude::AppInfoExt;
use remya_appearance::packs::{self, ThemePacks};
use remya_appearance::text_size::{self, TextSize};
use remya_appearance::{APPLY_TO_SYSTEM, DesktopTheme, SystemThemeMode};
use remya_applications as applications;
use remya_background::{
    BackgroundManager, RealDesktop, is_accent_setting, is_background_setting, manager::Paths,
};
use remya_date_time as clock;
use remya_date_time::language::{self, Language};
use remya_default_apps::{self as default_apps, DefaultApps};
use remya_display::{
    DisplayManager, Hyprctl, NightLight, RealSunset, atomic_write, is_display_setting,
    is_night_light_setting,
};
use remya_fonts::{self as fonts, FontManager, RealFontconfig};
use remya_input::{self as input, Input, RealHypr};
use remya_network::{NetworkManager, is_network_setting};
use remya_printers::{self as printers, Printers, RealCups};
use remya_sound::{self as sound, Sound, pulse::RealPulse};
use remya_startup::{self as startup, Startup};
use remya_system_information::account::{self, AccountInformation};
use remya_system_information::hostname::{self, HostnameManager};
use remya_updates::{self as updates, Updates};
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    process::{Child, Command},
    sync::{Arc, Mutex},
};

pub fn linked_page(id: &str) -> Option<&'static str> {
    match id {
        "power.743773bd" => Some("battery-saver"),
        "power.03bd05ee" => Some("battery-usage"),
        _ => None,
    }
}

pub const ZONE: &str = "date-time.fb2a23dc";
pub const MANUAL: &str = "date-time.6422d209";
pub const AUTO: &str = "date-time.db451de3";
pub const SYNC: &str = "date-time.098d06ec";
pub const SERVERS: &str = "date-time.7e33a213";
fn invalid(message: &str) -> String {
    format!("InvalidArgument:{message}")
}
fn prefs(state: &clock::DateTimeState) -> Value {
    json!({"timezone":state.timezone,"ntp":state.ntp,"time_servers":state.time_servers})
}
fn write(path: &std::path::Path, value: &Value) -> Result<(), String> {
    atomic_write(
        path,
        toml::to_string(value)
            .map_err(|e| e.to_string())?
            .as_bytes(),
    )
}
/// `system-information` is the public name of the About page.
pub fn page_alias(page: &str) -> &str {
    if page == "system-information" {
        "about"
    } else {
        page
    }
}
fn load(path: &std::path::Path) -> Result<Option<Value>, String> {
    match fs::read_to_string(path) {
        Ok(s) => toml::from_str(&s)
            .map(Some)
            .map_err(|e| format!("{}: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

/// Hyprland reads `$XDG_CONFIG_HOME/hypr`; relative XDG values are ignored.
fn hypr_config_dir() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .map(|p| p.join(".config"))
        })
        .unwrap_or_default()
        .join("hypr")
}

/// Desktop Exec may wrap the GUI with env and escape literal percent signs.
fn settings_executable(commandline: &str) -> Option<PathBuf> {
    gio::glib::shell_parse_argv(commandline)
        .ok()?
        .into_iter()
        .map(|arg| PathBuf::from(arg.to_string_lossy().replace("%%", "%")))
        .find(|arg| arg.file_name().is_some_and(|name| name == "remya_settings"))
        .and_then(gio::glib::find_program_in_path)
}

fn background_paths(roots: &Roots) -> Paths {
    let dirs = xdg::BaseDirectories::new();
    Paths {
        config: roots.system.join("background.toml"),
        accent: roots.shared.join("accent.toml"),
        media: roots.data.join("backgrounds"),
        cache: roots.cache.join("background-previews"),
        hyprpaper: hypr_config_dir().join("hyprpaper.conf"),
        units: dirs
            .get_config_home()
            .unwrap_or_default()
            .join("systemd/user"),
        home: std::env::var_os("HOME").map(PathBuf::from),
    }
}

/// File-backed shared theme state, serialized independently of system refreshes.
pub struct SharedTheme {
    path: PathBuf,
    default_mode: &'static str,
    pub value: Value,
}
impl SharedTheme {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            default_mode: "light",
            value: shared_theme::defaults("light"),
        }
    }

    pub fn read(&self) -> Result<Value, String> {
        if let Some(value) = load(&self.path)? {
            shared_theme::normalize(value)
        } else {
            let defaults = shared_theme::defaults(self.default_mode);
            write(&self.path, &defaults)?;
            Ok(defaults)
        }
    }

    fn refresh(&mut self, system_mode: Option<SystemThemeMode>) -> Result<(), String> {
        self.default_mode = match system_mode {
            Some(SystemThemeMode::Dark) => "dark",
            _ => "light",
        };
        self.value = self.read()?;
        Ok(())
    }

    fn set(&mut self, field: &str, value: &str) -> Result<Value, String> {
        let mut next = self.read()?;
        next["theme"][field] = json!(value);
        let next = shared_theme::normalize(next).map_err(|error| invalid(&error))?;
        write(&self.path, &next)?;
        self.value = next;
        Ok(self.value.clone())
    }
}

pub struct State {
    pub catalog: Value,
    pub appearance: DesktopTheme,
    pub date: Value,
    pub hostname: HostnameManager,
    pub account: AccountInformation,
    roots: Roots,
    saved: Value,
    pub theme_error: Option<String>,
    pub date_error: Option<String>,
    pub display: DisplayManager<Hyprctl>,
    pub night_light: NightLight<RealSunset>,
    fonts: FontManager<RealFontconfig>,
    pub default_apps: DefaultApps<default_apps::RealDesktop>,
    pub startup: Startup<startup::RealSystemd>,
    pub input: Input<RealHypr>,
    pub desktop: Input<RealHypr>,
    pub themes: ThemePacks,
    pub text_size: TextSize,
    pub language: Language<language::RealSystem>,
    pub appimages: applications::appimages::AppImages,
    pub network: NetworkManager,
    pub bluetooth: remya_bluetooth::Bluetooth,
    pub proxy: remya_network::proxy::Proxy,
    pub cameras: remya_camera::Cameras<remya_camera::native::Native>,
    pub sound: Sound<RealPulse>,
    pub power: remya_power::Power,
    pub updates: Updates,
    pub uninstalling: bool,
    pub storage: remya_storage::Storage,
    pub printers: Printers<RealCups>,
    pub auth_error: Option<String>,
    privileged: Option<Arc<Mutex<Session>>>,
    gui: Option<Child>,
    auth_attempted: bool,
    pub restart_suggested: bool,
    pub navigation: String,
    pub requests: crate::requests::Requests,
}
impl State {
    pub fn new(roots: Roots) -> Self {
        let catalog: Value =
            serde_json::from_str(include_str!("../../../assets/catalog.json")).expect("catalog");
        let requests = crate::requests::Requests::new(
            &roots,
            &catalog,
            BackgroundManager::new(RealDesktop, background_paths(&roots)),
        );
        Self {
            catalog,
            requests,
            appearance: DesktopTheme::new(roots.system.join("appearance.toml")),
            date: json!({}),
            hostname: HostnameManager::new(roots.system.join("hostname.toml"), atomic_write),
            account: AccountInformation::new(roots.system.join("avatar.png")),
            display: DisplayManager::new(
                Hyprctl,
                hypr_config_dir(),
                roots.system.join("display.toml"),
            ),
            night_light: NightLight::new(
                RealSunset,
                hypr_config_dir().join("hyprsunset.conf"),
                roots.system.join("night-light.toml"),
            ),
            fonts: FontManager::new(
                RealFontconfig,
                fonts::user_font_directory().unwrap_or_default(),
            ),
            default_apps: DefaultApps::new(
                default_apps::RealDesktop,
                roots.system.join("default-apps.toml"),
            ),
            startup: Startup::new(
                startup::RealSystemd,
                startup::Directories::detect(),
                roots.system.join("startup.toml"),
            ),
            input: Input::new(
                &input::INPUT,
                RealHypr,
                hypr_config_dir(),
                roots.system.join("input.toml"),
            ),
            desktop: Input::new(
                &input::DESKTOP,
                RealHypr,
                hypr_config_dir(),
                roots.system.join("desktop.toml"),
            ),
            themes: ThemePacks::new(roots.system.join("themes.toml")),
            text_size: TextSize::new(roots.system.join("text-size.toml")),
            language: Language::new(language::RealSystem, roots.system.join("language.toml")),
            bluetooth: remya_bluetooth::Bluetooth::new(),
            proxy: remya_network::proxy::Proxy::new(),
            cameras: remya_camera::Cameras::new(
                remya_camera::native::Native,
                roots.system.join("cameras.toml"),
            ),
            sound: Sound::new(RealPulse),
            power: remya_power::Power::new(
                roots.system.join("power.json"),
                hypr_config_dir().join("hypridle.conf"),
                roots.state.join("power.sqlite3"),
            ),
            updates: Updates::new(roots.system.join("updates.json"), atomic_write),
            uninstalling: false,
            storage: remya_storage::Storage::real(roots.system.join("cleanup.toml"))
                .expect("absolute HOME"),
            network: NetworkManager::real(roots.system.join("network.toml")),
            printers: Printers::new(RealCups),
            appimages: applications::appimages::AppImages::real(
                roots.system.join("appimages.toml"),
            )
            .expect("absolute HOME"),
            saved: Value::Null,
            theme_error: None,
            date_error: None,
            auth_error: None,
            privileged: None,
            gui: None,
            auth_attempted: false,
            restart_suggested: false,
            navigation: "home".into(),
            roots,
        }
    }
    fn date_file(&self) -> PathBuf {
        self.roots.system.join("date-time.toml")
    }
    pub fn refresh(&mut self) {
        self.appearance.refresh();
        self.theme_error = self.refresh_theme().err();
        self.date_error = self.refresh_date().err();
        let _ = self.hostname.refresh();
        let _ = self.default_apps.refresh();
        let _ = self.startup.refresh();
        let _ = self.input.refresh();
        let _ = self.desktop.refresh();
        let _ = self.themes.refresh();
        let _ = self.text_size.refresh();
        let _ = self.language.refresh();
        // Failures stay in the manager's state for every reader.
        let _ = self.network.refresh();
        // A detection failure is retained by the manager and reported in its state.
        let _ = self.display.refresh();
        // Failures stay in the manager's state for every reader.
        let _ = self.night_light.refresh();
        let _ = self.requests.background.lock().unwrap().refresh();
        let _ = self.sound.refresh();
        let _ = self.power.refresh();
        let _ = self.cameras.refresh();
    }
    fn refresh_theme(&mut self) -> Result<(), String> {
        self.requests
            .theme
            .lock()
            .unwrap()
            .refresh(self.appearance.system().mode)
    }
    pub fn theme_state(&self) -> Value {
        self.requests.theme.lock().unwrap().value.clone()
    }
    pub fn background_state(&self) -> Value {
        self.requests.background.lock().unwrap().state()
    }
    pub fn accent_state(&self) -> Value {
        self.requests.background.lock().unwrap().accent_state()
    }
    fn refresh_date(&mut self) -> Result<(), String> {
        let observed = clock::read_date_time()?;
        let actual = prefs(&observed);
        let file = self.date_file();
        let loaded = load(&file)?;
        let mut saved = actual.clone();
        if let Some(value) = loaded.as_ref() {
            if value["version"] != 1
                || !value.is_object()
                || value
                    .as_object()
                    .unwrap()
                    .keys()
                    .any(|k| !["version", "timezone", "ntp", "time_servers"].contains(&k.as_str()))
            {
                return Err("Invalid date/time configuration".into());
            }
            for (k, v) in value.as_object().unwrap() {
                if k != "version" {
                    saved[k] = v.clone();
                }
            }
        }
        let zone = saved["timezone"].as_str().ok_or("Invalid saved timezone")?;
        if !clock::list_timezones()?.iter().any(|z| z.id == zone)
            || !saved["ntp"].is_boolean()
            || saved["time_servers"]
                .as_array()
                .is_none_or(|v| v.iter().any(|s| s.as_str().is_none()))
        {
            return Err("Invalid saved date/time preferences".into());
        }
        self.saved = saved;
        self.set_date(observed)?;
        Ok(())
    }
    fn set_date(&mut self, state: clock::DateTimeState) -> Result<(), String> {
        let actual = prefs(&state);
        let mut map = serde_json::to_value(&state).map_err(|e| e.to_string())?;
        map["unix_micros"] = json!(state.unix_micros.to_string());
        map["saved_timezone"] = self.saved["timezone"].clone();
        map["saved"] = self.saved.clone();
        map["conflict"] = json!(actual != self.saved);
        map["restart_suggested"] = json!(self.restart_suggested);
        map["authorized"] = json!(self.privileged.is_some());
        map["authorizing"] =
            json!(self.auth_attempted && self.privileged.is_none() && self.auth_error.is_none());
        self.date = map;
        Ok(())
    }
    fn save_date(&mut self, state: clock::DateTimeState, keys: &[&str]) -> Result<Value, String> {
        let values = prefs(&state);
        let mut next = self.saved.clone();
        for key in keys {
            next[*key] = values[*key].clone();
        }
        let mut file = next.clone();
        file["version"] = json!(1);
        write(&self.date_file(), &file)?;
        self.saved = next;
        self.date_error = None;
        self.set_date(state)?;
        Ok(self.date.clone())
    }
    pub fn begin_authorization(&mut self) -> bool {
        if self.auth_attempted {
            return false;
        }
        self.auth_attempted = true;
        true
    }
    pub fn finish_authorization(&mut self, result: Result<Session, String>) {
        match result {
            Ok(session) => {
                self.privileged = Some(Arc::new(Mutex::new(session)));
                self.auth_error = None;
                self.date_error = None;
            }
            Err(error) => self.auth_error = Some(error),
        }
    }
    pub fn prepare_uninstall(
        &mut self,
        value: &str,
    ) -> Result<impl FnOnce() -> Result<Value, String> + Send + use<>, String> {
        if self.uninstalling {
            return Err("Busy:An application uninstall is already running".into());
        }
        let control = self
            .controls()
            .into_iter()
            .find(|control| control["id"] == applications::UNINSTALL)
            .ok_or_else(|| invalid("Unknown setting"))?;
        if control["availability"] != "available" {
            return Err(format!("Unavailable:{}", control["reason"]));
        }
        let removal: applications::Removal = serde_json::from_str(value)
            .map_err(|e| invalid(&format!("Invalid uninstall request: {e}")))?;
        let session = if applications::needs_authorization(&removal.id) {
            if self.updates.busy() {
                return Err("Busy:Wait for the system update operation to finish".into());
            }
            Some(
                self.privileged
                    .as_ref()
                    .ok_or_else(|| {
                        self.auth_error
                            .clone()
                            .unwrap_or("Open Settings and authorize system changes first".into())
                    })?
                    .clone(),
            )
        } else {
            None
        };
        self.uninstalling = true;
        Ok(move || match session {
            Some(shared) => shared
                .try_lock()
                .map_err(|_| "Busy:Another authorized operation is running")?
                .call(&Request::Uninstall { removal }),
            None => applications::uninstall(&removal, false),
        })
    }

    fn request(&mut self, request: Request) -> Result<Value, String> {
        if self.updates.busy() {
            return Err("Busy:Wait for the system update operation to finish".into());
        }
        let shared = self
            .privileged
            .as_ref()
            .ok_or_else(|| {
                self.auth_error
                    .clone()
                    .unwrap_or("Open Settings and authorize system changes first".into())
            })?
            .clone();
        let mut session = shared
            .try_lock()
            .map_err(|_| "Busy:Another authorized operation is running")?;
        let result = session.call(&request);
        if !session.is_alive() {
            self.privileged = None;
            self.auth_error = Some("Privileged service stopped; close and reopen the Settings window to authorize again".into());
        }
        result
    }
    fn privileged(&mut self, request: Request) -> Result<clock::DateTimeState, String> {
        serde_json::from_value(self.request(request)?).map_err(|e| e.to_string())
    }
    pub fn hostname_state(&self) -> Value {
        let mut value = self.hostname.state();
        value["authorized"] = json!(self.privileged.is_some());
        value["authorizing"] =
            json!(self.auth_attempted && self.privileged.is_none() && self.auth_error.is_none());
        value
    }
    fn rename_hostname(&mut self, name: &str, expected: String) -> Result<Value, String> {
        let result = self
            .request(Request::Hostname {
                expected_static: expected,
                hostname: name.into(),
            })
            .and_then(|value| serde_json::from_value(value).map_err(|e| e.to_string()));
        self.hostname.finish(name, result)?;
        Ok(self.hostname_state())
    }
    pub fn date_state(&self) -> Value {
        let mut date = self.date.clone();
        date["error"] = json!(self.date_error.as_ref().or(self.auth_error.as_ref()));
        date
    }
    pub fn configuration(&self) -> Value {
        json!({"appearance": self.appearance.state(),"date-time":self.date_state(),"display":self.display.state(),"night-light":self.night_light.state(),"hostname":self.hostname_state(),"background":self.background_state(),"default-apps":self.default_apps.state(),"startup":self.startup.state(),"input":self.input.state(),"language":self.language.state(),"desktop":self.desktop.state(),"themes":self.themes.state(),"text-size":self.text_size.state(),"network":self.network.state(),"sound":self.sound.state(),"power":self.power.state(),"cameras":self.cameras.state()})
    }
    pub fn controls(&self) -> Vec<&Value> {
        self.catalog["pages"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|p| p["sections"].as_array().unwrap())
            .flat_map(|s| s["controls"].as_array().unwrap())
            .collect()
    }
    pub fn execute(&mut self, method: &str, args: &[String]) -> Result<Value, String> {
        match method {
            "Replace" => Err(invalid("Replace requires an application setting id")),
            "Get" => {
                let key = args[0].as_str();
                match key {
                    "theme" => {
                        self.requests.theme.lock().unwrap().read()
                    }
                    "appearance" => {
                        self.appearance.refresh();
                        Ok(self.appearance.state())
                    }
                    "system" => {
                        self.appearance.refresh();
                        serde_json::to_value(self.appearance.system()).map_err(|error| error.to_string())
                    }
                    APPLY_TO_SYSTEM => {
                        self.appearance.refresh();
                        let mut control = self.controls().into_iter().find(|c| c["id"] == key).unwrap().clone();
                        control["appearance"] = self.appearance.state();
                        Ok(control)
                    }
                    STYLE | MODE | CONTRAST => {
                        let theme = self.requests.theme.lock().unwrap().read()?;
                        Ok(
                            json!({"id": key,"availability":"available","reason":self.controls().into_iter().find(|c|c["id"]==key).unwrap()["reason"],"value":theme["theme"][shared_theme::field(key).unwrap()]}),
                        )
                    }
                    "configuration" => {
                        self.refresh();
                        Ok(self.configuration())
                    }
                    "date-time" | ZONE | MANUAL | AUTO | SYNC | SERVERS => {
                        self.refresh_date()?;
                        let mut v = self.date.clone();
                        v["error"] = json!(self.date_error.as_ref().or(self.auth_error.as_ref()));
                        if key != "date-time" {
                            v["id"] = json!(key);
                            v["availability"] = json!("available");
                            v["value"] = match key {
                                ZONE => v["timezone"].clone(),
                                MANUAL => v["local_time"].clone(),
                                AUTO => v["ntp"].clone(),
                                SYNC => v["ntp_synchronized"].clone(),
                                _ => v["time_servers"].clone(),
                            };
                        }
                        Ok(v)
                    }
                    "display" => {
                        let _ = self.display.refresh();
                        Ok(self.display.state())
                    }
                    "display-brightness" => {
                        self.display.refresh()?;
                        Ok(self.display.brightness_state())
                    }
                    _ if key.starts_with("display-brightness:") => {
                        self.display.output_brightness(&key[19..])
                    }
                    remya_display::night_light::GAMMA => {
                        Ok(self.night_light.gamma_state())
                    }
                    _ if is_display_setting(key) => {
                        let _ = self.display.refresh();
                        let control = self
                            .controls()
                            .into_iter()
                            .find(|c| c["id"] == key)
                            .ok_or_else(|| invalid("Unknown setting"))?;
                        Ok(json!({
                            "id": key,
                            "availability": control["availability"],
                            "reason": control["reason"],
                            "value": self.display.setting_value(key),
                            "error": self.display.error(),
                        }))
                    }
                    "night-light" => {
                        let _ = self.night_light.refresh();
                        Ok(self.night_light.state())
                    }
                    _ if is_night_light_setting(key) => {
                        let _ = self.night_light.refresh();
                        let control = self
                            .controls()
                            .into_iter()
                            .find(|c| c["id"] == key)
                            .ok_or_else(|| invalid("Unknown setting"))?;
                        Ok(json!({
                            "id": key,
                            "availability": control["availability"],
                            "reason": control["reason"],
                            "value": self.night_light.setting_value(key),
                            "error": self.night_light.state()["error"],
                        }))
                    }
                    "background" => {
                        let _ = self.requests.background.lock().unwrap().refresh();
                        Ok(self.background_state())
                    }
                    "accent" => Ok(self.accent_state()),
                    _ if is_background_setting(key) || is_accent_setting(key) => {
                        let mut background = self.requests.background.lock().unwrap();
                        let _ = background.refresh();
                        let control = self
                            .controls()
                            .into_iter()
                            .find(|c| c["id"] == key)
                            .ok_or_else(|| invalid("Unknown setting"))?;
                        let value = if is_accent_setting(key) {
                            background.accent_value(key)
                        } else {
                            background.setting_value(key)
                        };
                        Ok(json!({
                            "id": key,
                            "availability": control["availability"],
                            "reason": control["reason"],
                            "value": value,
                            "error": background.error(),
                        }))
                    }
                    // Polled every second: answered from the last state, without a NM read.
                    "network-traffic" => Ok(self.network.traffic()),
                    "airplane" => remya_network::airplane::read(&self.bluetooth),
                    "proxy" => Ok(self.proxy.read()),
                    _ if remya_network::proxy::IDS.contains(&key) => self.proxy.get(key),
                    remya_network::airplane::ENABLED | remya_network::airplane::WIFI => {
                        let state = remya_network::airplane::read(&self.bluetooth)?;
                        let value = if key == remya_network::airplane::ENABLED { &state["enabled"] } else { &state["wifi_enabled"] };
                        Ok(json!({"id":key,"availability":"available","value":value}))
                    }
                    "bluetooth" => self.bluetooth.read(),
                    _ if remya_bluetooth::is_setting(key) => {
                        let state = self.bluetooth.read()?;
                        Ok(
                            json!({"id":key,"availability":"available","reason":"Uses system BlueZ.","value":state,"error":state["error"]}),
                        )
                    }
                    "network" => {
                        let _ = self.network.refresh();
                        Ok(self.network.state())
                    }
                    _ if is_network_setting(key)
                        && self
                            .controls()
                            .iter()
                            .any(|c| c["id"] == key && c["availability"] == "available") =>
                    {
                        let _ = self.network.refresh();
                        Ok(
                            json!({"id":key,"availability":"available","reason":"Uses NetworkManager through the Settings service.","value":self.network.read_value(key),"error":self.network.state()["error"]}),
                        )
                    }
                    "input" => {
                        let _ = self.input.refresh();
                        Ok(self.input.state())
                    }
                    "desktop" => {
                        let _ = self.desktop.refresh();
                        Ok(self.desktop.state())
                    }
                    _ if input::is_desktop_setting(key) => {
                        let _ = self.desktop.refresh();
                        Ok(json!({"id":key,"availability":"available","reason":"Uses Hyprland compositor options.","value":self.desktop.setting_value(key),"error":self.desktop.state()["error"]}))
                    }
                    "themes" => {
                        let _ = self.themes.refresh();
                        Ok(self.themes.state())
                    }
                    text_size::FEATURE => {
                        let _ = self.text_size.refresh();
                        Ok(self.text_size.state())
                    }
                    language::FEATURE => {
                        let _ = self.language.refresh();
                        Ok(self.language.state())
                    }
                    _ if language::is_setting(key) => {
                        let _ = self.language.refresh();
                        Ok(json!({"id":key,"availability":"available","reason":"Uses systemd-localed.","value":self.language.setting_value(key),"error":self.language.state()["error"]}))
                    }
                    language::LOCALES => self.language.locales(),
                    _ if key.starts_with(language::PREVIEW) => {
                        language::preview(&key[language::PREVIEW.len()..])
                    }
                    "keyboard-layouts" => Ok(input::xkb::Rules::load()?.catalog()),
                    _ if text_size::is_setting(key) => {
                        let _ = self.text_size.refresh();
                        Ok(json!({"id":key,"availability":"available","reason":"Uses the GNOME interface text-scaling-factor.","value":self.text_size.setting_value(),"error":self.text_size.state()["error"]}))
                    }
                    _ if key.starts_with(packs::PREVIEW) => self.themes.preview(&key[packs::PREVIEW.len()..]),
                    _ if packs::is_setting(key) => {
                        let _ = self.themes.refresh();
                        Ok(json!({"id":key,"availability":"available","reason":"Uses GNOME interface settings, GTK, KDE, XCursor and Hyprland.","value":self.themes.setting_value(key),"error":self.themes.state()["error"]}))
                    }
                    _ if key.starts_with(input::LOOKUP) => self.input.lookup(&key[input::LOOKUP.len()..]),
                    _ if input::is_setting(key) => {
                        let read = self.input.refresh();
                        if key == input::TOUCH_OUTPUT
                            && let Some(reason) = read.as_ref().err().map(String::as_str)
                                .or_else(|| self.input.touchscreen_unavailable())
                        {
                            return Ok(json!({"id":key,"availability":"unavailable","reason":reason,"value":null,"error":self.input.state()["error"]}));
                        }
                        Ok(json!({"id":key,"availability":"available","reason":"Uses Hyprland input settings and key bindings.","value":self.input.setting_value(key),"error":self.input.state()["error"]}))
                    }
                    "startup" => {
                        let _ = self.startup.refresh();
                        Ok(self.startup.state())
                    }
                    _ if key.starts_with("startup-candidates:") => self.startup.candidates(&key[19..]),
                    _ if startup::IDS.contains(&key) => {
                        let _ = self.startup.refresh();
                        Ok(json!({"id":key,"availability":"available","reason":"Uses XDG autostart entries started by UWSM.","value":self.startup.state()}))
                    }
                    "default-apps" => {
                        let _ = self.default_apps.refresh();
                        Ok(self.default_apps.state())
                    }
                    _ if key.starts_with("default-applications:") => {
                        self.default_apps.applications(&key[21..])
                    }
                    _ if key.starts_with("default-types:") => self.default_apps.search(&key[14..]),
                    _ if key.starts_with("default-candidates:") => self
                        .default_apps
                        .candidates(&key[19..].split(',').map(str::to_owned).collect::<Vec<_>>()),
                    _ if default_apps::IDS.contains(&key) => {
                        let _ = self.default_apps.refresh();
                        let mut state = self.default_apps.state();
                        state["id"] = json!(key);
                        state["availability"] = json!("available");
                        state["value"] = state["groups"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .find(|group| group["id"] == key)
                            .map(|group| {
                                json!(
                                    group["types"]
                                        .as_array()
                                        .unwrap()
                                        .iter()
                                        .map(|t| (
                                            t.as_str().unwrap().to_string(),
                                            state["system"][t.as_str().unwrap()].clone()
                                        ))
                                        .collect::<std::collections::BTreeMap<_, _>>()
                                )
                            })
                            .unwrap_or(Value::Null);
                        Ok(state)
                    }
                    _ if key.starts_with("calendar-date:") => Ok(
                        json!({"value":remya_date_time::format_calendar_date(&key[14..])?}),
                    ),
                    "input-method-tools" => applications::external_tools::read("time"),
                    _ if key.starts_with("external-tools:") => {
                        applications::external_tools::read(&key[15..])
                    }
                    _ if applications::external_tools::IDS.contains(&key) => {
                        applications::external_tools::read_setting(key)
                    }
                    "account-information" => self.account_information(),
                    _ if account::IDS.contains(&key) => self
                        .account_information()
                        .map(|state| AccountInformation::setting(key, state)),
                    "storage" | "cleanup" => Ok(self.storage.read(key)),
                    "printers" => Ok(self.printers.state()),
                    _ if printers::IDS.contains(&key) => Ok(self.printers.setting_value(key)),
                    _ if remya_storage::is_setting(key) => Ok(self.storage.read_setting(key)),
                    "updates" => {
                        self.updates.refresh();
                        Ok(self.updates.state())
                    }
                    _ if updates::is_setting(key) => {
                        let mut value = self.updates.state();
                        value["id"] = json!(key);
                        // Missing tools are a host limit; a missing root updater is reported by `available`/`reason`.
                        value["availability"] = json!(if value["missing"].as_array().is_some_and(Vec::is_empty) { "available" } else { "planned" });
                        Ok(value)
                    }
                    "cameras" => {
                        let _ = self.cameras.refresh();
                        Ok(self.cameras.state())
                    }
                    _ if remya_camera::is_setting(key) => {
                        let _ = self.cameras.refresh();
                        let mut value = self.cameras.state();
                        value["id"] = json!(key);
                        Ok(value)
                    }
                    "power" | "battery-saver" => Ok(self.power.read()),
                    _ if remya_power::is_setting(key) => Ok(self.power.read_setting(key)),
                    _ if key.starts_with("battery-history:") => self.power.query(&key[16..]),
                    "sound" => {
                        let _ = self.sound.refresh();
        let _ = self.power.refresh();
                        Ok(self.sound.state())
                    }
                    _ if sound::is_setting(key) => {
                        let _ = self.sound.refresh();
        let _ = self.power.refresh();
                        let mut value = self.sound.state();
                        value["id"] = json!(key);
                        value["availability"] = json!("available");
                        Ok(value)
                    }
                    "appimages" => Ok(self.appimages.state()),
                    _ if applications::appimages::is_setting(key)
                        && self.controls().iter().any(|c| c["id"] == key) =>
                    {
                        Ok(self.appimages.read_setting(key))
                    }
                    "fonts" => self.fonts.families(),
                    _ if key.starts_with("font-search:") => self.fonts.search(&key[12..]),
                    _ if key.starts_with("font:") => self.fonts.details(&key[5..]),
                    "timezones" => {
                        let zones = clock::list_timezones()?;
                        Ok(
                            json!({"options":zones.iter().map(|z|&z.id).collect::<Vec<_>>(),"labels":zones.iter().map(|z|(z.id.clone(),json!(z.label))).collect::<serde_json::Map<_,_>>()}),
                        )
                    }
                    "system-information" => {
                        let _ = self.hostname.refresh();
                        let mut value = remya_system_information::read();
                        value["hostname"] = self.hostname_state();
                        Ok(value)
                    }
                    "hostname" | hostname::RENAME => {
                        let _ = self.hostname.refresh();
                        let mut value = self.hostname_state();
                        if key == hostname::RENAME {
                            value["id"] = json!(key);
                            value["availability"] = json!("available");
                            value["value"] = value["static_hostname"].clone();
                        }
                        Ok(value)
                    }
                    _ if remya_system_information::IDS.contains(&key) => {
                        let snapshot = remya_system_information::read();
                        let mut field = snapshot["values"][key].clone();
                        field["id"] = json!(key);
                        field["availability"] = json!("available");
                        field["reason"] = json!("Read-only system information");
                        Ok(field)
                    }
                    "about" => Ok(
                        json!({"name":"Remy'a Core","version":env!("CARGO_PKG_VERSION"),"repository":"https://github.com/remyhuang03/remya-core","license":"PolyForm-Noncommercial-1.0.0","api_version":1}),
                    ),
                    "catalog" => {
                        let mut v = self.catalog.clone();
                        let pages = self.catalog["pages"].as_array().unwrap();
                        v["pages"]=json!(pages.iter().map(|p|json!({"id":p["id"],"title":p["title"],"category":p["category"]})).collect::<Vec<_>>());
                        // Summaries omit controls, so clients cannot count coverage themselves.
                        let controls = pages.iter().flat_map(|p| p["sections"].as_array().into_iter().flatten()).flat_map(|s| s["controls"].as_array().into_iter().flatten());
                        let (total, planned) = controls.fold((0, 0), |(t, n), c| (t + 1, n + usize::from(c["availability"] == "planned")));
                        v["coverage"] = json!({"supported": total - planned, "planned": planned, "total": total});
                        Ok(v)
                    }
                    _ if key.starts_with("page:") => self.catalog["pages"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|p| p["id"] == page_alias(&key[5..]))
                        .cloned()
                        .ok_or_else(|| invalid("Unknown page")),
                    _ => {
                        let c = self
                            .controls()
                            .into_iter()
                            .find(|c| c["id"] == key)
                            .ok_or_else(|| invalid("Unknown setting"))?;
                        Ok(
                            json!({"id":key,"availability":c["availability"],"reason":c["reason"],"value":null}),
                        )
                    }
                }
            }
            "Set" => self.set(&args[0], &args[1]),
            "List" => {
                let mut controls = self.controls().into_iter().map(|c| json!({"id":c["id"],"title":c["title"],"availability":c["availability"],"reason":c["reason"]})).collect::<Vec<_>>();
                controls.extend(self.requests.apps.lock().unwrap().controls());
                let start = args[0]
                    .parse::<usize>()
                    .map_err(|_| invalid("Invalid list cursor"))?;
                if start > controls.len() {
                    return Err(invalid("Invalid list cursor"));
                }
                let end = (start + 32).min(controls.len());
                Ok(
                    json!({"api_version":1,"next_cursor":if end<controls.len(){Some(end.to_string())}else{None},"controls":controls[start..end],"apps":self.requests.apps.lock().unwrap().list()}),
                )
            }
            "Search" => {
                let query = args[0].to_lowercase();
                let terms = query.split_whitespace().collect::<Vec<_>>();
                let mut pages = self.catalog["pages"].as_array().unwrap().iter().filter(|p|{let text=p.to_string().to_lowercase();terms.iter().all(|term|text.contains(term))}).map(|p|json!({"id":p["id"],"title":p["title"],"category":p["category"]})).collect::<Vec<_>>();
                pages.extend(self.requests.apps.lock().unwrap().search(&args[0]));
                Ok(json!({"pages":pages}))
            }
            "Open" => {
                let page = &args[0];
                if page == "repository" {
                    open_repository_url()?;
                } else {
                    if !["home", "about", "licenses"].contains(&page.as_str())
                        && !self.catalog["pages"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .any(|p| p["id"] == page_alias(page))
                    {
                        return Err(invalid("Unknown page"));
                    }
                    self.navigation = page.clone();
                    self.open_gui()?;
                    self.ensure_idle();
                }
                Ok(json!({"opened":page}))
            }
            "Manage" => self.manage(&args[0]),
            "Resolve" => self.resolve(&args[0], &args[1]),
            "Reload" => {
                self.refresh();
                if let Some(error) = &self.theme_error {
                    return Err(error.clone());
                }
                Ok(
                    json!({"theme":self.theme_state(),"system":self.appearance.system(),"configuration":self.configuration()}),
                )
            }
            "Restart" => {
                self.request(Request::Restart {})?;
                Ok(json!({"restarting":true}))
            }
            "Close" => {
                let bus = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE)
                    .map_err(|e| e.to_string())?;
                let owned = bus
                    .call_sync(
                        Some("org.freedesktop.DBus"),
                        "/org/freedesktop/DBus",
                        "org.freedesktop.DBus",
                        "NameHasOwner",
                        Some(&("top.remya.settings",).to_variant()),
                        None,
                        gio::DBusCallFlags::NONE,
                        5000,
                        gio::Cancellable::NONE,
                    )
                    .map_err(|e| e.to_string())?
                    .get::<(bool,)>()
                    == Some((true,));
                if owned {
                    bus.call_sync(
                        Some("top.remya.settings"),
                        "/top/remya/settings",
                        "org.gtk.Actions",
                        "Activate",
                        Some(
                            &(
                                "close",
                                Vec::<Variant>::new(),
                                std::collections::HashMap::<String, Variant>::new(),
                            )
                                .to_variant(),
                        ),
                        None,
                        gio::DBusCallFlags::NONE,
                        5000,
                        gio::Cancellable::NONE,
                    )
                    .map_err(|e| e.to_string())?;
                }
                Ok(json!({"closed":true}))
            }
            "Quit" if self.uninstalling => Err("Busy:Wait for the application uninstall to finish".into()),
            "Quit" if self.updates.busy() => Err("Busy:An update operation is running; wait for it to finish before quitting the service".into()),
            "Quit" if self.storage.busy() => Err("Busy:A storage operation is running; wait for it to finish before quitting the service".into()),
            "Quit" => Ok(json!({"quitting":true})),
            _ => Err(invalid("Unknown method")),
        }
    }
    pub fn ensure_idle(&self) {
        if !self.roots.system.join("power.json").exists() {
            return;
        }
        if let Err(error) = self.power.ensure_idle() {
            eprintln!("hypridle startup: {error}");
        }
    }

    pub fn account_information(&self) -> Result<Value, String> {
        let google = self.requests.google_account.avatar();
        let mut value = match google.path.as_deref().filter(|path| path.is_file()) {
            Some(path) => self.account.read_using(path)?,
            None => self.account.read()?,
        };
        value["google_avatar_sync"] = google.enabled.into();
        value["google_avatar_error"] = google.error.into();
        Ok(value)
    }

    fn set(&mut self, key: &str, value: &str) -> Result<Value, String> {
        if key == remya_power::idle::START_ID {
            if value != "start" {
                return Err(invalid("Use start to ensure hypridle is running"));
            }
            self.power.ensure_idle()?;
            return Ok(json!({"running":true}));
        }
        if key == remya_display::night_light::GAMMA {
            return self.night_light.set_gamma(value);
        }
        if let Some(output) = key.strip_prefix("display-brightness:") {
            return self.display.set_output_brightness(output, value);
        }
        if remya_power::is_setting(key) {
            return self.power.set(key, value);
        }
        let control = self
            .controls()
            .into_iter()
            .find(|c| c["id"] == key)
            .ok_or_else(|| invalid("Unknown setting"))?;
        if control["availability"] != "available" {
            return Err(format!("Unavailable:{}", control["reason"]));
        }
        if printers::IDS.contains(&key) {
            return self.printers.set(key, value);
        }
        if let Some(page) = linked_page(key) {
            if value != "open" {
                return Err(invalid("Use open to open the linked Settings page"));
            }
            return self.execute("Open", &[page.into()]);
        }
        if account::IDS.contains(&key) {
            self.account.set(key, value, atomic_write)?;
            return self.account_information();
        }
        if [
            remya_network::airplane::ENABLED,
            remya_network::airplane::WIFI,
        ]
        .contains(&key)
        {
            let result = remya_network::airplane::set(&self.bluetooth, key, value);
            let _ = self.network.refresh();
            return result;
        }
        if remya_network::proxy::IDS.contains(&key) {
            return self.proxy.set(key, value);
        }
        if remya_bluetooth::is_setting(key) {
            return self.bluetooth.set(key, value);
        }
        if is_network_setting(key) {
            return self.network.set(key, value);
        }
        if input::is_setting(key) {
            return self.input.set(key, value);
        }
        if input::is_desktop_setting(key) {
            return self.desktop.set(key, value);
        }
        if packs::is_setting(key) {
            return self.themes.set(key, value);
        }
        if text_size::is_setting(key) {
            return self.text_size.set(value);
        }
        if language::is_setting(key) {
            let values = self.language.plan(key, value)?;
            let written = self.request(Request::Language { values }).map(|_| ());
            return self.language.finish(written);
        }
        if startup::IDS.contains(&key) {
            return self.startup.set(key, value);
        }
        if default_apps::IDS.contains(&key) {
            return self.default_apps.set(key, value);
        }
        if remya_storage::is_setting(key) {
            if self.uninstalling {
                return Err("Busy:Wait for the application uninstall to finish".into());
            }
            if self.updates.busy() {
                return Err("Busy:Wait for the system update operation to finish".into());
            }
            let request = match key {
                "cleanup.package-cache" => {
                    let p: remya_storage::packages::PackageCache = remya_storage::payload(value)?;
                    remya_storage::packages::validate_keep(p.keep)?;
                    Some(Request::PackageCache {
                        keep: p.keep,
                        uninstalled: p.uninstalled,
                    })
                }
                "cleanup.orphans" => {
                    let p: remya_storage::packages::RemovePackages = remya_storage::payload(value)?;
                    Some(Request::RemovePackages {
                        packages: p.packages,
                    })
                }
                _ => None,
            };
            if let Some(request) = request {
                let session = self
                    .privileged
                    .as_ref()
                    .ok_or_else(|| {
                        self.auth_error
                            .clone()
                            .unwrap_or("Open Settings and authorize system changes first".into())
                    })?
                    .clone();
                return self.storage.start_privileged(move || {
                    session
                        .try_lock()
                        .map_err(|_| "Busy:Another authorized operation is running")?
                        .call(&request)
                });
            }
            return self.storage.set(key, value);
        }
        if updates::is_setting(key) {
            if self.uninstalling {
                return Err("Busy:Wait for the application uninstall to finish".into());
            }
            if self.storage.busy() {
                return Err("Busy:Wait for the storage operation to finish".into());
            }
            return self.updates.set(key, value);
        }
        if remya_camera::is_setting(key) {
            return self.cameras.set(key, value);
        }
        if sound::is_setting(key) {
            return self.sound.set(key, value);
        }
        if applications::appimages::is_setting(key) {
            return self.appimages.set(key, value);
        }
        if applications::external_tools::IDS.contains(&key) {
            if value != "open" {
                return Err(invalid("Use open to launch the configuration tool"));
            }
            return applications::external_tools::open(key);
        }
        if applications::IDS.contains(&key) {
            return Err("ReadOnly:Read installed-apps or installed-app:<desktop-id>".into());
        }
        if key == hostname::RENAME {
            hostname::validate_name(value)?;
            self.hostname.refresh()?;
            let expected = self.hostname.prepare(value)?;
            return self.rename_hostname(value, expected);
        }
        if remya_system_information::IDS.contains(&key) {
            return Err("ReadOnly:System information cannot be changed".into());
        }
        if is_display_setting(key) {
            return self.display.set(key, value);
        }
        if is_night_light_setting(key) {
            return self.night_light.set(key, value);
        }
        if is_background_setting(key) {
            return self.requests.background.lock().unwrap().set(key, value);
        }
        if is_accent_setting(key) {
            return self
                .requests
                .background
                .lock()
                .unwrap()
                .set_accent(key, value);
        }
        if fonts::IDS.contains(&key) {
            return match key {
                fonts::INSTALL => self.fonts.install(value),
                fonts::UNINSTALL => self.fonts.uninstall(value),
                _ => Err("ReadOnly:Use get font-search:<query> or get font:<family>".into()),
            };
        }
        if key == APPLY_TO_SYSTEM {
            if value != "apply" {
                return Err(invalid(
                    "Use apply to apply the Remy'a theme mode to the system",
                ));
            }
            let theme = self.requests.theme.lock().unwrap().read()?;
            let mode = if theme["theme"]["theme_mode"] == "dark" {
                SystemThemeMode::Dark
            } else {
                SystemThemeMode::Light
            };
            return self.appearance.apply(mode);
        }
        if let Some(field) = shared_theme::field(key) {
            let value = self.requests.theme.lock().unwrap().set(field, value)?;
            self.theme_error = None;
            return Ok(value);
        }
        self.refresh_date()?;
        let (request, keys) = match key {
            ZONE => {
                if !clock::list_timezones()?.iter().any(|z| z.id == value) {
                    return Err(invalid("Unknown time zone"));
                }
                (
                    Request::Timezone {
                        timezone: value.into(),
                    },
                    vec!["timezone"],
                )
            }
            AUTO => {
                let enabled = match value {
                    "true" => true,
                    "false" => false,
                    _ => return Err(invalid("Use true or false")),
                };
                (Request::Automatic { enabled }, vec!["ntp"])
            }
            SYNC => {
                if value != "sync" {
                    return Err(invalid("Use sync"));
                }
                (Request::Sync {}, vec![])
            }
            SERVERS => {
                let servers = serde_json::from_str::<Vec<String>>(value)
                    .map_err(|_| invalid("Expected JSON server array"))?;
                if servers.len() > 16
                    || servers
                        .iter()
                        .any(|s| s.is_empty() || s.contains([' ', '/', '\n']) || s.starts_with('-'))
                {
                    return Err(invalid("Invalid time servers"));
                }
                (Request::Servers { servers }, vec!["time_servers"])
            }
            MANUAL => {
                #[derive(serde::Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Manual {
                    local_time: String,
                    timezone: String,
                }
                let p: Manual = serde_json::from_str(value)
                    .map_err(|_| invalid("Expected local_time and timezone"))?;
                (
                    Request::Manual {
                        local_time: p.local_time,
                        timezone: p.timezone,
                    },
                    vec!["ntp"],
                )
            }
            _ => return Err(invalid("Unknown setting")),
        };
        let state = self.privileged(request)?;
        self.save_date(state, &keys)
    }
    fn manage(&mut self, feature: &str) -> Result<Value, String> {
        match feature {
            "display" => self.display.manage()?,
            "night-light" => self.night_light.manage()?,
            "input" => self.input.manage()?,
            "desktop" => self.desktop.manage()?,
            "themes" => self.themes.manage()?,
            "text-size" => self.text_size.manage()?,
            "language" => self.language.manage()?,
            "hostname" => self.hostname.manage()?,
            "default-apps" => self.default_apps.manage()?,
            "startup" => self.startup.manage()?,
            "network" => self.network.manage()?,
            "background" => self.requests.background.lock().unwrap().manage()?,
            "power" => self.power.manage()?,
            "cameras" => self.cameras.manage()?,
            "cleanup" => self.storage.manage()?,
            "appimages" => self.appimages.manage()?,
            "updates" => self.updates.manage()?,
            "appearance" => return self.appearance.manage(),
            "date-time" => {
                self.refresh_date()?;
                let mut snapshot = self.saved.clone();
                snapshot["version"] = json!(1);
                if !self.date_file().try_exists().map_err(|e| e.to_string())? {
                    write(&self.date_file(), &snapshot)?;
                }
            }
            _ => return Err(invalid("Unknown feature")),
        }
        self.execute("Get", &[feature.into()])
    }

    fn resolve(&mut self, feature: &str, source: &str) -> Result<Value, String> {
        if feature == "power" {
            return self.power.resolve(source);
        }
        if let Some(name) = feature.strip_prefix("cameras:") {
            return self.cameras.resolve(name, source);
        }
        if feature == "network" {
            return self.network.resolve(source);
        }
        if feature == "input" {
            return self.input.resolve(source);
        }
        if feature == "desktop" {
            return self.desktop.resolve(source);
        }
        if feature == "themes" {
            return self.themes.resolve(source);
        }
        if feature == text_size::FEATURE {
            return self.text_size.resolve(source);
        }
        if feature == language::FEATURE {
            return match self.language.resolve(source)? {
                Some(values) => {
                    let written = self.request(Request::Language { values }).map(|_| ());
                    self.language.finish(written)
                }
                None => Ok(self.language.state()),
            };
        }
        if feature == "startup" {
            return self.startup.resolve(source);
        }
        if feature == "default-apps" {
            return self.default_apps.resolve(source);
        }
        if feature == "hostname" {
            self.hostname.refresh()?;
            match source {
                "system" => self.hostname.adopt()?,
                "settings" => {
                    let name = self.hostname.saved()?;
                    let expected = self.hostname.expected()?;
                    self.rename_hostname(&name, expected)?;
                }
                _ => return Err(invalid("Unknown resolution")),
            }
            return Ok(self.configuration());
        }
        if feature == "display" {
            self.display.resolve(source)?;
            return Ok(self.configuration());
        }
        if feature == "night-light" {
            self.night_light.resolve(source)?;
            return Ok(self.configuration());
        }
        if feature == "background" {
            self.requests.background.lock().unwrap().resolve(source)?;
            return Ok(self.configuration());
        }
        if !["system", "settings"].contains(&source) {
            return Err(invalid("Unknown resolution"));
        }
        match feature {
            "appearance" => return self.appearance.resolve(source),
            "date-time" => {
                self.refresh_date()?;
                if source == "system" {
                    let state = clock::read_date_time()?;
                    self.save_date(state, &["timezone", "ntp", "time_servers"])?;
                } else {
                    let saved = self.saved.clone();
                    self.set(ZONE, saved["timezone"].as_str().unwrap())?;
                    if self.date["time_servers"] != saved["time_servers"] {
                        self.set(AUTO, "true")?;
                        self.set(SERVERS, &saved["time_servers"].to_string())?;
                    }
                    self.set(
                        AUTO,
                        if saved["ntp"] == true {
                            "true"
                        } else {
                            "false"
                        },
                    )?;
                }
            }
            _ => return Err(invalid("Unknown feature")),
        }
        Ok(self.configuration())
    }
    pub fn reap_gui(&mut self) {
        if self
            .gui
            .as_mut()
            .and_then(|p| p.try_wait().ok())
            .flatten()
            .is_some()
        {
            self.bluetooth.cleanup();
            if self.input.is_recording() {
                // A closed or crashed recorder must not leave compositor shortcuts suspended.
                let _ = self.input.stop_recording();
            }
            self.gui = None;
        }
    }
    pub fn reap_authorization(&mut self) -> bool {
        let dead = self.privileged.as_ref().is_some_and(|shared| {
            shared
                .try_lock()
                .is_ok_and(|mut session| !session.is_alive())
        });
        if dead {
            self.privileged = None;
            self.auth_error = Some("Privileged service stopped; close and reopen the Settings window to authorize again".into());
        }
        dead
    }
    pub fn shutdown(&mut self) {
        self.power.shutdown();
        if let Some(mut gui) = self.gui.take() {
            let _ = gui.kill();
            let _ = gui.wait();
        }
        self.privileged = None;
        self.updates.shutdown();
        self.appimages.shutdown();
        self.bluetooth.cleanup();
        // Never leave an unconfirmed display change behind when the service stops.
        let _ = self.display.revert_pending();
    }
    pub fn open_gui(&mut self) -> Result<(), String> {
        if self
            .gui
            .as_mut()
            .and_then(|p| p.try_wait().ok())
            .flatten()
            .is_some()
        {
            self.gui = None;
        }
        if self.gui.is_none() {
            if self.privileged.is_none() && self.auth_error.is_some() {
                self.auth_attempted = false;
                self.auth_error = None;
            }
            let binary = gio::DesktopAppInfo::new("top.remya.settings.desktop")
                .and_then(|app| app.commandline())
                .and_then(|command| settings_executable(&command.to_string_lossy()))
                .or_else(|| gio::glib::find_program_in_path("remya_settings"))
                .ok_or("Unavailable:Remy'a Settings is not installed")?;
            self.gui = Some(
                Command::new(binary)
                    .args(["--gui", &self.navigation])
                    .spawn()
                    .map_err(|e| e.to_string())?,
            );
        }
        Ok(())
    }
}
impl Drop for State {
    fn drop(&mut self) {
        if let Some(mut gui) = self.gui.take() {
            let _ = gui.kill();
            let _ = gui.wait();
        }
    }
}

const MAX_REAPED_CHILDREN: usize = 16;

/// Bounded nonblocking reaper for short-lived fire-and-forget subprocesses.
struct ChildReaper {
    children: Vec<Child>,
    running: bool,
    capacity: usize,
}

impl ChildReaper {
    const fn new(capacity: usize) -> Self {
        Self {
            children: Vec::new(),
            running: false,
            capacity,
        }
    }

    fn reap_finished(&mut self) -> usize {
        self.children
            .retain_mut(|child| matches!(child.try_wait(), Ok(None)));
        self.children.len()
    }

    fn spawn(reaper: &'static Mutex<Self>, command: &mut Command) -> Result<(), String> {
        let mut state = reaper.lock().unwrap_or_else(|e| e.into_inner());
        if state.reap_finished() >= state.capacity {
            return Err("Busy:Too many open requests in progress".into());
        }
        let child = command.spawn().map_err(|e| e.to_string())?;
        state.children.push(child);
        if !state.running {
            state.running = true;
            std::thread::spawn(move || Self::reap_loop(reaper));
        }
        Ok(())
    }

    fn reap_loop(reaper: &'static Mutex<Self>) {
        let mut delay = std::time::Duration::from_millis(50);
        loop {
            std::thread::sleep(delay);
            let mut state = reaper.lock().unwrap_or_else(|e| e.into_inner());
            if state.reap_finished() == 0 {
                state.running = false;
                return;
            }
            delay = (delay * 2).min(std::time::Duration::from_millis(500));
        }
    }

    #[cfg(test)]
    fn kill_all(&mut self) {
        for child in &mut self.children {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.children.clear();
    }
}

fn open_repository_url() -> Result<(), String> {
    static REAPER: std::sync::OnceLock<Mutex<ChildReaper>> = std::sync::OnceLock::new();
    let reaper = REAPER.get_or_init(|| Mutex::new(ChildReaper::new(MAX_REAPED_CHILDREN)));
    let mut command = Command::new("xdg-open");
    command
        .arg("https://github.com/remyhuang03/remya-settings")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    ChildReaper::spawn(reaper, &mut command)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn atomic_preferences_preserve_links_permissions_and_reject_malformed_files() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("config");
        let link = root.path().join("link");
        write(
            &target,
            &json!({"version":1,"theme":{"style_variant":"vibrant","theme_mode":"dark"}}),
        )
        .unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o640)).unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();
        write(&link, &json!({"version":1})).unwrap();
        assert!(link.is_symlink());
        assert_eq!(
            fs::metadata(&target).unwrap().permissions().mode() & 0o777,
            0o640
        );
        fs::write(&target, "bad [").unwrap();
        assert!(load(&target).is_err());
        assert_eq!(fs::read_to_string(&target).unwrap(), "bad [");
    }
    #[test]
    fn gui_desktop_command_resolves_env_wrappers_and_percent_escapes() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("Settings % home");
        fs::create_dir(&dir).unwrap();
        let binary = dir.join("remya_settings");
        fs::write(&binary, "#!/bin/sh\n").unwrap();
        fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).unwrap();
        let quoted = gio::glib::shell_quote(binary.to_str().unwrap().replace('%', "%%"));
        let quoted = quoted.to_str().unwrap();
        assert_eq!(
            settings_executable(&format!("/usr/bin/env {quoted} %U")),
            Some(binary.clone())
        );
        assert_eq!(settings_executable(quoted), Some(binary.clone()));
        fs::remove_file(binary).unwrap();
        assert_eq!(settings_executable(quoted), None);
        assert_eq!(settings_executable("'unterminated"), None);
    }

    #[test]
    fn manage_is_explicit_validated_and_returns_the_get_payload() {
        let root = tempfile::tempdir().unwrap();
        let roots = Roots::detect(Some(root.path().into())).unwrap();
        let path = roots.system.join("appimages.toml");
        let mut state = State::new(roots);
        state.execute("Get", &["appimages".into()]).unwrap();
        assert!(!path.exists());
        assert!(
            state
                .execute("Manage", &["unknown".into()])
                .unwrap_err()
                .starts_with("InvalidArgument:")
        );
        let managed = state.execute("Manage", &["appimages".into()]).unwrap();
        assert!(path.is_file());
        assert_eq!(
            managed,
            state.execute("Get", &["appimages".into()]).unwrap()
        );
    }

    #[test]
    fn unlocked_uninstall_preserves_guards_and_clears_them_on_failure() {
        let root = tempfile::tempdir().unwrap();
        let mut state = State::new(Roots::detect(Some(root.path().into())).unwrap());
        let value = json!({"id":"flatpak:user:../bad/stable","token":"test"}).to_string();
        let work = state.prepare_uninstall(&value).unwrap();
        assert!(state.uninstalling);
        assert!(state.prepare_uninstall(&value).is_err());
        for (method, args) in [
            ("Set", vec![updates::CHECK.into(), "check".into()]),
            ("Set", vec!["cleanup.package-cache".into(), "{}".into()]),
            ("Quit", vec![]),
        ] {
            assert!(
                state
                    .execute(method, &args)
                    .unwrap_err()
                    .starts_with("Busy:")
            );
        }
        // The malformed reference fails validation before any package command runs.
        assert!(work().unwrap_err().starts_with("InvalidArgument:"));
        state.uninstalling = false;
        let requests = state.requests.clone();
        let state = Mutex::new(state);
        assert!(
            requests
                .execute(&state, "Set", &[applications::UNINSTALL.into(), value])
                .unwrap()
                .unwrap_err()
                .starts_with("InvalidArgument:")
        );
        assert!(!state.lock().unwrap().uninstalling);
    }

    #[test]
    fn reading_theme_does_not_consume_a_pending_external_change() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("theme.toml");
        let mut theme = SharedTheme::new(path.clone());
        theme.refresh(Some(SystemThemeMode::Dark)).unwrap();
        assert_eq!(theme.value["theme"]["theme_mode"], "dark");
        let before = theme.value.clone();

        let next = shared_theme::defaults("light");
        write(&path, &next).unwrap();
        assert_eq!(theme.read().unwrap(), next);
        assert_eq!(theme.value, before);

        theme.refresh(None).unwrap();
        assert_eq!(theme.value, next);
        fs::write(path, "invalid").unwrap();
        assert!(theme.read().is_err());
        assert!(theme.refresh(None).is_err());
        assert_eq!(theme.value, next);
    }

    #[test]
    fn catalog_parity_bounds_and_unknown_operations() {
        let root = tempfile::tempdir().unwrap();
        let mut state = State::new(Roots::detect(Some(root.path().into())).unwrap());
        assert_eq!(
            state.execute("Get", &["page:date-time".into()]).unwrap()["id"],
            "date-time"
        );
        let coverage = &state.execute("Get", &["catalog".into()]).unwrap()["coverage"];
        assert!(coverage["supported"].as_u64().unwrap() > 0);
        assert_eq!(
            coverage["supported"].as_u64().unwrap() + coverage["planned"].as_u64().unwrap(),
            coverage["total"].as_u64().unwrap()
        );
        assert_eq!(
            state.execute("List", &["0".into()]).unwrap()["controls"]
                .as_array()
                .unwrap()
                .len(),
            32
        );
        assert!(
            state
                .execute("List", &["999999".into()])
                .unwrap_err()
                .starts_with("InvalidArgument:")
        );
        assert!(state.requests.is_app_setting("remya-bar.layout"));
        for id in [
            "installed-app:top.remya.bar",
            "font:Family.2",
            "ai.provider/name",
            "sound.volume",
        ] {
            assert!(!state.requests.is_app_setting(id), "{id}");
        }
        assert!(state.execute("Get", &["unknown".into()]).is_err());
        assert!(
            state
                .execute("Set", &["night-light.50840111".into(), "Set hours".into()])
                .unwrap_err()
                .starts_with("Unavailable:")
        );
    }

    #[test]
    fn child_reaper_reaps_short_lived_processes_and_bounds_concurrency() {
        let short_lived: &'static Mutex<ChildReaper> =
            Box::leak(Box::new(Mutex::new(ChildReaper::new(4))));

        assert!(
            ChildReaper::spawn(
                short_lived,
                &mut Command::new("/nonexistent/remya-test-opener")
            )
            .is_err()
        );
        {
            let mut guard = short_lived.lock().unwrap();
            assert_eq!(guard.reap_finished(), 0);
            assert!(!guard.running);
        }

        for _ in 0..4 {
            ChildReaper::spawn(short_lived, Command::new("/bin/sh").args(["-c", "exit 0"]))
                .unwrap();
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            let done = {
                let mut guard = short_lived.lock().unwrap();
                guard.reap_finished() == 0 && !guard.running
            };
            if done {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "reaper did not drain exited children in time"
            );
            std::thread::sleep(std::time::Duration::from_millis(20));
        }

        let bounded: &'static Mutex<ChildReaper> =
            Box::leak(Box::new(Mutex::new(ChildReaper::new(2))));
        ChildReaper::spawn(bounded, Command::new("/bin/sleep").arg("5")).unwrap();
        ChildReaper::spawn(bounded, Command::new("/bin/sleep").arg("5")).unwrap();
        let err = ChildReaper::spawn(bounded, Command::new("/bin/sleep").arg("5")).unwrap_err();
        assert!(err.starts_with("Busy:"), "{err}");

        {
            let mut guard = bounded.lock().unwrap();
            guard.kill_all();
        }
        ChildReaper::spawn(bounded, Command::new("/bin/sh").args(["-c", "exit 0"])).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            let done = {
                let mut guard = bounded.lock().unwrap();
                guard.reap_finished() == 0 && !guard.running
            };
            if done {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "reaper did not drain recovered child in time"
            );
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }
}
