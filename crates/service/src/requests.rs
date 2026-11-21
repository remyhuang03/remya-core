//! Request paths whose work must not run under the system-feature State lock.
use crate::{
    ai,
    paths::Roots,
    state::{SharedTheme, State},
};
use remya_applications as applications;
use remya_background::{BackgroundManager, RealDesktop};
use remya_display::software_dimming::{OUTPUT_PREFIX, SoftwareDimming};
use serde_json::Value;
use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex},
};

pub const GOOGLE_ACCOUNT: &str = "google-account";
const GOOGLE_CONNECT: &str = "google-account.connect";
const GOOGLE_AVATAR_SYNC: &str = "google-account.avatar-sync";
const GOOGLE_DISCONNECT: &str = "google-account.disconnect";

pub fn is_google_account_write(id: &str) -> bool {
    [GOOGLE_CONNECT, GOOGLE_DISCONNECT, GOOGLE_AVATAR_SYNC].contains(&id)
}

#[derive(Clone)]
pub struct Requests {
    pub ai: Arc<ai::Ai>,
    pub google_account: Arc<remya_google_account::Account>,
    pub theme: Arc<Mutex<SharedTheme>>,
    pub background: Arc<Mutex<BackgroundManager<RealDesktop>>>,
    pub apps: Arc<Mutex<remya_app_preferences::Store>>,
    pub software_dimming: Arc<Mutex<SoftwareDimming>>,
    catalog_prefixes: Arc<BTreeSet<String>>,
}

impl Requests {
    pub fn new(roots: &Roots, catalog: &Value, background: BackgroundManager<RealDesktop>) -> Self {
        let mut prefixes = BTreeSet::from([GOOGLE_ACCOUNT.to_owned()]);
        for page in catalog["pages"].as_array().unwrap() {
            prefixes.insert(
                page["id"]
                    .as_str()
                    .unwrap()
                    .split('.')
                    .next()
                    .unwrap()
                    .to_owned(),
            );
            for section in page["sections"].as_array().unwrap() {
                for control in section["controls"].as_array().unwrap() {
                    prefixes.insert(
                        control["id"]
                            .as_str()
                            .unwrap()
                            .split('.')
                            .next()
                            .unwrap()
                            .to_owned(),
                    );
                }
            }
        }
        Self {
            theme: Arc::new(Mutex::new(SharedTheme::new(
                roots.shared.join("theme.toml"),
            ))),
            background: Arc::new(Mutex::new(background)),
            ai: Arc::new(ai::Ai::new(
                roots.shared.join("ai.toml"),
                roots.cache.join("models-dev.json"),
            )),
            google_account: Arc::new(remya_google_account::Account::new(
                roots.shared.join("google-oauth-client.json"),
                roots.cache.clone(),
            )),
            apps: Arc::new(Mutex::new(remya_app_preferences::Store::new(
                roots.system.parent().expect("config root").into(),
                remya_app_preferences::data_roots(),
                prefixes.clone(),
                remya_display::atomic_write,
            ))),
            software_dimming: Arc::default(),
            catalog_prefixes: Arc::new(prefixes),
        }
    }

    pub fn is_app_setting(&self, id: &str) -> bool {
        remya_app_preferences::setting_prefix(id)
            .is_some_and(|prefix| !self.catalog_prefixes.contains(prefix))
    }

    pub fn execute(
        &self,
        state: &Mutex<State>,
        method: &str,
        args: &[String],
    ) -> Option<Result<Value, String>> {
        let key = args.first().map(String::as_str).unwrap_or_default();
        if let Some(output) = key.strip_prefix(OUTPUT_PREFIX) {
            return match method {
                "Get" => Some(Ok(self.software_dimming.lock().unwrap().get(output))),
                "Set" => Some(self.software_dimming.lock().unwrap().set(output, &args[1])),
                _ => None,
            };
        }
        if ["Get", "Set", "Replace"].contains(&method) && self.is_app_setting(key) {
            let mut apps = self.apps.lock().unwrap();
            return Some(match method {
                "Get" => apps.get(key),
                "Set" => apps.set(key, &args[1], None),
                "Replace" => apps.set(key, &args[1], Some(&args[2])),
                _ => unreachable!(),
            });
        }
        match method {
            "GoogleTasks" => Some(self.google_account.tasks_request(key)),
            "OpenApp" => Some(self.apps.lock().unwrap().open(key, &args[1])),
            "Get" if ai::Ai::is_get(key) => Some(self.ai.get(key)),
            "Set" if ai::SET_IDS.contains(&key) => Some(self.ai.set(key, &args[1])),
            "Set" if is_google_account_write(key) => Some(self.write_google_account(key, &args[1])),
            "Get" => match key {
                GOOGLE_ACCOUNT => Some(Ok(serde_json::json!(self.google_account.status()))),
                "theme" => Some(self.theme.lock().unwrap().read()),
                "accent" => Some(Ok(self.background.lock().unwrap().accent_state())),
                "installed-apps" => Some(applications::read()),
                _ if key.starts_with("installed-apps:") => Some(applications::search(&key[15..])),
                _ if key.starts_with("installed-app:") => Some(applications::details(&key[14..])),
                _ if applications::IDS.contains(&key) => {
                    Some(applications::read().map(|mut value| {
                        value["id"] = key.into();
                        value["availability"] = "available".into();
                        value
                    }))
                }
                _ => None,
            },
            "Set" if key == applications::UNINSTALL => {
                let work = state.lock().unwrap().prepare_uninstall(&args[1]);
                let work_started = work.is_ok();
                let result = work.and_then(|work| work());
                let mut state = state.lock().unwrap();
                // A rejected concurrent uninstall must not clear the active one's guard.
                if work_started {
                    state.uninstalling = false;
                    state.reap_authorization();
                }
                Some(result)
            }
            _ => None,
        }
    }

    fn write_google_account(&self, id: &str, raw: &str) -> Result<Value, String> {
        match id {
            GOOGLE_CONNECT => {
                if !raw.trim_start().starts_with('{') {
                    return Err(
                        "InvalidArgument:Expected {calendar?:bool,tasks?:bool,profile?:bool}."
                            .into(),
                    );
                }
                let requested = serde_json::from_str(raw).map_err(
                    |_| "InvalidArgument:Expected {calendar?:bool,tasks?:bool,profile?:bool}.",
                )?;
                self.google_account.connect(requested)
            }
            GOOGLE_AVATAR_SYNC => {
                let enabled: bool =
                    serde_json::from_str(raw).map_err(|_| "InvalidArgument:Expected a boolean.")?;
                self.google_account.set_avatar_sync(enabled)
            }
            GOOGLE_DISCONNECT => {
                let value = serde_json::from_str::<Value>(raw)
                    .map_err(|_| "InvalidArgument:Expected an empty object.")?;
                if !value.as_object().is_some_and(serde_json::Map::is_empty) {
                    return Err("InvalidArgument:Expected an empty object.".into());
                }
                self.google_account.disconnect()
            }
            _ => unreachable!(),
        }
        .map(|status| serde_json::json!(status))
    }
}
