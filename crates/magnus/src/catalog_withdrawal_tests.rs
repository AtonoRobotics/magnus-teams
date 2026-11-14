//! The signed withdrawal list: read only when it verifies with the release
//! key, applied once, and what it does to a withdrawn connector.

use super::*;
use base64::Engine;
use std::sync::atomic::{AtomicUsize, Ordering};

const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD;

struct Key(minisign::KeyPair);

impl Key {
    fn new() -> Key {
        Key(minisign::KeyPair::generate_unencrypted_keypair().unwrap())
    }
    /// The public key as `[updates] public_key` holds it.
    fn public(&self) -> String {
        B64.encode(self.0.pk.to_box().unwrap().to_string())
    }
    /// A signature bound to `version`, as `tauri signer sign --app-version` makes it.
    fn sign(&self, data: &[u8], version: &str) -> String {
        let trusted = format!("timestamp:{}\tfile:{FILE}\tversion:{version}", chrono::Utc::now().timestamp());
        let sig = minisign::sign(Some(&self.0.pk), &self.0.sk, std::io::Cursor::new(data), Some(&trusted), Some("signature from tauri secret key")).unwrap();
        B64.encode(sig.to_string())
    }
    /// Publish `list` into `dir`, signed as `release/publish-withdrawals.sh` signs it.
    fn publish(&self, dir: &Path, list: &List) {
        std::fs::create_dir_all(dir).unwrap();
        let bytes = serde_json::to_vec_pretty(list).unwrap();
        std::fs::write(dir.join(format!("{FILE}.sig")), self.sign(&bytes, &bound_version(list.serial))).unwrap();
        std::fs::write(dir.join(FILE), bytes).unwrap();
    }
}

fn list(serial: u64, ids: &[&str]) -> List {
    List { serial, withdrawn: ids.iter().map(|id| Withdrawal { id: id.to_string(), versions: None, reason: "Its maker stopped supporting it.".into() }).collect() }
}

#[test]
fn a_list_signed_with_the_release_key_is_read() {
    let (key, dir) = (Key::new(), tempfile::tempdir().unwrap());
    key.publish(dir.path(), &list(4, &["slack"]));
    assert_eq!(read(dir.path(), &key.public()).unwrap(), Some(list(4, &["slack"])));
}

#[test]
fn no_list_published_is_nothing_to_apply() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(read(dir.path(), &Key::new().public()).unwrap(), None);
}

#[test]
fn a_list_signed_by_another_key_is_refused() {
    let (key, other, dir) = (Key::new(), Key::new(), tempfile::tempdir().unwrap());
    other.publish(dir.path(), &list(1, &["slack"]));
    assert!(format!("{:#}", read(dir.path(), &key.public()).unwrap_err()).contains("does not verify"));
}

#[test]
fn a_list_changed_after_it_was_signed_is_refused() {
    let (key, dir) = (Key::new(), tempfile::tempdir().unwrap());
    key.publish(dir.path(), &list(2, &["slack"]));
    // Someone with the directory but not the key takes Slack off the list.
    std::fs::write(dir.path().join(FILE), serde_json::to_vec_pretty(&list(2, &[])).unwrap()).unwrap();
    assert!(read(dir.path(), &key.public()).is_err());
}

#[test]
fn an_unsigned_list_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(FILE), serde_json::to_vec(&list(1, &["slack"])).unwrap()).unwrap();
    assert!(format!("{:#}", read(dir.path(), &Key::new().public()).unwrap_err()).contains("not signed"));
}

/// A signature binds its serial: an old list's signature does not carry a
/// new serial, and a release package's signature is no list's.
#[test]
fn a_signature_for_another_serial_or_a_release_is_refused() {
    let (key, dir) = (Key::new(), tempfile::tempdir().unwrap());
    let bytes = serde_json::to_vec(&list(9, &[])).unwrap();
    for version in [bound_version(8), "9".to_string(), "0.9.0".to_string()] {
        std::fs::write(dir.path().join(format!("{FILE}.sig")), key.sign(&bytes, &version)).unwrap();
        std::fs::write(dir.path().join(FILE), &bytes).unwrap();
        assert!(read(dir.path(), &key.public()).is_err(), "{version}");
    }
}

#[test]
fn a_newer_list_withdraws_and_restores_and_the_same_one_again_changes_nothing() {
    let mut l = Ledger::default();
    for c in changes(&l, &list(1, &["slack", "github"])).unwrap() {
        l.apply(&c);
    }
    assert_eq!((l.serial, l.withdrawn.keys().cloned().collect::<Vec<_>>()), (1, vec!["github".to_string(), "slack".to_string()]));
    assert!(changes(&l, &list(1, &["slack", "github"])).unwrap().is_empty());
    let cs = changes(&l, &list(2, &["slack"])).unwrap();
    assert!(matches!(&cs[..], [Change::Restored { id, .. }, Change::Applied { serial: 2 }] if id == "github"), "{cs:?}");
    for c in cs {
        l.apply(&c);
    }
    assert!(l.withdrawn.contains_key("slack") && !l.withdrawn.contains_key("github"));
    // A list older than the one applied is refused: a replay cannot restore Slack.
    assert!(changes(&l, &list(1, &[])).is_err());
}

#[test]
fn a_range_withdraws_only_the_versions_in_it() {
    let w = |v: Option<&str>| Withdrawal { id: "slack".into(), versions: v.map(String::from), reason: String::new() };
    assert!(w(None).covers("1.0.0"));
    assert!(w(Some("<1.4.0")).covers("1.3.9") && !w(Some("<1.4.0")).covers("1.4.0"));
    // A range that does not parse withdraws every version: the safer reading.
    assert!(w(Some("not a range")).covers("2.0.0"));
}

/// A revocation endpoint that counts the tokens revoked at it.
async fn revoker() -> (String, Arc<AtomicUsize>) {
    let n = Arc::new(AtomicUsize::new(0));
    let c = n.clone();
    let app = axum::Router::new().route(
        "/revoke",
        axum::routing::post(move || {
            let c = c.clone();
            async move {
                c.fetch_add(1, Ordering::SeqCst);
                axum::http::StatusCode::OK
            }
        }),
    );
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/revoke", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, app).await });
    (url, n)
}

fn sign_in(k: &Kernel, app: &str, person: &str, revoke_at: &str) {
    let t = json!({ "access_token": "at", "refresh_token": "rt", "client": { "token_endpoint": "http://127.0.0.1:9/token", "revocation_endpoint": revoke_at, "client_id": "magnus", "resource": "r" } });
    k.vault.put(&crate::connectors::connection_name(app, person), &t.to_string()).unwrap();
}

fn notices(k: &Kernel, person: &str, app: &str) -> usize {
    k.read(|s| s.items.values().filter(|i| i.kind == "notice" && i.person == person && i.details["connector"] == app && i.details["withdrawn"] == true).count())
}

/// Withdrawing GitHub takes it out of every Library and run, revokes Mo's
/// connection at GitHub, deletes it and tells Mo once; Ana, connected to
/// Slack only, is untouched. Read again, nothing is done twice; a list
/// changed or older is refused and the withdrawal stands; a later list
/// without GitHub brings it back.
#[tokio::test(flavor = "multi_thread")]
async fn a_withdrawn_connector_is_turned_off_its_connections_revoked_and_its_people_told_once() {
    let key = Key::new();
    let more = format!("[updates]\npublic_key = {:?}\ncheck_minutes = 0\n", key.public());
    let s = crate::admin_app::tests::server_with(&more, |_| axum::Router::new()).await;
    let k = s.k.clone();
    let (revoke_at, revoked) = revoker().await;
    sign_in(&k, "github", "mo", &revoke_at);
    sign_in(&k, "slack", "ana", &revoke_at);
    assert!(k.connector("github").is_some());
    let dir = list_dir(&k.cfg);

    key.publish(&dir, &list(2, &["github"]));
    let v = k.check_withdrawals().await.unwrap();
    assert_eq!((v["serial"].as_u64(), v["refused"].is_null(), v["connections_ended"].as_u64()), (Some(2), true, Some(1)), "{v}");
    assert!(k.connector("github").is_none() && k.all_connectors().iter().all(|(c, _)| c.name != "github"));
    assert!(k.vault.get(&crate::connectors::connection_name("github", "mo")).unwrap().is_none());
    assert_eq!(revoked.load(Ordering::SeqCst), 2, "the refresh and the access token, revoked at the provider");
    assert_eq!(notices(&k, "mo", "github"), 1);
    assert!(k.connector("slack").is_some() && k.vault.get(&crate::connectors::connection_name("slack", "ana")).unwrap().is_some());
    assert_eq!(notices(&k, "ana", "github"), 0);

    // Read again: nothing more is done.
    let v = k.check_withdrawals().await.unwrap();
    assert_eq!(v["connections_ended"].as_u64(), Some(0), "{v}");
    assert_eq!((revoked.load(Ordering::SeqCst), notices(&k, "mo", "github")), (2, 1));

    // A sign-in left behind (a crash after Mo was told) is ended, and Mo is not told twice.
    sign_in(&k, "github", "mo", &revoke_at);
    k.check_withdrawals().await.unwrap();
    assert!(k.vault.get(&crate::connectors::connection_name("github", "mo")).unwrap().is_none());
    assert_eq!((revoked.load(Ordering::SeqCst), notices(&k, "mo", "github")), (4, 1));

    // Changed after signing, or older than the one applied: refused, and the last good list stands.
    std::fs::write(dir.join(FILE), serde_json::to_vec_pretty(&list(3, &[])).unwrap()).unwrap();
    assert!(!k.check_withdrawals().await.unwrap()["refused"].is_null());
    key.publish(&dir, &list(1, &[]));
    assert!(!k.check_withdrawals().await.unwrap()["refused"].is_null());
    std::fs::remove_file(dir.join(FILE)).unwrap();
    assert!(k.check_withdrawals().await.unwrap()["refused"].is_null());
    assert!(k.connector("github").is_none());

    // A later list without GitHub brings it back.
    key.publish(&dir, &list(3, &[]));
    k.check_withdrawals().await.unwrap();
    assert!(k.connector("github").is_some());
    assert_eq!(k.read(|s| s.catalog.serial), 3);
}
