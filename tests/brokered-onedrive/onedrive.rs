//! The OneDrive server's own pieces: its tools against its manifest, where
//! a folder and an upload go, and what a read brings back.

use super::*;

#[test]
fn every_tool_is_classed_in_the_manifest() {
    let m: toml::Table = include_str!("../../catalog/microsoft-onedrive.toml").parse().unwrap();
    let mut classed: Vec<&str> = m["tools"].as_table().unwrap().keys().map(String::as_str).collect();
    let mut listed: Vec<&str> = tools().iter().map(|t| t.name).collect();
    classed.sort();
    listed.sort();
    assert_eq!(classed, listed);
    assert_eq!((m["tools"]["upload_file"].as_str(), m["reach"]["upload_file"].as_str()), (Some("reversible_write"), Some("self")));
    for t in ["list_files", "search_files", "read_file"] {
        assert_eq!(m["tools"][t].as_str(), Some("read"), "{t}");
    }
    for t in tools() {
        assert_eq!(t.input["type"], "object", "{}", t.name);
        assert_eq!(t.output["type"], "object", "{}", t.name);
    }
}

/// A folder is the root, a path under it, or an id; a path never climbs
/// out, and each of its parts is encoded.
#[test]
fn a_folder_is_under_the_root() {
    assert_eq!(folder(&json!({})).unwrap(), "/v1.0/me/drive/root");
    assert_eq!(folder(&json!({ "folder": "/Reports/2026 Q4/" })).unwrap(), "/v1.0/me/drive/root:/Reports/2026%20Q4:");
    assert_eq!(folder(&json!({ "folder_id": "01AB/C" })).unwrap(), "/v1.0/me/drive/items/01AB%2FC");
    assert!(folder(&json!({ "folder": "Reports/../../x" })).is_err());
}

/// An upload makes a new file and never replaces one; its name is one
/// name, and its bytes are within the limit.
#[test]
fn an_upload_makes_a_new_file_only() {
    assert_eq!(upload_path(&json!({ "name": "notes.txt" })).unwrap(), "/v1.0/me/drive/root:/notes.txt:/content?@microsoft.graph.conflictBehavior=fail");
    assert_eq!(upload_path(&json!({ "name": "a b.txt", "folder": "Reports" })).unwrap(), "/v1.0/me/drive/root:/Reports/a%20b.txt:/content?@microsoft.graph.conflictBehavior=fail");
    for bad in [json!({}), json!({ "name": "x/y.txt" }), json!({ "name": ".." }), json!({ "name": "a", "folder": "../b" })] {
        assert!(upload_path(&bad).is_err(), "{bad}");
    }
    assert_eq!(upload_bytes(&json!({ "text": "hi" })).unwrap(), b"hi");
    assert_eq!(upload_bytes(&json!({ "content_base64": STANDARD.encode([0u8, 255]) })).unwrap(), [0u8, 255]);
    assert!(upload_bytes(&json!({})).is_err());
    assert!(upload_bytes(&json!({ "text": "a", "content_base64": "YQ==" })).is_err());
    assert!(upload_bytes(&json!({ "text": "a".repeat(UPLOAD_MAX_BYTES + 1) })).unwrap_err().contains("more than"));
}

#[test]
fn a_read_is_text_when_it_is_text() {
    assert_eq!(content(b"hello"), json!({ "text": "hello" }));
    assert_eq!(content(&[0xff, 0x00]), json!({ "content_base64": "/wA=" }));
    let i = item(&json!({ "id": "1", "name": "a.txt", "size": 5, "file": { "mimeType": "text/plain" }, "parentReference": { "path": "/drive/root:" } }));
    assert_eq!((i["item_id"].as_str(), i["folder"].as_bool(), i["type"].as_str()), (Some("1"), Some(false), Some("text/plain")));
    assert_eq!(item(&json!({ "id": "2", "name": "R", "folder": { "childCount": 1 } }))["folder"], true);
}
