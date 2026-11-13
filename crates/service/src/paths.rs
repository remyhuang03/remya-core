//! Explicit roots for Core-owned files; relative XDG values are ignored.
use std::path::PathBuf;

pub struct Roots {
    pub system: PathBuf,
    pub shared: PathBuf,
    pub cache: PathBuf,
    pub data: PathBuf,
    pub state: PathBuf,
}

impl Roots {
    pub fn detect(debug: Option<PathBuf>) -> Result<Self, String> {
        Self::from_environment(debug, |name| std::env::var_os(name).map(PathBuf::from))
    }

    fn from_environment(
        debug: Option<PathBuf>,
        env: impl Fn(&str) -> Option<PathBuf>,
    ) -> Result<Self, String> {
        if let Some(repo) = debug {
            let config = repo.join("config");
            return Ok(Self {
                system: config.join("system"),
                shared: config.join("shared"),
                cache: config.join("cache"),
                data: config.join("data"),
                state: config.join("state"),
            });
        }
        let root = |name, fallback| {
            env(name)
                .filter(|p| p.is_absolute())
                .or_else(|| {
                    env("HOME")
                        .filter(|p| p.is_absolute())
                        .map(|p| p.join(fallback))
                })
                .ok_or_else(|| format!("Absolute HOME or {name} required"))
        };
        let config = root("XDG_CONFIG_HOME", ".config")?.join("remya");
        Ok(Self {
            system: config.join("system"),
            shared: config.join("shared"),
            cache: root("XDG_CACHE_HOME", ".cache")?.join("remya/core"),
            data: root("XDG_DATA_HOME", ".local/share")?.join("remya/core"),
            state: root("XDG_STATE_HOME", ".local/state")?.join("remya/core"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn isolated_roots_ignore_relative_xdg_values_and_separate_debug_state() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        let roots = Roots::from_environment(None, |name| {
            Some(if name == "HOME" {
                home.clone()
            } else {
                "relative".into()
            })
        })
        .unwrap();
        assert_eq!(roots.system, home.join(".config/remya/system"));
        assert_eq!(roots.shared, home.join(".config/remya/shared"));
        assert_eq!(roots.cache, home.join(".cache/remya/core"));
        assert_eq!(roots.data, home.join(".local/share/remya/core"));
        assert_eq!(roots.state, home.join(".local/state/remya/core"));
        let debug = Roots::detect(Some(temp.path().into())).unwrap();
        assert_eq!(debug.shared, temp.path().join("config/shared"));
        assert_eq!(debug.system, temp.path().join("config/system"));
        assert_eq!(debug.state, temp.path().join("config/state"));
        assert!(!temp.path().join("config").exists());
    }
}
