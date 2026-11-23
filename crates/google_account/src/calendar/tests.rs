use super::*;

fn request() -> Value {
    json!({
        "accountId": "123", "timeMin": "2026-11-01T00:00:00+08:00",
        "timeMax": "2027-01-15T00:00:00+08:00", "calendarIds": ["shared@example.com"]
    })
}

#[test]
fn request_rejects_unbounded_windows_wrong_types_and_unknown_fields() {
    assert!(Call::parse(&request().to_string()).is_ok());
    for (key, value) in [
        ("timeMin", json!("2026-11-01T00:00:00")),
        ("timeMax", json!("2027-08-02T00:00:00Z")),
        ("timeMax", json!("2026-10-01T00:00:00Z")),
        ("calendarIds", json!(["bad\ncalendar"])),
        ("url", json!("https://example.com")),
    ] {
        let mut invalid = request();
        invalid[key] = value;
        assert_eq!(
            Call::parse(&invalid.to_string()).err(),
            Some(Failure::Invalid),
            "{key}"
        );
    }
}

#[test]
fn incremental_changes_replace_move_and_remove_cached_instances() {
    let (_, window) = Call::parse(&request().to_string()).unwrap();
    let initial = |id| {
        json!({
            "id": id, "start": {"date": "2026-11-12"}, "end": {"date": "2026-11-14"}
        })
    };
    let mut events = BTreeMap::new();
    merge_events(
        &mut events,
        vec![initial("updated"), initial("moved"), initial("removed")],
        window,
    );
    merge_events(
        &mut events,
        vec![
            json!({"id": "removed", "status": "cancelled"}),
            json!({"id": "moved", "start":{"date":"2027-06-01"},"end":{"date":"2027-06-02"}}),
            json!({"id": "updated", "start":{"dateTime":"2026-11-08T12:00:00Z"},
            "end":{"dateTime":"2026-11-08T13:00:00Z"}, "attendees":[
                {"self":false,"responseStatus":"accepted"},{"self":true,"responseStatus":"declined"}
            ]}),
        ],
        window,
    );
    assert_eq!(events.len(), 1);
    assert_eq!(
        events["updated"]["attendees"],
        json!([{"self":true,"responseStatus":"declined"}])
    );
}

#[test]
fn provider_times_keep_all_day_and_named_zone_dates_for_local_display() {
    let (_, window) = Call::parse(&request().to_string()).unwrap();
    assert!(window.overlaps(&json!({
        "start":{"dateTime":"2026-11-23T10:00:00","timeZone":"Asia/Taipei"},
        "end":{"dateTime":"2026-11-23T11:00:00","timeZone":"Asia/Taipei"}
    })));
    assert!(!window.overlaps(&json!({
        "start":{"dateTime":"2026-11-23T10:00:00"},
        "end":{"dateTime":"2026-11-23T11:00:00"}
    })));
    assert!(window.overlaps(&json!({
        "start":{"date":"2026-11-12"},"end":{"date":"2026-11-13"}
    })));
}

#[test]
fn persisted_snapshot_keeps_private_file_and_provider_selection() {
    let (call, window) = Call::parse(&request().to_string()).unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("calendar").join("account.json");
    let event =
        json!({"id":"a","colorId":"1","start":{"date":"2026-11-11"},"end":{"date":"2026-11-12"}});
    let snapshot = Snapshot {
        listed_at: Some(SystemTime::now().into()),
        calendars: vec![Calendar {
            id: "shared@example.com".into(),
            summary: "Work".into(),
            selected: false,
            background_color: Some("#3399ff".into()),
        }],
        colors: Colors {
            fetched_at: None,
            event: BTreeMap::from([(
                "1".into(),
                Color {
                    background: "#ff9933".into(),
                },
            )]),
        },
        slices: BTreeMap::from([(
            "shared@example.com".into(),
            EventWindow {
                window,
                synced_at: SystemTime::now().into(),
                sync_token: Some("opaque".into()),
                events: BTreeMap::from([("a".into(), event)]),
                payload_size: 256,
            },
        )]),
    };
    snapshot.save(&file).unwrap();
    assert_eq!(
        fs::metadata(&file).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let loaded = Snapshot::load(&file).unwrap();
    assert_eq!(
        loaded.slices["shared@example.com"].sync_token.as_deref(),
        Some("opaque")
    );
    let reply = loaded.reply(&call, window, Some(Failure::Offline));
    assert_eq!(reply["error"], "offline");
    assert_eq!(reply["calendars"][0]["selected"], false);
    assert_eq!(reply["events"][0]["backgroundColor"], "#ff9933");
    assert!(reply.get("syncToken").is_none());
}
