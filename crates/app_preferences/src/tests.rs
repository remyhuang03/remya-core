use super::*;
use tempfile::{TempDir, tempdir};

fn registration() -> Value {
    json!({"version":1,"id":"remya-bar","app":"top.remya.bar","title":"Remya Bar","icon":"top.remya.bar","config":"bar",
    "pages":[{"id":"layout","title":"Layout","keywords":["pill"]}],
    "settings":{
        "layout":{"title":"Bar layout","page":"layout","keywords":["modules"],"default":{"size":2},
            "schema":{"type":"object","properties":{"size":{"type":"integer","minimum":1}},"required":["size"],"additionalProperties":false}},
        "enabled":{"title":"Enabled","page":"layout","keywords":[],"default":true,"schema":{"type":"boolean"}},
        "anything":{"title":"Anything","page":"layout","keywords":[],"default":[],"schema":true}
    }})
}
fn register(root: &Path, id: &str, value: &Value) {
    let dir = root.join("remya/apps");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join(format!("{id}.json")), value.to_string()).unwrap();
}
fn fixture() -> (TempDir, Store) {
    let temp = tempdir().unwrap();
    let data = temp.path().join("data");
    register(&data, "remya-bar", &registration());
    let store = Store::new(
        temp.path().join("config"),
        vec![data],
        BTreeSet::from(["sound".into()]),
        remya_display::atomic_write,
    );
    (temp, store)
}

#[test]
fn registration_is_strict_and_meta_validates_defaults() {
    assert!(App::load(&registration().to_string(), "remya-bar", &BTreeSet::new()).is_ok());
    let mut cases = Vec::new();
    let mut bad = registration();
    bad["extra"] = json!(1);
    cases.push(bad);
    let mut bad = registration();
    bad["pages"][0]["extra"] = json!(1);
    cases.push(bad);
    let mut bad = registration();
    bad["settings"]["layout"]["extra"] = json!(1);
    cases.push(bad);
    let mut bad = registration();
    bad["id"] = json!("../bar");
    cases.push(bad);
    let mut bad = registration();
    bad["id"] = json!("a".repeat(65));
    cases.push(bad);
    let mut bad = registration();
    bad["settings"]["layout"]["default"] = json!({"size":0});
    cases.push(bad);
    let mut bad = registration();
    bad["settings"]["layout"]["schema"]["type"] = json!(17);
    cases.push(bad);
    let mut bad = registration();
    bad["settings"]["layout"]["schema"] = json!({"$ref":"file:///etc/passwd"});
    cases.push(bad);
    let mut bad = registration();
    bad["settings"]["layout"]["page"] = json!("unknown");
    cases.push(bad);
    let mut bad = registration();
    bad["config"] = json!("shared");
    cases.push(bad);
    let mut bad = registration();
    bad["app"] = json!("../bar");
    cases.push(bad);
    let mut bad = registration();
    bad["settings"]["anything"]["default"] = Value::Null;
    cases.push(bad);
    for bad in cases {
        assert!(
            App::load(&bad.to_string(), "remya-bar", &BTreeSet::new()).is_err(),
            "{bad}"
        );
    }
    assert!(
        App::load(
            &registration().to_string(),
            "remya-bar",
            &BTreeSet::from(["remya-bar".into()])
        )
        .is_err()
    );
}

#[test]
fn precedence_collisions_and_invalid_registrations_remain_visible() {
    let (temp, mut store) = fixture();
    let second = temp.path().join("second");
    let mut shadowed = registration();
    shadowed["extra"] = json!(true);
    register(&second, "remya-bar", &shadowed);
    store.data.push(second);
    let mut duplicate = registration();
    duplicate["id"] = json!("remya-launcher");
    register(&store.data[0], "remya-launcher", &duplicate);
    let mut collision = registration();
    collision["id"] = json!("sound");
    register(&store.data[0], "sound", &collision);
    let mut bad = registration();
    bad["extra"] = json!(true);
    register(&store.data[0], "invalid", &bad);
    store.rescan();
    assert_eq!(store.list().as_array().unwrap().len(), 4);
    assert!(
        store
            .list()
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["id"] != "remya-bar")
            .all(|e| e["error"].is_string())
    );
    assert_eq!(store.get("remya-bar.enabled").unwrap()["value"], true);
    assert!(
        store
            .get("remya-launcher.enabled")
            .unwrap_err()
            .starts_with("Unavailable:")
    );
    assert_eq!(store.search("modules")[0]["app"], "remya-bar");
    assert_eq!(store.search("pill")[0]["page"], "layout");
    assert!(!store.search("layout").is_empty());
}

#[test]
fn get_defaults_set_validation_and_per_key_stored_errors() {
    let (temp, mut store) = fixture();
    let path = temp.path().join("config/bar/settings.toml");
    let initial = store.get("remya-bar.layout").unwrap();
    assert_eq!(initial["value"], json!({"size":2}));
    assert!(!path.exists());
    assert_eq!(
        store.get("remya-bar.layout").unwrap()["revision"],
        initial["revision"]
    );
    let stored = store
        .set("remya-bar.layout", r#"{"size":3}"#, None)
        .unwrap();
    assert_ne!(stored["revision"], initial["revision"]);
    assert_eq!(store.get("remya-bar.layout").unwrap(), stored);
    for (id, value) in [
        ("layout", r#"{"size":0}"#),
        ("anything", "null"),
        ("anything", r#"{"a":[null]}"#),
        ("anything", "18446744073709551615"),
    ] {
        assert!(
            store
                .set(&format!("remya-bar.{id}"), value, None)
                .unwrap_err()
                .starts_with("InvalidArgument:")
        );
        assert_eq!(store.get("remya-bar.layout").unwrap(), stored);
    }
    fs::write(&path, "layout = {size = 0}\nenabled = false\n").unwrap();
    assert!(store.get("remya-bar.layout").is_err());
    assert_eq!(store.get("remya-bar.enabled").unwrap()["value"], false);
    store
        .set("remya-bar.layout", r#"{"size":4}"#, None)
        .unwrap();
    assert_eq!(store.get("remya-bar.enabled").unwrap()["value"], false);
}

#[test]
fn replace_conflict_and_malformed_file_preserve_user_data() {
    let (temp, mut store) = fixture();
    let revision = store.get("remya-bar.enabled").unwrap()["revision"]
        .as_str()
        .unwrap()
        .to_owned();
    store
        .set("remya-bar.enabled", "false", Some(&revision))
        .unwrap();
    assert_eq!(
        store
            .set("remya-bar.enabled", "true", Some(&revision))
            .unwrap_err(),
        "Changed in another view. Reopen the editor."
    );
    let path = temp.path().join("config/bar/settings.toml");
    fs::write(&path, "broken = [").unwrap();
    assert!(store.set("remya-bar.enabled", "true", None).is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), "broken = [");
}

#[test]
fn unknown_apps_rescan_and_uninstalled_apps_leave_files_alone() {
    let (temp, mut store) = fixture();
    let mut new = registration();
    new["id"] = json!("remya-new");
    new["config"] = json!("new");
    register(&store.data[0], "remya-new", &new);
    assert_eq!(store.get("remya-new.enabled").unwrap()["value"], true);
    store.set("remya-bar.enabled", "false", None).unwrap();
    fs::remove_file(store.data[0].join("remya/apps/remya-bar.json")).unwrap();
    store.rescan();
    assert!(
        store
            .get("remya-bar.enabled")
            .unwrap_err()
            .starts_with("Unavailable:")
    );
    assert!(
        store
            .set("remya-bar.enabled", "true", None)
            .unwrap_err()
            .starts_with("Unavailable:")
    );
    assert_eq!(
        fs::read_to_string(temp.path().join("config/bar/settings.toml")).unwrap(),
        "enabled = false\n"
    );
}

#[test]
fn xdg_roots_ignore_relative_entries_and_keep_precedence() {
    let roots = data_roots_from(|key| {
        Some(
            match key {
                "HOME" => "/tmp/home",
                "XDG_DATA_HOME" => "relative",
                "XDG_DATA_DIRS" => "relative:/tmp/one::/tmp/two",
                _ => unreachable!(),
            }
            .into(),
        )
    });
    assert_eq!(
        roots,
        vec![
            PathBuf::from("/tmp/home/.local/share"),
            "/tmp/one".into(),
            "/tmp/two".into()
        ]
    );
}

#[test]
fn open_app_uses_desktop_action_and_validated_argv_without_spawning() {
    let (_temp, mut store) = fixture();
    let pages = vec!["layout".into()];
    let args = settings_argv(
        r#""/path with spaces/bar" %f %F %u %U %i %c %k --literal=100%%"#,
        "layout",
        &pages,
    )
    .unwrap();
    assert_eq!(
        args,
        [
            "/path with spaces/bar",
            "--literal=100%",
            "--settings",
            "layout"
        ]
        .map(OsString::from)
    );
    assert_eq!(
        settings_argv("bar --settings %U", "layout", &pages).unwrap(),
        ["bar", "--settings", "layout"].map(OsString::from)
    );
    assert_eq!(
        settings_argv("bar", "", &pages).unwrap(),
        ["bar", "--settings"].map(OsString::from)
    );
    assert!(
        settings_argv("bar", "unknown", &pages)
            .unwrap_err()
            .starts_with("InvalidArgument:")
    );
    assert!(settings_argv("%U", "", &pages).is_err());
    assert!(settings_argv("%U another-program", "", &pages).is_err());
    assert_eq!(
        settings_argv(r#"bar """#, "", &pages).unwrap(),
        ["bar", "", "--settings"].map(OsString::from)
    );
    assert!(
        store
            .open_argv("remya-bar", "layout")
            .unwrap_err()
            .starts_with("Unavailable:")
    );
    let applications = store.data[0].join("applications");
    fs::create_dir_all(&applications).unwrap();
    let path = applications.join("top.remya.bar.desktop");
    fs::write(&path, "[Desktop Entry]\nExec=wrong\n").unwrap();
    assert!(
        store
            .open_argv("remya-bar", "layout")
            .unwrap_err()
            .contains("Desktop Action settings")
    );
    fs::write(
        &path,
        "[Desktop Entry]\nExec=wrong\n[Desktop Action settings]\nExec=bar --settings %U\n",
    )
    .unwrap();
    assert_eq!(
        store.open_argv("remya-bar", "layout").unwrap(),
        ["bar", "--settings", "layout"].map(OsString::from)
    );
    assert!(
        store
            .open_argv("remya-bar", "unknown")
            .unwrap_err()
            .starts_with("InvalidArgument:")
    );
}
