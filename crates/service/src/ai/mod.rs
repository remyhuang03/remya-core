//! AI providers: CodexBar usage and credentials, custom endpoints and the
//! Bar quota preferences, described with models.dev metadata.
mod codexbar;
mod models_dev;
mod prefs;
mod secrets;

use models_dev::Status;
use prefs::{Custom, Prefs};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, mpsc},
    time::{Duration, Instant, SystemTime},
};

pub const PROVIDERS: &str = "ai.providers";
pub const PROVIDER_PREFIX: &str = "ai.provider/";
pub const CREDENTIAL: &str = "ai.credential";
pub const CUSTOM: &str = "ai.custom";
/// Settable ids; each Set emits Changed for its own id and for `ai.providers`.
pub const SET_IDS: [&str; 3] = [PROVIDERS, CREDENTIAL, CUSTOM];

const TTL: Duration = Duration::from_secs(5 * 60);
const SOURCES: &[&str] = &["auto", "web", "cli", "oauth", "api"];
const MAX_SECRET: usize = 16 << 10;

fn invalid(message: &str) -> String {
    format!("InvalidArgument:{message}")
}

struct Probe {
    updated_at: String,
    usage: Vec<Value>,
}

#[derive(Default)]
struct ProbeCache {
    generation: u64,
    value: Option<Probe>,
    refreshing: bool,
    attempted: Option<Instant>,
    failed: bool,
    full_attempted: Option<Instant>,
}

impl ProbeCache {
    fn invalidate(&mut self) {
        self.generation += 1;
        self.attempted = None;
        self.failed = false;
    }

    fn state(&self) -> Value {
        json!({"status":if self.refreshing { "refreshing" } else if self.failed {
            "failed"
        } else { "ready" }, "error":if self.failed { Some("AI usage could not be read.") } else { None }})
    }
}

struct Snapshot {
    value: Value,
    visible: Vec<Value>,
    catalog: Vec<Value>,
    /// Sanitized probe rows.
    usage: Vec<Value>,
}

type Models = (Instant, Status, Option<Arc<Value>>);

#[derive(Default)]
struct ModelsCache {
    value: Option<Models>,
    refreshing: bool,
}

type RunProbe = dyn Fn(&Path, &str) -> Result<Vec<Value>, String> + Send + Sync;

pub struct Ai {
    prefs: PathBuf,
    cache: PathBuf,
    // The generation prevents an old in-flight probe from undoing a Set invalidation.
    probe: Mutex<ProbeCache>,
    models: Mutex<ModelsCache>,
    codexbar_writes: Mutex<()>,
    custom_writes: Mutex<()>,
    secret_bus: std::sync::OnceLock<gio::DBusConnection>,
    notifier: Mutex<Option<mpsc::Sender<()>>>,
    run_probe: Box<RunProbe>,
}

/// Removes account e-mail addresses at any depth before usage leaves CodexBar.
fn strip_email(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.retain(|key, _| !key.to_ascii_lowercase().ends_with("email"));
            map.values_mut().for_each(strip_email);
        }
        Value::Array(items) => items.iter_mut().for_each(strip_email),
        _ => {}
    }
}

fn credential_configured(dump: Option<&Value>) -> bool {
    dump.is_some_and(|entry| {
        ["apiKey", "cookieHeader"]
            .iter()
            .any(|key| entry[key].as_str().is_some_and(|s| !s.is_empty()))
            || entry["tokenAccounts"]
                .as_array()
                .is_some_and(|accounts| !accounts.is_empty())
            || entry["tokenAccounts"]
                .as_object()
                .is_some_and(|accounts| !accounts.is_empty())
    })
}

#[derive(Debug, PartialEq)]
enum Visibility {
    Visible(&'static str, Option<String>),
    Catalog,
    Hidden,
}

/// Ready usage, or a configured credential, makes a provider visible. Runtime
/// failures of providers without a Linux credential path stay hidden.
fn classify(provider: &Value, dump: Option<&Value>, usage: Option<&Value>) -> Visibility {
    let auth = &provider["authentication"];
    let error = usage.map(|row| &row["error"]).filter(|e| e.is_object());
    if usage.is_some_and(|row| row["usage"].is_object()) {
        return Visibility::Visible("ready", None);
    }
    if credential_configured(dump) {
        return match error {
            Some(error) => Visibility::Visible("error", Some(short_error(error))),
            None => Visibility::Visible("needs-setup", None),
        };
    }
    let supports =
        auth["supportsConfigAPIKey"] == true || auth["supportsConfigCookieHeader"] == true;
    if error.is_some_and(|e| e["kind"] == "runtime") && !supports {
        return Visibility::Hidden;
    }
    Visibility::Catalog
}

/// One bounded line, including for errors returned by provider detail reads.
fn short_error(error: &Value) -> String {
    let line = error["message"]
        .as_str()
        .unwrap_or("Usage unavailable")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if line.chars().count() > 200 {
        format!("{}…", line.chars().take(199).collect::<String>())
    } else {
        line
    }
}

fn summarize_usage(mut state: Value, visible: &[Value]) -> Value {
    let failures = visible
        .iter()
        .filter(|row| row["usageError"].is_string())
        .count();
    if failures > 0 {
        state["error"] = json!(format!(
            "Usage could not be read for {failures} tracked provider{}.",
            if failures == 1 { "" } else { "s" }
        ));
    }
    state
}

fn models_dev_id(id: &str, catalog: Option<&Value>) -> Option<String> {
    let alias = match id {
        "claude" => Some("anthropic"),
        "codex" => Some("openai"),
        "gemini" | "antigravity" => Some("google"),
        "moonshot" => Some("moonshotai"),
        "xai" | "grok" => Some("xai"),
        "vertexai" => Some("google-vertex"),
        _ => None,
    };
    alias.map(str::to_owned).or_else(|| {
        catalog
            .is_some_and(|catalog| catalog[id].is_object())
            .then(|| id.to_owned())
    })
}

fn window_label(minutes: Option<u64>) -> Option<String> {
    match minutes? {
        10080 => Some("Week".into()),
        1440 => Some("Day".into()),
        m if m > 0 && m < 1440 && m % 60 == 0 => Some(format!("{}h", m / 60)),
        _ => None,
    }
}

fn windows(usage: Option<&Value>) -> Vec<Value> {
    let Some(row) = usage.filter(|row| row["usage"].is_object()) else {
        return Vec::new();
    };
    ["primary", "secondary", "tertiary"]
        .into_iter()
        .filter_map(|slot| {
            let window = &row["usage"][slot];
            let used = window["usedPercent"].as_f64()?;
            let minutes = window["windowMinutes"].as_u64();
            let label = row["rateWindowLabels"][slot]
                .as_str()
                .map(str::to_owned)
                .or_else(|| window_label(minutes));
            Some(json!({"slot":slot,"label":label,"windowMinutes":minutes,
                "usedPercent":used,"resetsAt":window["resetsAt"].as_str()}))
        })
        .collect()
}

struct Inputs<'a> {
    providers: &'a [Value],
    dump: &'a [Value],
    usage: &'a [Value],
    custom: &'a [(Custom, bool)],
    models_dev: Option<&'a Value>,
}

fn codexbar_credential(provider: &Value, configured: Option<bool>) -> Value {
    let auth = &provider["authentication"];
    let mut credential = json!({"apiKey":auth["supportsConfigAPIKey"] == true,
        "cookie":auth["supportsConfigCookieHeader"] == true});
    if let Some(configured) = configured {
        credential["configured"] = json!(configured);
    }
    credential
}

fn source_modes(provider: &Value) -> Value {
    json!(
        provider["authentication"]["sourceModes"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    )
}

/// Returns the visible provider rows and the addable catalog.
fn build(inputs: &Inputs) -> (Vec<Value>, Vec<Value>) {
    let mut visible = Vec::new();
    let mut catalog = Vec::new();
    let mut mapped = HashSet::new();
    for provider in inputs.providers {
        let id = provider["provider"].as_str().unwrap_or_default();
        let dump = inputs.dump.iter().find(|entry| entry["id"] == id);
        let usage = inputs.usage.iter().find(|row| row["provider"] == id);
        let models_dev = models_dev_id(id, inputs.models_dev);
        mapped.extend(models_dev.clone());
        mapped.insert(id.to_owned());
        match classify(provider, dump, usage) {
            Visibility::Visible(status, error) => {
                let ready = usage.filter(|row| row["usage"].is_object());
                let source = ready
                    .and_then(|row| row["source"].as_str())
                    .or_else(|| dump.and_then(|entry| entry["source"].as_str()))
                    .unwrap_or("auto");
                let plan = ready.and_then(|row| {
                    row["usage"]["identity"]["loginMethod"]
                        .as_str()
                        .or_else(|| row["usage"]["loginMethod"].as_str())
                });
                visible.push(json!({
                    "id":id,"name":provider["displayName"],"kind":"codexbar","status":status,
                    "enabled":provider["enabled"] == true,
                    "source":source,"plan":plan,"sourceModes":source_modes(provider),
                    "credential":codexbar_credential(provider, Some(credential_configured(dump))),
                    "envVars":provider["authentication"]["environmentVariables"].as_array().cloned().unwrap_or_default(),
                    "error":error,"usageError":usage.and_then(|row| row["error"].as_object().map(|_| short_error(&row["error"]))),"baseUrl":null,"modelsDev":models_dev,"windows":windows(ready),
                }));
            }
            Visibility::Catalog => catalog.push(json!({
                "id":id,"name":provider["displayName"],"kind":"codexbar",
                "credential":codexbar_credential(provider, None),"sourceModes":source_modes(provider),
            })),
            Visibility::Hidden => {}
        }
    }
    for (custom, configured) in inputs.custom {
        visible.push(json!({
            "id":custom.id,"name":custom.name,"kind":"custom","status":"custom","enabled":true,
            "source":null,"plan":null,"sourceModes":[],
            "credential":{"apiKey":true,"cookie":false,"configured":configured},
            "envVars":[],"error":null,"usageError":null,"baseUrl":custom.base_url,"modelsDev":custom.models_dev,"windows":[],
        }));
    }
    let mut extra: Vec<Value> = inputs
        .models_dev
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter(|(id, provider)| provider["api"].is_string() && !mapped.contains(*id))
        .map(|(id, provider)| {
            json!({"id":format!("models.dev:{id}"),"name":provider["name"].as_str().unwrap_or(id),
                "kind":"models-dev","baseUrl":provider["api"],"modelsDev":id})
        })
        .collect();
    extra.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    catalog.extend(extra);
    (visible, catalog)
}

fn now_iso() -> String {
    let seconds = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    gio::glib::DateTime::from_unix_utc(seconds)
        .and_then(|time| time.format("%Y-%m-%dT%H:%M:%SZ"))
        .map(|text| text.to_string())
        .unwrap_or_default()
}

fn check_secret(secret: &str) -> Result<(), String> {
    if secret.is_empty() || secret.len() > MAX_SECRET || secret.chars().any(char::is_control) {
        return Err(invalid(
            "Credentials must be non-empty, at most 16 KiB, without control characters",
        ));
    }
    Ok(())
}

impl Ai {
    pub fn new(prefs: PathBuf, cache: PathBuf) -> Self {
        Self {
            prefs,
            cache,
            probe: Mutex::new(ProbeCache::default()),
            models: Mutex::new(ModelsCache::default()),
            codexbar_writes: Mutex::new(()),
            custom_writes: Mutex::new(()),
            secret_bus: std::sync::OnceLock::new(),
            notifier: Mutex::new(None),
            run_probe: Box::new(codexbar::provider_usage),
        }
    }

    pub fn set_secret_bus(&self, bus: gio::DBusConnection) {
        let _ = self.secret_bus.set(bus);
    }

    fn clear_secret(&self, id: &str) -> Result<(), String> {
        let bus = self
            .secret_bus
            .get()
            .ok_or("Unavailable:The Secret Service is unavailable.")?;
        secrets::clear(bus, id)
    }

    pub fn set_notifier(&self, notifier: mpsc::Sender<()>) {
        *self.notifier.lock().unwrap() = Some(notifier);
    }

    fn notify(&self) {
        if let Some(notifier) = &*self.notifier.lock().unwrap() {
            let _ = notifier.send(());
        }
    }

    fn probe(
        self: &Arc<Self>,
        path: &Path,
        providers: &[Value],
        dump: &[Value],
        automatic: bool,
    ) -> (Option<String>, Vec<Value>, Value) {
        let mut cache = self.probe.lock().unwrap();
        if !cache.refreshing && cache.attempted.is_none_or(|at| at.elapsed() >= TTL) {
            cache.refreshing = true;
            let generation = cache.generation;
            let mut relevant = Vec::new();
            let mut discovery = Vec::new();
            let full = automatic && cache.full_attempted.is_none_or(|at| at.elapsed() >= TTL);
            for provider in providers.iter().filter(|row| row["enabled"] == true) {
                let id = provider["provider"].as_str().unwrap();
                let configured = dump.iter().find(|entry| entry["id"] == id);
                let previous = cache
                    .value
                    .as_ref()
                    .and_then(|probe| probe.usage.iter().find(|row| row["provider"] == id));
                if credential_configured(configured)
                    || configured.is_some_and(|entry| entry["enabled"] == true)
                    || previous.is_some_and(|row| row["usage"].is_object())
                {
                    relevant.push(id.to_owned());
                } else if full {
                    discovery.push(id.to_owned());
                }
            }
            let phases = if discovery.is_empty() {
                vec![relevant]
            } else {
                vec![relevant, discovery]
            };
            let ai = self.clone();
            let path = path.to_owned();
            std::thread::spawn(move || {
                let mut succeeded = false;
                let last = phases.len() - 1;
                for (index, ids) in phases.into_iter().enumerate() {
                    if index == last {
                        let mut cache = ai.probe.lock().unwrap();
                        if cache.generation != generation {
                            cache.refreshing = false;
                            drop(cache);
                            ai.notify();
                            return;
                        }
                        if full {
                            cache.full_attempted = Some(Instant::now());
                        }
                    }
                    let mut rows = codexbar::probe(&path, &ids, &*ai.run_probe);
                    for row in &mut rows {
                        strip_email(row);
                        if row["error"].is_object() {
                            row["error"]["message"] = json!(short_error(&row["error"]));
                        }
                    }
                    let successful = rows.iter().any(|row| row["usage"].is_object());
                    succeeded |= successful;
                    let mut cache = ai.probe.lock().unwrap();
                    if cache.generation != generation {
                        cache.refreshing = false;
                        drop(cache);
                        ai.notify();
                        return;
                    }
                    let probe = cache.value.get_or_insert_with(|| Probe {
                        updated_at: String::new(),
                        usage: Vec::new(),
                    });
                    for mut row in rows {
                        if let Some(old) = probe
                            .usage
                            .iter_mut()
                            .find(|old| old["provider"] == row["provider"])
                        {
                            // Keep the last quota on failure, along with the new failure.
                            if !row["usage"].is_object() && old["usage"].is_object() {
                                let error = row["error"].clone();
                                row = old.clone();
                                row["error"] = error;
                            }
                            *old = row;
                        } else {
                            probe.usage.push(row);
                        }
                    }
                    if successful {
                        probe.updated_at = now_iso();
                    }
                    cache.failed = !succeeded;
                    if index == last {
                        cache.refreshing = false;
                        cache.attempted = Some(Instant::now());
                    }
                    drop(cache);
                    if !ids.is_empty() || index == last {
                        ai.notify();
                    }
                }
            });
        }
        (
            cache
                .value
                .as_ref()
                .map(|probe| probe.updated_at.clone())
                .filter(|at| !at.is_empty()),
            cache
                .value
                .as_ref()
                .map(|probe| probe.usage.clone())
                .unwrap_or_default(),
            cache.state(),
        )
    }

    fn models_dev(self: &Arc<Self>) -> (Status, Option<Arc<Value>>) {
        let mut cache = self.models.lock().unwrap();
        if cache.value.is_none() {
            let (status, value) = models_dev::resolve(Some(&self.cache), SystemTime::now(), || {
                Err("Background refresh required".into())
            });
            cache.value = Some((Instant::now(), status, value.map(Arc::new)));
            // A fresh disk cache needs no network refresh yet.
            if status != Status::Fresh {
                cache.value.as_mut().unwrap().0 = Instant::now() - TTL;
            }
        }
        let (at, status, value) = cache.value.as_ref().unwrap();
        let result = (*status, value.clone());
        if at.elapsed() >= TTL && !cache.refreshing {
            cache.refreshing = true;
            let ai = self.clone();
            std::thread::spawn(move || {
                let (status, value) =
                    models_dev::resolve(Some(&ai.cache), SystemTime::now(), models_dev::fetch);
                let mut cache = ai.models.lock().unwrap();
                cache.value = Some((Instant::now(), status, value.map(Arc::new)));
                cache.refreshing = false;
                drop(cache);
                ai.notify();
            });
        }
        result
    }

    fn snapshot(self: &Arc<Self>) -> Result<Snapshot, String> {
        let prefs = Prefs::load(&self.prefs)?;
        let path = codexbar::config_path()?;
        let version = codexbar::version()?;
        let automatic = !codexbar::configured(&path)?;
        let (providers, dump) = if version.is_some() {
            let dump = if automatic {
                Vec::new()
            } else {
                codexbar::cli(&path, &["config", "validate"])?;
                codexbar::dump(&path)?
            };
            (codexbar::providers(&path)?, dump)
        } else {
            (Vec::new(), Vec::new())
        };
        let (updated_at, mut usage, usage_state) = self.probe(&path, &providers, &dump, automatic);
        let custom: Vec<(Custom, bool)> = prefs
            .custom
            .iter()
            .map(|custom| (custom.clone(), secrets::exists(&custom.id)))
            .collect();
        let (_, models_dev) = self.models_dev();
        let (visible, catalog) = build(&Inputs {
            providers: &providers,
            dump: &dump,
            usage: &usage,
            custom: &custom,
            models_dev: models_dev.as_deref(),
        });
        let usage_state = summarize_usage(usage_state, &visible);
        usage.retain(|row| {
            visible
                .iter()
                .any(|provider| provider["id"] == row["provider"])
        });
        let value = json!({
            "codexbar":{"available":version.is_some(),"version":version},
            "updatedAt":updated_at,
            "usage":usage_state,
            "providers":visible,
            "catalog":catalog,
        });
        Ok(Snapshot {
            value,
            visible,
            catalog,
            usage,
        })
    }

    fn reply(self: &Arc<Self>) -> Result<Value, String> {
        let value = self.snapshot()?.value;
        Ok(json!({"id":PROVIDERS,"availability":"available","value":value}))
    }

    pub fn is_get(key: &str) -> bool {
        key == PROVIDERS || key.starts_with(PROVIDER_PREFIX)
    }

    pub fn get(self: &Arc<Self>, key: &str) -> Result<Value, String> {
        let Some(id) = key.strip_prefix(PROVIDER_PREFIX) else {
            return self.reply();
        };
        let Snapshot {
            visible,
            catalog,
            usage,
            ..
        } = self.snapshot()?;
        let provider = visible
            .into_iter()
            .chain(catalog)
            .find(|row| row["id"] == id)
            .ok_or_else(|| invalid("Unknown AI provider"))?;
        let usage = usage.into_iter().find(|row| row["provider"] == id);
        let models_dev_id = provider["modelsDev"].as_str().map(str::to_owned);
        let (status, catalog) = self.models_dev();
        let entry = models_dev_id
            .as_deref()
            .and_then(|id| catalog.as_deref().map(|catalog| &catalog[id]))
            .filter(|entry| entry.is_object());
        Ok(json!({"id":key,"availability":"available","value":{
            "provider":provider,"usage":usage,
            "modelsDev":{"status":status.name(),
                "provider":entry.map(models_dev::provider_summary),
                "models":entry.map(models_dev::models).unwrap_or_default()},
        }}))
    }

    pub fn set(self: &Arc<Self>, key: &str, request: &str) -> Result<Value, String> {
        match key {
            PROVIDERS => self.set_provider(request),
            CREDENTIAL => self.set_credential(request),
            CUSTOM => self.set_custom(request),
            _ => return Err(invalid("Unknown setting")),
        }?;
        self.probe.lock().unwrap().invalidate();
        self.reply()
    }

    fn codexbar_provider(path: &Path, id: &str) -> Result<(bool, Vec<Value>, Value), String> {
        let automatic = !codexbar::configured(path)?;
        if !automatic {
            codexbar::cli(path, &["config", "validate"])?;
        }
        let rows = codexbar::providers(path)?;
        let row = rows
            .iter()
            .find(|row| row["provider"] == id && codexbar::editable(row))
            .cloned()
            .ok_or_else(|| invalid("Unknown or unsupported AI provider"))?;
        Ok((automatic, rows, row))
    }

    fn set_provider(&self, request: &str) -> Result<(), String> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Change {
            id: String,
            enabled: Option<bool>,
            source: Option<String>,
        }
        let change: Change = serde_json::from_str(request)
            .ok()
            .filter(|c: &Change| c.enabled.is_some() || c.source.is_some())
            .ok_or_else(|| invalid("Expected a provider id with enabled and/or source"))?;
        let path = codexbar::config_path()?;
        let _write = self.codexbar_writes.lock().unwrap();
        let (automatic, rows, row) = Self::codexbar_provider(&path, &change.id)?;
        if let Some(source) = &change.source
            && (!SOURCES.contains(&source.as_str())
                || !source_modes(&row)
                    .as_array()
                    .is_some_and(|modes| modes.iter().any(|mode| mode == source)))
        {
            return Err(invalid("Unsupported source for this provider"));
        }
        if automatic {
            codexbar::initialize(&path, &rows, &change.id, change.enabled.unwrap_or(true))?;
        } else if let Some(enabled) = change.enabled {
            let action = if enabled { "enable" } else { "disable" };
            codexbar::cli(&path, &["config", action, "--provider", &change.id])?;
        }
        if let Some(source) = &change.source {
            codexbar::cli(
                &path,
                &[
                    "config",
                    "set-source",
                    "--provider",
                    &change.id,
                    "--source",
                    source,
                ],
            )?;
        }
        let enabled_kept = change.enabled.is_none_or(|enabled| {
            codexbar::providers(&path).is_ok_and(|rows| {
                rows.iter()
                    .any(|row| row["provider"] == change.id.as_str() && row["enabled"] == enabled)
            })
        });
        let source_kept = change.source.as_deref().is_none_or(|source| {
            codexbar::dump(&path).is_ok_and(|dump| {
                dump.iter().any(|entry| {
                    entry["id"] == change.id.as_str()
                        && entry["source"].as_str().unwrap_or("auto") == source
                })
            })
        });
        if !enabled_kept || !source_kept {
            return Err("Unavailable:CodexBar did not retain the requested provider state.".into());
        }
        Ok(())
    }

    fn set_credential(&self, request: &str) -> Result<(), String> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields, rename_all = "camelCase")]
        struct Change {
            id: String,
            api_key: Option<String>,
            cookie: Option<String>,
            #[serde(default)]
            clear: bool,
        }
        let change: Change = serde_json::from_str(request)
            .ok()
            .filter(|c: &Change| {
                [c.api_key.is_some(), c.cookie.is_some(), c.clear]
                    .iter()
                    .filter(|set| **set)
                    .count()
                    == 1
            })
            .ok_or_else(|| {
                invalid("Expected a provider id with exactly one of apiKey, cookie or clear")
            })?;
        for secret in [&change.api_key, &change.cookie].into_iter().flatten() {
            check_secret(secret)?;
        }
        if change.id.starts_with("custom-") {
            let _write = self.custom_writes.lock().unwrap();
            if !Prefs::load(&self.prefs)?
                .custom
                .iter()
                .any(|c| c.id == change.id)
            {
                return Err(invalid("Unknown AI provider"));
            }
            return match (&change.api_key, change.cookie.is_some()) {
                (Some(key), _) => secrets::store(&change.id, key),
                (None, true) => Err(invalid("Custom endpoints accept only an API key")),
                (None, false) => self.clear_secret(&change.id),
            };
        }
        let path = codexbar::config_path()?;
        let _write = self.codexbar_writes.lock().unwrap();
        let (automatic, rows, row) = Self::codexbar_provider(&path, &change.id)?;
        let auth = &row["authentication"];
        if (change.api_key.is_some() && auth["supportsConfigAPIKey"] != true)
            || (change.cookie.is_some() && auth["supportsConfigCookieHeader"] != true)
        {
            return Err(invalid("This provider does not accept that credential"));
        }
        if automatic {
            codexbar::initialize(&path, &rows, &change.id, true)?;
        }
        let id = change.id.as_str();
        match (&change.api_key, &change.cookie) {
            (Some(key), _) => codexbar::cli_stdin(
                &path,
                &["config", "set-api-key", "--provider", id, "--stdin"],
                key,
            )?,
            (None, Some(cookie)) => codexbar::cli_stdin(
                &path,
                &["config", "set-cookie", "--provider", id, "--stdin"],
                cookie,
            )?,
            (None, None) => {
                codexbar::cli(&path, &["config", "clear-credential", "--provider", id])?
            }
        };
        let dump = codexbar::dump(&path)?;
        let entry = dump.iter().find(|entry| entry["id"] == id);
        let has = |key: &str| entry.is_some_and(|entry| entry[key].is_string());
        let kept = match (&change.api_key, &change.cookie) {
            (Some(_), _) => has("apiKey"),
            (None, Some(_)) => has("cookieHeader"),
            (None, None) => !has("apiKey") && !has("cookieHeader"),
        };
        if !kept {
            return Err("Unavailable:CodexBar did not retain the credential change.".into());
        }
        Ok(())
    }

    fn set_custom(&self, request: &str) -> Result<(), String> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields, rename_all = "camelCase")]
        struct Add {
            name: String,
            base_url: String,
            models_dev: Option<String>,
            api_key: Option<String>,
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Change {
            add: Option<Add>,
            remove: Option<String>,
        }
        let change: Change = serde_json::from_str(request)
            .ok()
            .filter(|c: &Change| c.add.is_some() != c.remove.is_some())
            .ok_or_else(|| invalid("Expected exactly one of add or remove"))?;
        // Serialize keyring and preference changes together for custom endpoints.
        let _write = self.custom_writes.lock().unwrap();
        let mut prefs = Prefs::load(&self.prefs)?;
        if let Some(id) = change.remove {
            let before = prefs.custom.len();
            prefs.custom.retain(|custom| custom.id != id);
            if prefs.custom.len() == before {
                return Err(invalid("Unknown custom provider"));
            }
            self.clear_secret(&id)?;
            return prefs.save(&self.prefs);
        }
        let add = change.add.expect("add or remove");
        let name = prefs::validate_name(&add.name).map_err(|e| invalid(&e))?;
        let id =
            prefs::custom_id(name).ok_or_else(|| invalid("The name needs letters or digits"))?;
        prefs::validate_base_url(&add.base_url).map_err(|e| invalid(&e))?;
        if add
            .models_dev
            .as_deref()
            .is_some_and(|id| !prefs::valid_models_dev_id(id))
        {
            return Err(invalid("Invalid models.dev provider id"));
        }
        if prefs.custom.iter().any(|custom| custom.id == id) {
            return Err(invalid("A custom provider with this name already exists"));
        }
        if let Some(key) = &add.api_key {
            check_secret(key)?;
        }
        prefs.custom.push(Custom {
            id: id.clone(),
            name: name.to_owned(),
            base_url: add.base_url,
            models_dev: add.models_dev,
        });
        if let Some(key) = &add.api_key {
            secrets::store(&id, key)?;
        }
        prefs.save(&self.prefs).inspect_err(|_| {
            if add.api_key.is_some() {
                let _ = self.clear_secret(&id);
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider(id: &str, api_key: bool, cookie: bool) -> Value {
        json!({"provider":id,"displayName":id.to_uppercase(),"enabled":false,"defaultEnabled":false,
            "authentication":{"supportsConfigAPIKey":api_key,"supportsConfigCookieHeader":cookie,
                "environmentVariables":["KEY"],"sourceModes":["api","auto"]}})
    }

    fn fixture() -> (Vec<Value>, Vec<Value>, Vec<Value>) {
        let providers = vec![
            provider("claude", false, true),
            provider("openrouter", true, false),
            provider("cursor", false, true),
            provider("macos", false, false),
            provider("zai", true, false),
        ];
        let dump = vec![
            json!({"id":"claude","enabled":true}),
            json!({"id":"openrouter","enabled":true,"apiKey":"[REDACTED]"}),
            json!({"id":"cursor","enabled":true,"cookieHeader":"[REDACTED]","cookieSource":"manual"}),
        ];
        let mut usage = vec![
            json!({"provider":"claude","source":"oauth","rateWindowLabels":{"secondary":"Weekly"},
                "usage":{"identity":{"loginMethod":"Claude Pro","accountEmail":"a@example.com"},
                "accountEmail":"a@example.com",
                "primary":{"usedPercent":22,"windowMinutes":300,"resetsAt":"2026-11-11T09:20:00Z"},
                "secondary":{"usedPercent":29.5,"windowMinutes":10080,"resetsAt":null},
                "tertiary":null}}),
            json!({"provider":"openrouter","source":"auto","error":{"kind":"provider","code":1,"message":"Bad key"}}),
            json!({"provider":"macos","source":"auto","error":{"kind":"runtime","code":1,"message":"macOS only"}}),
            json!({"provider":"zai","source":"auto","error":{"kind":"provider","code":1,"message":"No strategy"}}),
        ];
        usage.iter_mut().for_each(strip_email);
        (providers, dump, usage)
    }

    #[test]
    fn strips_emails_recursively() {
        let mut row = json!({"usage":{"identity":{"accountEmail":"x","loginMethod":"Pro"},
            "accounts":[{"email":"y","id":1}],"accountEmail":"x"}});
        strip_email(&mut row);
        assert!(!row.to_string().contains("mail"), "{row}");
        assert_eq!(row["usage"]["identity"]["loginMethod"], "Pro");
    }

    #[test]
    fn classifies_visible_catalog_and_hidden_providers() {
        let (providers, dump, usage) = fixture();
        let custom = [(
            Custom {
                id: "custom-local".into(),
                name: "Local".into(),
                base_url: "http://localhost:11434/v1".into(),
                models_dev: None,
            },
            false,
        )];
        let models = json!({"anthropic":{"api":null},"groq":{"name":"Groq","api":"https://api.groq.com/openai/v1"},
            "openrouter":{"name":"OpenRouter","api":"https://openrouter.ai/api/v1"},"nokey":{"name":"No API"}});
        let (visible, catalog) = build(&Inputs {
            providers: &providers,
            dump: &dump,
            usage: &usage,
            custom: &custom,
            models_dev: Some(&models),
        });
        let status = |id: &str| {
            visible
                .iter()
                .find(|row| row["id"] == id)
                .map(|row| row["status"].as_str().unwrap().to_owned())
        };
        assert_eq!(status("claude").as_deref(), Some("ready"));
        assert_eq!(status("openrouter").as_deref(), Some("error"));
        assert_eq!(status("cursor").as_deref(), Some("needs-setup"));
        assert_eq!(status("custom-local").as_deref(), Some("custom"));
        assert_eq!(status("zai"), None);
        let claude = visible.iter().find(|row| row["id"] == "claude").unwrap();
        assert_eq!(claude["plan"], "Claude Pro");
        assert_eq!(claude["source"], "oauth");
        assert_eq!(claude["modelsDev"], "anthropic");
        assert_eq!(
            claude["credential"],
            json!({"apiKey":false,"cookie":true,"configured":false})
        );
        assert_eq!(
            claude["windows"],
            json!([{"slot":"primary","label":"5h","windowMinutes":300,"usedPercent":22.0,"resetsAt":"2026-11-11T09:20:00Z"},
                   {"slot":"secondary","label":"Weekly","windowMinutes":10080,"usedPercent":29.5,"resetsAt":null}])
        );
        let openrouter = visible
            .iter()
            .find(|row| row["id"] == "openrouter")
            .unwrap();
        assert_eq!(openrouter["error"], "Bad key");
        assert_eq!(openrouter["credential"]["configured"], true);
        assert_eq!(openrouter["windows"], json!([]));
        let ids: Vec<_> = catalog
            .iter()
            .map(|row| row["id"].as_str().unwrap())
            .collect();
        assert_eq!(
            ids,
            ["zai", "models.dev:groq"],
            "macOS-only runtime failures stay hidden"
        );
        assert!(
            !serde_json::to_string(&visible)
                .unwrap()
                .contains("example.com")
        );
    }

    #[test]
    fn tracking_uses_codexbar_enabled_flags() {
        let (mut providers, _, usage) = fixture();
        providers[0]["enabled"] = json!(true);
        let (visible, _) = build(&Inputs {
            providers: &providers,
            dump: &[],
            usage: &usage,
            custom: &[],
            models_dev: None,
        });
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0]["enabled"], true);
        assert_eq!(visible[0]["modelsDev"], "anthropic");
    }

    #[test]
    fn shared_preferences_complete_while_ai_probes_and_system_work_are_blocked() {
        use crate::{paths::Roots, state::State};
        use std::{fs, sync::mpsc, thread};

        let root = tempfile::tempdir().unwrap();
        let roots = Roots::detect(Some(root.path().into())).unwrap();
        // A fresh on-disk models.dev cache keeps this test entirely offline.
        fs::create_dir_all(&roots.cache).unwrap();
        fs::write(roots.cache.join("models-dev.json"), "{}").unwrap();
        let data = root.path().join("data");
        fs::create_dir_all(data.join("remya/apps")).unwrap();
        fs::write(
            data.join("remya/apps/remya-bar.json"),
            json!({
                "version":1,"id":"remya-bar","app":"top.remya.bar","title":"Bar",
                "icon":"top.remya.bar","config":"bar",
                "pages":[{"id":"clock","title":"Clock","keywords":[]}],
                "settings":{"clock-seconds":{"title":"Seconds","page":"clock","keywords":[],
                    "default":false,"schema":{"type":"boolean"}}}
            })
            .to_string(),
        )
        .unwrap();
        let mut state = State::new(roots);
        state.requests.apps = Arc::new(Mutex::new(remya_app_preferences::Store::new(
            root.path().join("config"),
            vec![data],
            Default::default(),
            remya_display::atomic_write,
        )));
        let (entered, started) = mpsc::channel();
        let (release, wait) = mpsc::channel();
        let wait = Mutex::new(wait);
        let ai = Arc::get_mut(&mut state.requests.ai).unwrap();
        ai.probe.get_mut().unwrap().value = Some(Probe {
            updated_at: "previous".into(),
            usage: vec![json!({"provider":"codex","usage":{}})],
        });
        ai.probe.get_mut().unwrap().full_attempted = Some(Instant::now());
        ai.run_probe = Box::new(move |_, id| {
            if id != "codex" {
                return Err("not configured".into());
            }
            entered.send(()).unwrap();
            wait.lock().unwrap().recv().unwrap();
            Ok(vec![json!({"provider":id,"usage":{}})])
        });
        let requests = state.requests.clone();
        let state = Arc::new(Mutex::new(state));
        let (replied, replies) = mpsc::channel();
        let (changed, changes) = mpsc::channel();
        requests.ai.set_notifier(changed);
        // Concurrent Gets return while exactly one usage probe is blocked.
        for _ in 0..2 {
            let requests = requests.clone();
            let state = state.clone();
            let replied = replied.clone();
            thread::spawn(move || {
                replied
                    .send(requests.execute(&state, "Get", &[PROVIDERS.into()]))
                    .unwrap();
            });
        }
        started.recv_timeout(Duration::from_secs(2)).unwrap();
        for _ in 0..2 {
            assert_eq!(
                replies
                    .recv_timeout(Duration::from_secs(2))
                    .expect("Get waited for usage")
                    .unwrap()
                    .unwrap()["value"]["usage"]["status"],
                "refreshing"
            );
        }
        assert!(started.try_recv().is_err());
        let system_work = state.lock().unwrap();
        let (finished, result) = mpsc::channel();
        let app_state = state.clone();
        let app_requests = requests.clone();
        let app = thread::spawn(move || {
            let theme = app_requests
                .execute(&app_state, "Get", &["theme".into()])
                .unwrap()
                .unwrap();
            let accent = app_requests
                .execute(&app_state, "Get", &["accent".into()])
                .unwrap()
                .unwrap();
            assert_eq!(theme["version"], 1);
            assert!(accent["color"].as_str().is_some());
            let key = "remya-bar.clock-seconds".to_string();
            let get = app_requests
                .execute(&app_state, "Get", std::slice::from_ref(&key))
                .unwrap()
                .unwrap();
            let set = app_requests
                .execute(&app_state, "Set", &[key.clone(), "true".into()])
                .unwrap()
                .unwrap();
            let replace = app_requests
                .execute(
                    &app_state,
                    "Replace",
                    &[
                        key,
                        "false".into(),
                        set["revision"].as_str().unwrap().into(),
                    ],
                )
                .unwrap()
                .unwrap();
            finished.send((get, set, replace)).unwrap();
        });
        let reply = result.recv_timeout(Duration::from_secs(2));
        drop(system_work);
        release.send(()).unwrap();
        changes.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(changes.try_recv().is_err());
        app.join().unwrap();
        let (get, set, replace) = reply.expect("shared preferences waited for AI or State");
        assert_eq!(get["value"], false);
        assert_eq!(set["value"], true);
        assert_eq!(replace["value"], false);
        // The completed probe is reused for five minutes rather than running again.
        requests
            .execute(&state, "Get", &[PROVIDERS.into()])
            .unwrap()
            .unwrap();
        assert!(started.try_recv().is_err());
    }

    fn tracked_providers(ids: &[&str]) -> (Vec<Value>, Vec<Value>) {
        (
            ids.iter()
                .map(|id| {
                    let mut row = provider(id, true, false);
                    row["enabled"] = json!(true);
                    row
                })
                .collect(),
            ids.iter()
                .map(|id| json!({"id":id,"enabled":true}))
                .collect(),
        )
    }

    #[test]
    fn a_probe_started_before_invalidation_cannot_repopulate_the_cache() {
        let root = tempfile::tempdir().unwrap();
        let mut ai = Ai::new(root.path().join("ai.toml"), root.path().join("models.json"));
        let (providers, dump) = tracked_providers(&["codex"]);
        let (entered, started) = mpsc::channel();
        let (release, wait) = mpsc::channel();
        let wait = Mutex::new(wait);
        ai.run_probe = Box::new(move |_, id| {
            entered.send(()).unwrap();
            let value = wait.lock().unwrap().recv().unwrap();
            Ok(vec![json!({"provider":id,"usage":{"value":value}})])
        });
        let (changed, changes) = mpsc::channel();
        ai.set_notifier(changed);
        let ai = Arc::new(ai);
        let probe = || ai.probe(Path::new("unused"), &providers, &dump, false);
        probe();
        started.recv_timeout(Duration::from_secs(2)).unwrap();
        ai.probe.lock().unwrap().invalidate();
        release.send("old").unwrap();
        changes.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(ai.probe.lock().unwrap().value.is_none());
        probe();
        started.recv_timeout(Duration::from_secs(2)).unwrap();
        release.send("new").unwrap();
        changes.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(probe().1[0]["usage"]["value"], "new");
        assert!(changes.try_recv().is_err());
    }

    #[test]
    fn failed_refresh_keeps_usage_and_backs_off() {
        let root = tempfile::tempdir().unwrap();
        let mut ai = Ai::new(root.path().join("ai.toml"), root.path().join("models.json"));
        let (providers, dump) = tracked_providers(&["codex"]);
        let quota = json!({"primary":{"usedPercent":22}});
        ai.probe.get_mut().unwrap().value = Some(Probe {
            updated_at: "previous".into(),
            usage: vec![json!({"provider":"codex","usage":quota})],
        });
        let (changed, changes) = mpsc::channel();
        ai.set_notifier(changed);
        let (entered, started) = mpsc::channel();
        let (release, wait) = mpsc::channel();
        let wait = Mutex::new(wait);
        ai.run_probe = Box::new(move |_, _| {
            entered.send(()).unwrap();
            wait.lock().unwrap().recv().unwrap()
        });
        let ai = Arc::new(ai);
        for result in [
            Err("offline".into()),
            Ok(vec![
                json!({"provider":"codex","error":{"message":"offline"}}),
            ]),
        ] {
            ai.probe.lock().unwrap().attempted = None;
            assert_eq!(
                ai.probe(Path::new("unused"), &providers, &dump, false).2["status"],
                "refreshing"
            );
            started.recv_timeout(Duration::from_secs(2)).unwrap();
            release.send(result).unwrap();
            changes.recv_timeout(Duration::from_secs(2)).unwrap();
            let (updated, usage, state) = ai.probe(Path::new("unused"), &providers, &dump, false);
            assert_eq!(updated.as_deref(), Some("previous"));
            assert_eq!(usage[0]["usage"], quota);
            assert_eq!(usage[0]["error"]["message"], "offline");
            assert_eq!(
                state,
                json!({"status":"failed","error":"AI usage could not be read."})
            );
            assert!(
                started.try_recv().is_err(),
                "failed probes must back off for the TTL"
            );
            assert!(changes.try_recv().is_err());
        }
    }

    #[test]
    fn partial_failure_is_ready_and_errors_are_bounded_on_tracked_rows() {
        for (all_failed, claude_failed) in [(false, true), (true, true), (false, false)] {
            let root = tempfile::tempdir().unwrap();
            let mut ai = Ai::new(root.path().join("ai.toml"), root.path().join("models.json"));
            let (providers, dump) = tracked_providers(&["codex", "claude", "irrelevant"]);
            let mut dump = dump[..2].to_vec();
            for entry in &mut dump {
                entry["apiKey"] = json!("[REDACTED]");
            }
            let dump = &dump;
            ai.run_probe = Box::new(move |_, id| {
                if (id == "codex" && !all_failed) || (id == "claude" && !claude_failed) {
                    Ok(vec![json!({"provider":id,"usage":{}})])
                } else {
                    Err("offline\n".repeat(100))
                }
            });
            let (changed, changes) = mpsc::channel();
            ai.set_notifier(changed);
            let ai = Arc::new(ai);
            ai.probe(Path::new("unused"), &providers, dump, true);
            // Relevant rows publish before discovery completes, then the full result.
            changes.recv_timeout(Duration::from_secs(2)).unwrap();
            changes.recv_timeout(Duration::from_secs(2)).unwrap();
            let (_, usage, state) = ai.probe(Path::new("unused"), &providers, dump, true);
            assert_eq!(state["status"], if all_failed { "failed" } else { "ready" });
            let (visible, catalog) = build(&Inputs {
                providers: &providers,
                dump,
                usage: &usage,
                custom: &[],
                models_dev: None,
            });
            assert_eq!(visible.len(), 2);
            let claude = visible.iter().find(|row| row["id"] == "claude").unwrap();
            if claude_failed {
                let error = claude["usageError"].as_str().unwrap();
                assert_eq!(error.chars().count(), 200);
                assert!(!error.contains('\n'));
            } else {
                assert!(claude["usageError"].is_null());
            }
            let summary = summarize_usage(state, &visible);
            assert_eq!(
                summary["error"],
                if all_failed {
                    json!("Usage could not be read for 2 tracked providers.")
                } else if claude_failed {
                    json!("Usage could not be read for 1 tracked provider.")
                } else {
                    Value::Null
                }
            );
            assert!(catalog.iter().all(|row| row["usageError"].is_null()));
            assert_eq!(
                visible.iter().find(|row| row["id"] == "codex").unwrap()["usageError"].is_null(),
                !all_failed
            );
        }
    }

    #[test]
    fn probe_concurrency_is_bounded_and_relevant_rows_publish_before_discovery() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let root = tempfile::tempdir().unwrap();
        let mut ai = Ai::new(root.path().join("ai.toml"), root.path().join("models.json"));
        let ids: Vec<_> = (0..24).map(|i| format!("provider{i}")).collect();
        let (providers, dump) =
            tracked_providers(&ids.iter().map(String::as_str).collect::<Vec<_>>());
        let dump = &dump[..1];
        let active = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let calls = Arc::new(Mutex::new(Vec::new()));
        let (entered, started) = mpsc::channel();
        let (release, wait) = mpsc::channel();
        let wait = Arc::new(Mutex::new(wait));
        let counter = active.clone();
        let maximum = peak.clone();
        let recorded = calls.clone();
        ai.run_probe = Box::new(move |_, id| {
            recorded.lock().unwrap().push(id.to_owned());
            let count = counter.fetch_add(1, Ordering::SeqCst) + 1;
            maximum.fetch_max(count, Ordering::SeqCst);
            if id != "provider0" {
                entered.send(()).unwrap();
                wait.lock().unwrap().recv().unwrap();
            }
            counter.fetch_sub(1, Ordering::SeqCst);
            Ok(vec![json!({"provider":id,"usage":{}})])
        });
        let (changed, changes) = mpsc::channel();
        ai.set_notifier(changed);
        let ai = Arc::new(ai);
        ai.probe(Path::new("unused"), &providers, dump, true);
        changes.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(
            ai.probe.lock().unwrap().value.as_ref().unwrap().usage.len(),
            1
        );
        for _ in 0..5 {
            started.recv_timeout(Duration::from_secs(2)).unwrap();
        }
        assert_eq!(active.load(Ordering::SeqCst), 5);
        assert!(started.try_recv().is_err());
        for _ in 1..ids.len() {
            release.send(()).unwrap();
        }
        changes.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(active.load(Ordering::SeqCst), 0);
        assert!(peak.load(Ordering::SeqCst) <= 6);
        assert_eq!(calls.lock().unwrap().len(), ids.len());
        // Invalidation refreshes known usage but cannot repeat full discovery within TTL.
        ai.probe.lock().unwrap().invalidate();
        ai.probe
            .lock()
            .unwrap()
            .value
            .as_mut()
            .unwrap()
            .usage
            .truncate(1);
        calls.lock().unwrap().clear();
        ai.probe(Path::new("unused"), &providers, dump, true);
        changes.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(*calls.lock().unwrap(), vec!["provider0"]);
        // Once the full-probe TTL expires, discovery runs after the subset again.
        ai.probe.lock().unwrap().full_attempted = Some(Instant::now() - TTL);
        ai.probe.lock().unwrap().attempted = None;
        ai.probe(Path::new("unused"), &providers, dump, true);
        changes.recv_timeout(Duration::from_secs(2)).unwrap();
        for _ in 0..5 {
            started.recv_timeout(Duration::from_secs(2)).unwrap();
        }
        for _ in 1..ids.len() {
            release.send(()).unwrap();
        }
        changes.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(calls.lock().unwrap().len(), ids.len() + 1);
        assert!(peak.load(Ordering::SeqCst) <= 6);
    }

    #[test]
    fn custom_add_validates_url_and_unique_id() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ai.toml");
        let ai = Ai::new(path.clone(), directory.path().join("models-dev.json"));
        let add = |name: &str, url: &str| json!({"add":{"name":name,"baseUrl":url}}).to_string();
        ai.set_custom(&add("My Endpoint", "https://api.example.com/v1"))
            .unwrap();
        assert!(
            ai.set_custom(&add("my endpoint!", "https://other.example.com"))
                .is_err()
        );
        assert!(
            ai.set_custom(&add("Plain", "http://api.example.com"))
                .is_err()
        );
        ai.set_custom(&add("Local", "http://127.0.0.1:8080/v1"))
            .unwrap();
        assert!(
            ai.set_custom(r#"{"add":{"name":"X","baseUrl":"https://x.dev"},"remove":"custom-x"}"#)
                .is_err()
        );
        let prefs = Prefs::load(&path).unwrap();
        let ids: Vec<_> = prefs.custom.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, ["custom-my-endpoint", "custom-local"]);
    }

    #[test]
    fn secrets_are_validated() {
        assert!(check_secret("sk-test").is_ok());
        assert!(check_secret("").is_err());
        assert!(check_secret("a\nb").is_err());
        assert!(check_secret(&"a".repeat(MAX_SECRET + 1)).is_err());
    }
}
