//! Hugging Face's connector, against Hugging Face: no fakes (owner,
//! 2026-11-01). The catalog's Hugging Face is in every Library already set
//! up, and Magnus registers itself at Hugging Face's sign-in when a person
//! connects: nothing for anyone to set up.

use magnus_scratch::catalog;

#[tokio::test(flavor = "multi_thread")]
async fn hugging_face_is_in_every_library_and_magnus_registers_itself_at_its_sign_in() {
    let u = catalog::registers_itself_and_signs_in_at("huggingface", "huggingface.co").await;
    assert_eq!(u.path(), "/oauth/authorize", "{u}");
}

/// Tool drift (connectors spec, Required tests): every tool the live server
/// lists to anyone is classed in the manifest, so none is hidden for want of
/// a class. The tools a person turns on in their Hugging Face settings are
/// listed only to them, after a sign-in.
#[tokio::test(flavor = "multi_thread")]
async fn every_tool_hugging_face_lists_without_a_sign_in_is_classed() {
    let m = catalog::manifest("huggingface");
    let classed = m["tools"].as_table().unwrap();
    let live = catalog::tools_listed_without_sign_in(m["endpoint"].as_str().unwrap()).await;
    assert!(!live.is_empty(), "Hugging Face listed no tools");
    let unclassed: Vec<&String> = live.iter().filter(|t| !classed.contains_key(t.as_str())).collect();
    assert!(unclassed.is_empty(), "Hugging Face lists tools its manifest does not class: {unclassed:?}");
}
