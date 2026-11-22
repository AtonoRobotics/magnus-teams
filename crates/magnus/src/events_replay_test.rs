/// A call logged before `personal` was recorded, carrying the person's
/// notes, still counts as carrying them when replayed (#304).
#[test]
fn a_call_logged_before_personal_with_notes_carried_its_person() {
    let old: super::ContextRef = serde_json::from_value(serde_json::json!({ "through_seq": 7, "instructions": "v1", "tools_hash": "h", "relationship": 3 })).unwrap();
    assert!(!old.personal);
    assert!(old.carried_person());
    let none: super::ContextRef = serde_json::from_value(serde_json::json!({ "through_seq": 7, "instructions": "v1", "tools_hash": "h" })).unwrap();
    assert!(!none.carried_person());
}

/// All Wave 1–3 event additions are new top-level `Ev` kinds or `#[serde(default)]`
/// optional fields so older binaries skip unknown top-level kinds cleanly on rollback (B3, P8).
#[test]
fn new_events_are_top_level_and_legacy_events_deserialize_with_defaults() {
    use chrono::Utc;
    let new_events = vec![
        super::Ev::QueryGateRecorded(super::QueryGatePayload { scope: "t1".into(), bytes_added: 128, denied: false }),
        super::Ev::SafetyBlocked(super::SafetyBlockedPayload { run: "r1".into(), tool: "mail__send".into(), hash: "h1".into(), reason: "blocked".into() }),
        super::Ev::MemorySuperseded(super::MemorySupersededPayload {
            old_id: "m1".into(),
            new_id: "m2".into(),
            person: "alice".into(),
            reason: "updated".into(),
            valid_until: Utc::now(),
        }),
        super::Ev::MemoryArchived(super::MemoryArchivedPayload { id: "m3".into(), person: "alice".into(), reason: "tier2_cold".into() }),
        super::Ev::PrefetchRecorded {
            run: "r1".into(),
            person: "alice".into(),
            pointers: "- channel C01234567\n".into(),
            label: crate::tools::Label { untrusted: false, private: true },
        },
    ];
    for ev in new_events {
        let v = serde_json::to_value(&ev).unwrap();
        assert!(v["kind"].is_string(), "every new event must be a top-level kind");
        let roundtrip: super::Ev = serde_json::from_value(v).unwrap();
        let _ = roundtrip;
    }

    // Legacy ScheduleSet without quarantine_schema still deserializes cleanly.
    let old_sched: super::Ev = serde_json::from_value(serde_json::json!({
        "kind": "schedule_set",
        "id": "s1",
        "person": "alice",
        "text": "daily report"
    }))
    .unwrap();
    assert!(matches!(old_sched, super::Ev::ScheduleSet { quarantine_schema: None, .. }));
}
