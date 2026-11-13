//! Registration-driven application preferences. No application-specific schema lives here.
use glib::{KeyFile, KeyFileFlags};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

fn invalid(message: impl std::fmt::Display) -> String {
    format!("InvalidArgument:{message}")
}
fn unavailable(message: impl std::fmt::Display) -> String {
    format!("Unavailable:{message}")
}
fn slug(value: &str) -> bool {
    !value.is_empty()
        && value.split('-').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        })
}

/// Only plain registration prefixes belong to this store; colon-based queries do not.
pub fn setting_prefix(id: &str) -> Option<&str> {
    let (prefix, _) = id.split_once('.')?;
    (prefix.len() <= 64 && slug(prefix)).then_some(prefix)
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Page {
    id: String,
    title: String,
    keywords: Vec<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Setting {
    title: String,
    page: String,
    keywords: Vec<String>,
    default: Value,
    schema: Value,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Registration {
    version: u32,
    id: String,
    app: String,
    title: String,
    icon: String,
    config: String,
    pages: Vec<Page>,
    settings: BTreeMap<String, Setting>,
}
struct App {
    registration: Registration,
    validators: BTreeMap<String, jsonschema::Validator>,
}
impl App {
    fn load(text: &str, id: &str, reserved: &BTreeSet<String>) -> Result<Self, String> {
        let registration: Registration = serde_json::from_str(text).map_err(invalid)?;
        let r = &registration;
        if r.version != 1 || !slug(&r.id) || r.id.len() > 64 || r.id != id {
            return Err(invalid(
                "Expected version 1 and a valid id matching the filename",
            ));
        }
        if reserved.contains(&r.id) {
            return Err(invalid("Application id collides with a catalog prefix"));
        }
        if r.config.is_empty()
            || !r
                .config
                .bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
            || ["system", "shared", "core", "settings"].contains(&r.config.as_str())
        {
            return Err(invalid("Invalid or reserved config directory"));
        }
        if r.app.is_empty()
            || r.app.ends_with(".desktop")
            || !r
                .app
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
            || r.title.trim().is_empty()
            || r.icon.trim().is_empty()
            || r.icon.contains('/')
        {
            return Err(invalid("Invalid desktop id, title or icon name"));
        }
        let mut pages = BTreeSet::new();
        for page in &r.pages {
            if !slug(&page.id) || page.title.trim().is_empty() || !pages.insert(&page.id) {
                return Err(invalid("Invalid or duplicate page"));
            }
        }
        let mut validators = BTreeMap::new();
        for (key, setting) in &r.settings {
            if !slug(key) || setting.title.trim().is_empty() || !pages.contains(&setting.page) {
                return Err(invalid(format!("Invalid setting or page: {key}")));
            }
            if let Some(draft) = setting.schema.get("$schema")
                && draft != "https://json-schema.org/draft/2020-12/schema"
            {
                return Err(invalid("Only JSON Schema draft 2020-12 is supported"));
            }
            jsonschema::draft202012::meta::validate(&setting.schema).map_err(invalid)?;
            let validator = jsonschema::draft202012::options()
                .build(&setting.schema)
                .map_err(invalid)?;
            validator.validate(&setting.default).map_err(invalid)?;
            to_toml(&setting.default)?;
            validators.insert(key.clone(), validator);
        }
        Ok(Self {
            registration,
            validators,
        })
    }

    fn validate(&self, key: &str, value: &Value) -> Result<toml::Value, String> {
        self.validators
            .get(key)
            .ok_or_else(|| invalid("Unknown application setting"))?
            .validate(value)
            .map_err(invalid)?;
        to_toml(value)
    }
}

fn to_toml(value: &Value) -> Result<toml::Value, String> {
    toml::Value::try_from(value)
        .map_err(|e| invalid(format!("Value must be TOML-representable (no null): {e}")))
}

/// XDG data lookup order, shared by registrations and desktop files.
pub fn data_roots() -> Vec<PathBuf> {
    data_roots_from(|key| std::env::var_os(key))
}
fn data_roots_from(env: impl Fn(&str) -> Option<OsString>) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let home = env("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| {
            env("HOME")
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
                .map(|p| p.join(".local/share"))
        });
    roots.extend(home);
    let dirs = env("XDG_DATA_DIRS")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    roots.extend(std::env::split_paths(&dirs).filter(|p| p.is_absolute()));
    roots
}

type Write = fn(&Path, &[u8]) -> Result<(), String>;
pub struct Store {
    config: PathBuf,
    data: Vec<PathBuf>,
    reserved: BTreeSet<String>,
    apps: BTreeMap<String, App>,
    entries: Vec<Value>,
    write: Write,
}
impl Store {
    pub fn new(
        config: PathBuf,
        data: Vec<PathBuf>,
        reserved: BTreeSet<String>,
        write: Write,
    ) -> Self {
        let mut store = Self {
            config,
            data,
            reserved,
            apps: BTreeMap::new(),
            entries: Vec::new(),
            write,
        };
        store.rescan();
        store
    }

    pub fn rescan(&mut self) {
        self.apps.clear();
        self.entries.clear();
        let mut seen = BTreeSet::new();
        let mut configs = BTreeSet::new();
        for root in &self.data {
            let Ok(files) = fs::read_dir(root.join("remya/apps")) else {
                continue;
            };
            let mut files = files
                .filter_map(Result::ok)
                .map(|f| f.path())
                .collect::<Vec<_>>();
            files.sort();
            for path in files {
                if path.extension().is_none_or(|ext| ext != "json") {
                    continue;
                }
                let id = path.file_stem().unwrap().to_string_lossy().into_owned();
                if !seen.insert(id.clone()) {
                    continue;
                }
                let text = fs::read_to_string(&path).map_err(|e| e.to_string());
                let raw: Value = text
                    .as_ref()
                    .ok()
                    .and_then(|s| serde_json::from_str(s).ok())
                    .unwrap_or(Value::Null);
                let mut entry = json!({"id":id,"app":raw["app"],"title":raw["title"],"icon":raw["icon"],"pages":[]});
                let loaded = text.and_then(|text| App::load(&text, &id, &self.reserved));
                match loaded {
                    Ok(app) if configs.insert(app.registration.config.clone()) => {
                        let r = &app.registration;
                        entry = json!({"id":r.id,"app":r.app,"title":r.title,"icon":r.icon,"pages":r.pages});
                        self.apps.insert(id, app);
                    }
                    Ok(_) => entry["error"] = json!("Duplicate application config directory"),
                    Err(error) => entry["error"] = json!(error),
                }
                self.entries.push(entry);
            }
        }
    }

    fn app(&mut self, id: &str) -> Result<&App, String> {
        if !self.apps.contains_key(id) {
            self.rescan();
        }
        self.apps.get(id).ok_or_else(|| {
            unavailable(format!(
                "Application {id} is not registered; inspect List apps for errors"
            ))
        })
    }

    pub fn list(&self) -> Value {
        json!(self.entries)
    }

    pub fn controls(&self) -> Vec<Value> {
        self.apps
            .values()
            .flat_map(|app| {
                let r = &app.registration;
                r.settings.iter().map(|(key, setting)| {
                    json!({
                        "id":format!("{}.{key}", r.id),"title":setting.title,
                        "availability":"available","app":r.id,"page":setting.page
                    })
                })
            })
            .collect()
    }

    pub fn search(&self, query: &str) -> Vec<Value> {
        let query = query.to_lowercase();
        let terms = query.split_whitespace().collect::<Vec<_>>();
        let mut found = Vec::new();
        for app in self.apps.values() {
            let r = &app.registration;
            for page in &r.pages {
                let text = format!("{} {} {}", r.title, page.title, page.keywords.join(" "))
                    .to_lowercase();
                if terms.iter().all(|term| text.contains(term)) {
                    found.push(json!({"id":page.id,"title":page.title,"app":r.id,"page":page.id}));
                }
            }
            for (key, setting) in &r.settings {
                let page = r.pages.iter().find(|p| p.id == setting.page).unwrap();
                let text = format!(
                    "{} {} {} {}",
                    r.title,
                    setting.title,
                    setting.keywords.join(" "),
                    page.title
                )
                .to_lowercase();
                if terms.iter().all(|term| text.contains(term)) {
                    found.push(json!({"id":format!("{}.{key}", r.id),"title":setting.title,"app":r.id,"page":setting.page}));
                }
            }
        }
        found
    }

    pub fn get(&mut self, id: &str) -> Result<Value, String> {
        let (app_id, key) = id
            .split_once('.')
            .ok_or_else(|| invalid("Expected <app>.<key>"))?;
        let config = self.config.clone();
        let app = self.app(app_id)?;
        let setting = app
            .registration
            .settings
            .get(key)
            .ok_or_else(|| invalid("Unknown application setting"))?;
        let table = read(&config.join(&app.registration.config).join("settings.toml"))?;
        let value = match table.get(key) {
            Some(value) => serde_json::to_value(value).map_err(|e| e.to_string())?,
            None => setting.default.clone(),
        };
        app.validate(key, &value)
            .map_err(|e| format!("Invalid stored value for {id}: {e}"))?;
        Ok(payload(id, value))
    }

    pub fn set(&mut self, id: &str, value: &str, revision: Option<&str>) -> Result<Value, String> {
        let (app_id, key) = id
            .split_once('.')
            .ok_or_else(|| invalid("Expected <app>.<key>"))?;
        let config = self.config.clone();
        let app = self.app(app_id)?;
        let value: Value = serde_json::from_str(value).map_err(invalid)?;
        let stored = app.validate(key, &value)?;
        let path = config.join(&app.registration.config).join("settings.toml");
        let mut table = read(&path)?;
        if let Some(revision) = revision {
            let current = match table.get(key) {
                Some(value) => serde_json::to_value(value).map_err(|e| e.to_string())?,
                None => app.registration.settings[key].default.clone(),
            };
            app.validate(key, &current)?;
            if payload(id, current)["revision"] != revision {
                return Err("Changed in another view. Reopen the editor.".into());
            }
        }
        table.insert(key.into(), stored);
        let content = toml::to_string(&table).map_err(|e| e.to_string())?;
        (self.write)(&path, content.as_bytes())?;
        self.get(id)
    }

    pub fn open(&mut self, app_id: &str, page: &str) -> Result<Value, String> {
        let argv = self.open_argv(app_id, page)?;
        let mut child = Command::new(&argv[0])
            .args(&argv[1..])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()
            .map_err(|e| unavailable(format!("Cannot open {app_id}: {e}")))?;
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        Ok(json!({"opened":app_id,"page":page}))
    }

    fn open_argv(&mut self, app_id: &str, page: &str) -> Result<Vec<OsString>, String> {
        let app = self.app(app_id)?;
        let desktop = app.registration.app.clone();
        let pages = app
            .registration
            .pages
            .iter()
            .map(|p| p.id.clone())
            .collect::<Vec<_>>();
        if !page.is_empty() && !pages.iter().any(|p| p == page) {
            return Err(invalid("Unknown application page"));
        }
        let path = self
            .data
            .iter()
            .map(|root| root.join("applications").join(format!("{desktop}.desktop")))
            .find(|path| path.exists())
            .ok_or_else(|| {
                unavailable(format!("Desktop file {desktop}.desktop is not installed"))
            })?;
        let key = KeyFile::new();
        key.load_from_file(path, KeyFileFlags::NONE)
            .map_err(|e| unavailable(e.to_string()))?;
        let exec = key
            .string("Desktop Action settings", "Exec")
            .map_err(|e| unavailable(format!("Desktop Action settings Exec is missing: {e}")))?;
        settings_argv(&exec, page, &pages)
    }
}
use std::os::unix::process::CommandExt;

fn read(path: &Path) -> Result<toml::Table, String> {
    match fs::read_to_string(path) {
        Ok(text) => toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(toml::Table::new()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}
fn payload(id: &str, value: Value) -> Value {
    let revision = Sha256::digest(value.to_string().as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    json!({"id":id,"availability":"available","value":value,"revision":revision})
}

/// Parse a desktop Exec as argv, never as a shell command. Empty page opens settings home.
pub fn settings_argv(exec: &str, page: &str, pages: &[String]) -> Result<Vec<OsString>, String> {
    if !page.is_empty() && !pages.iter().any(|p| p == page) {
        return Err(invalid("Unknown application page"));
    }
    let parsed = glib::shell_parse_argv(exec).map_err(invalid)?;
    let mut argv = Vec::new();
    for (index, argument) in parsed.into_iter().enumerate() {
        let mut value = String::new();
        let mut chars = argument
            .to_str()
            .ok_or_else(|| invalid("Exec is not UTF-8"))?
            .chars();
        while let Some(c) = chars.next() {
            if c == '%' {
                match chars.next() {
                    Some('%') => value.push('%'),
                    Some(_) => (),
                    None => return Err(invalid("Incomplete desktop field code")),
                }
            } else {
                value.push(c);
            }
        }
        if index == 0 && value.is_empty() {
            return Err(unavailable("Empty settings executable"));
        }
        if !value.is_empty() || argument.is_empty() {
            argv.push(OsString::from(value));
        }
    }
    if argv.first().is_none_or(|s| s.is_empty()) {
        return Err(unavailable("Empty settings Exec"));
    }
    if !argv.iter().any(|s| s == "--settings") {
        argv.push("--settings".into());
    }
    if !page.is_empty() {
        argv.push(page.into());
    }
    Ok(argv)
}

#[cfg(test)]
mod tests;
