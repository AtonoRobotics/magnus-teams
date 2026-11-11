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
