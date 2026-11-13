//! Request paths whose work must not run under the system-feature State lock.
use crate::{ai, paths::Roots, state::State};
use remya_applications as applications;
use serde_json::Value;
use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex},
};

#[derive(Clone)]
pub struct Requests {
    pub ai: Arc<ai::Ai>,
    pub apps: Arc<Mutex<remya_app_preferences::Store>>,
    catalog_prefixes: Arc<BTreeSet<String>>,
}

impl Requests {
    pub fn new(roots: &Roots, catalog: &Value) -> Self {
        let mut prefixes = BTreeSet::new();
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
            ai: Arc::new(ai::Ai::new(
                roots.shared.join("ai.toml"),
                roots.cache.join("models-dev.json"),
            )),
            apps: Arc::new(Mutex::new(remya_app_preferences::Store::new(
                roots.system.parent().expect("config root").into(),
                remya_app_preferences::data_roots(),
                prefixes.clone(),
                remya_display::atomic_write,
            ))),
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
            "OpenApp" => Some(self.apps.lock().unwrap().open(key, &args[1])),
            "Get" if ai::Ai::is_get(key) => Some(self.ai.get(key)),
            "Set" if ai::SET_IDS.contains(&key) => Some(self.ai.set(key, &args[1])),
            "Get" => match key {
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
}
