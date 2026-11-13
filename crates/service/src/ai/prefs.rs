//! `ai.toml`: shared custom OpenAI-compatible endpoints.
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, fs, path::Path};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Prefs {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub custom: Vec<Custom>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Custom {
    pub id: String,
    pub name: String,
    pub base_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub models_dev: Option<String>,
}

/// Accepts https endpoints, or http only on a loopback host. Credentials in
/// the URL are refused because the file is not secret storage.
pub fn validate_base_url(url: &str) -> Result<(), String> {
    let error = || "Base URL must be https, or http on a loopback host".to_string();
    let uri: ureq::http::Uri = url.parse().map_err(|_| error())?;
    let authority = uri.authority().ok_or_else(error)?;
    if authority.as_str().contains('@') || url.chars().any(char::is_control) {
        return Err(error());
    }
    let host = authority
        .host()
        .trim_start_matches('[')
        .trim_end_matches(']');
    let loopback = host == "localhost"
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback());
    match uri.scheme_str() {
        Some("https") if !host.is_empty() => Ok(()),
        Some("http") if loopback => Ok(()),
        _ => Err(error()),
    }
}

pub fn valid_models_dev_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "-_.".contains(c))
}

pub fn validate_name(name: &str) -> Result<&str, String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 64 || name.chars().any(char::is_control) {
        return Err("Name must be 1 to 64 characters without control characters".into());
    }
    Ok(name)
}

/// `custom-<slug>` from the display name; `None` when nothing usable remains.
pub fn custom_id(name: &str) -> Option<String> {
    let mut slug = String::new();
    for c in name.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            slug.push(c);
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = slug.trim_end_matches('-');
    (!slug.is_empty()).then(|| format!("custom-{slug}"))
}

impl Prefs {
    pub fn validate(&self) -> Result<(), String> {
        let mut ids = HashSet::new();
        for custom in &self.custom {
            let slug = custom.id.strip_prefix("custom-").unwrap_or_default();
            if custom_id(slug).as_deref() != Some(&custom.id[..]) || !ids.insert(&custom.id) {
                return Err("Invalid or duplicate custom provider id".into());
            }
            validate_name(&custom.name)?;
            validate_base_url(&custom.base_url)?;
            if custom
                .models_dev
                .as_deref()
                .is_some_and(|id| !valid_models_dev_id(id))
            {
                return Err("Invalid models.dev provider id".into());
            }
        }
        Ok(())
    }

    /// Missing file means defaults; an invalid file is an error so it is never overwritten.
    pub fn load(path: &Path) -> Result<Self, String> {
        let prefs: Self = match fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(e.to_string()),
        };
        prefs
            .validate()
            .map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(prefs)
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        self.validate()?;
        let text = toml::to_string(self).map_err(|e| e.to_string())?;
        remya_display::atomic_write(path, text.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_url_requires_https_or_loopback_http() {
        for ok in [
            "https://api.example.com/v1",
            "http://localhost:11434/v1",
            "http://127.0.0.1:8080",
            "http://[::1]:8080/v1",
        ] {
            assert!(validate_base_url(ok).is_ok(), "{ok}");
        }
        for bad in [
            "http://api.example.com/v1",
            "ftp://example.com",
            "https://user:pw@example.com",
            "example.com",
            "",
        ] {
            assert!(validate_base_url(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn custom_ids_are_slugs() {
        assert_eq!(
            custom_id("My Endpoint").as_deref(),
            Some("custom-my-endpoint")
        );
        assert_eq!(
            custom_id("  Groq (fast)!! ").as_deref(),
            Some("custom-groq-fast")
        );
        assert_eq!(custom_id("日本"), None);
    }

    #[test]
    fn toml_round_trip_and_rejection() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ai.toml");
        assert_eq!(Prefs::load(&path).unwrap(), Prefs::default());
        let prefs = Prefs {
            custom: vec![Custom {
                id: "custom-my-endpoint".into(),
                name: "My Endpoint".into(),
                base_url: "https://api.example.com/v1".into(),
                models_dev: Some("openrouter".into()),
            }],
        };
        prefs.save(&path).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("[[custom]]"), "{text}");
        assert_eq!(Prefs::load(&path).unwrap(), prefs);
        fs::write(&path, "unknown = 1\n").unwrap();
        assert!(Prefs::load(&path).is_err());
    }
}
