use serde_json::{Value, json};
pub const STYLE: &str = "themes.material-style-variant";
pub const MODE: &str = "themes.material-theme-mode";
pub const CONTRAST: &str = "themes.material-contrast";

const STYLES: &[&str] = &[
    "tonal_spot",
    "fidelity",
    "monochrome",
    "neutral",
    "vibrant",
    "expressive",
    "content",
    "rainbow",
    "fruit_salad",
];

const FIELDS: [(&str, &str, &str, &[&str]); 3] = [
    (STYLE, "style_variant", "tonal_spot", STYLES),
    (MODE, "theme_mode", "light", &["light", "dark"]),
    (
        CONTRAST,
        "contrast",
        "standard",
        &["standard", "medium", "high"],
    ),
];

pub fn field(id: &str) -> Option<&'static str> {
    FIELDS
        .iter()
        .find_map(|&(setting, key, _, _)| (setting == id).then_some(key))
}

pub fn defaults(mode: &str) -> Value {
    let mut value = normalize(json!({"version": 1})).expect("valid theme defaults");
    value["theme"]["theme_mode"] = json!(mode);
    value
}

pub fn normalize(value: Value) -> Result<Value, String> {
    let object = value
        .as_object()
        .ok_or("Invalid appearance configuration")?;
    if value["version"] != 1
        || object
            .keys()
            .any(|key| !["version", "theme"].contains(&key.as_str()))
    {
        return Err("Invalid appearance configuration".into());
    }
    let empty = serde_json::Map::new();
    let theme = match object.get("theme") {
        Some(table) => table.as_object().ok_or("Invalid theme table")?,
        None => &empty,
    };
    if theme
        .keys()
        .any(|key| !FIELDS.iter().any(|&(_, field, _, _)| key == field))
    {
        return Err("Invalid theme table".into());
    }

    let mut normalized = json!({"version": 1, "theme": {}});
    for (_, field, default, choices) in FIELDS {
        let selected = match theme.get(field) {
            Some(value) => value
                .as_str()
                .filter(|value| choices.contains(value))
                .ok_or_else(|| format!("Invalid theme {field}"))?,
            None => default,
        };
        normalized["theme"][field] = json!(selected);
    }
    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_schema_matches_existing_client_defaults_and_validation() {
        assert_eq!(
            normalize(json!({"version":1})).unwrap()["theme"]["style_variant"],
            "tonal_spot"
        );
        assert!(normalize(json!({"version":1,"theme":{"theme_mode":"system"}})).is_err());
        let older = json!({"version":1,"theme":{"style_variant":"expressive","theme_mode":"dark"}});
        let mut current = normalize(older).unwrap();
        assert_eq!(current["theme"]["contrast"], "standard");
        current["theme"][field(CONTRAST).unwrap()] = json!("high");
        assert_eq!(normalize(current.clone()).unwrap(), current);
        for value in [
            json!({"version":2}),
            json!({"version":1,"theme":{"contrast":"maximum"}}),
            json!({"version":1,"theme":{"contrast":0.5}}),
            json!({"version":1,"unknown":true}),
            json!({"version":1,"theme":{"unknown":true}}),
            json!({"version":1,"theme":{"theme_mode":true}}),
        ] {
            assert!(normalize(value).is_err());
        }
    }
}
