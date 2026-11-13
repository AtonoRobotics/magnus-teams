use super::*;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;

fn known(version: &str, base: Option<&str>, ops: &[&str]) -> Seen {
    Seen::Known { version: version.into(), base_id: base.map(str::to_string), ops: ops.iter().map(|s| s.to_string()).collect() }
}

fn shipped(version: &str, base: Option<&str>) -> Shipped {
    Shipped { version: version.into(), base_id: base.map(str::to_string) }
}

#[test]
fn a_computerd_too_old_to_say_is_behind() {
    assert!(behind(&Seen::Old, &shipped("0.2.6-test.81", None)));
}

#[test]
fn an_older_release_is_behind_and_the_same_or_a_newer_one_is_not() {
    let s = shipped("0.2.6-test.81", Some("aa"));
    assert!(behind(&known("0.2.6-test.80", Some("aa"), &[]), &s));
    assert!(behind(&known("0.2.5", Some("aa"), &[]), &s));
    assert!(!behind(&known("0.2.6-test.81", Some("aa"), &[]), &s));
    assert!(!behind(&known("0.2.6-test.90", Some("bb"), &[]), &s), "a computerd newer than a rolled-back server is not behind");
    assert!(!behind(&known("0.2.6", None, &[]), &s));
}

#[test]
fn the_same_release_on_another_base_image_is_behind() {
    let s = shipped("0.2.6-test.81", Some("aa"));
    assert!(behind(&known("0.2.6-test.81", Some("bb"), &[]), &s));
    assert!(behind(&known("0.2.6-test.81", None, &[]), &s));
    assert!(!behind(&known("0.2.6-test.81", Some("bb"), &[]), &shipped("0.2.6-test.81", None)), "without the release's hashes only the version is compared");
}

#[test]
fn a_bad_request_to_version_means_older_and_anything_else_is_an_error() {
    assert_eq!(read_reply(Err(anyhow!("bad request: unknown variant `version`"))).unwrap(), Seen::Old);
    assert!(read_reply(Err(anyhow!("reach magnus-computerd"))).is_err());
    let v = read_reply(Ok(json!({ "ok": true, "version": "1.0.0", "base_id": "ab", "ops": ["create", "version"] }))).unwrap();
    assert_eq!(v, known("1.0.0", Some("ab"), &["create", "version"]));
}

#[test]
fn only_a_listed_request_is_surely_served() {
    let s = known("1.0.0", None, &["create"]);
    assert!(lacks(Some(&s), "erase_volume"));
    assert!(!lacks(Some(&s), "create"));
    assert!(!lacks(Some(&Seen::Old), "erase_volume"), "an old computerd is sent the request; its refusal is put in plain words");
    assert!(!lacks(None, "erase_volume"));
    assert!(can_surely(&s, "create"));
    assert!(!can_surely(&Seen::Old, "erase_volume"), "an old computerd is not trusted to erase");
}

#[test]
fn the_release_base_image_id_comes_from_its_signed_hashes() {
    let d = tempfile::tempdir().unwrap();
    assert_eq!(shipped_base_id(d.path()), None);
    std::fs::write(d.path().join("images.sha256"), "11AA  vmlinux\n22bb  rootfs.ext4\n33CC  computer.ext4\n").unwrap();
    assert_eq!(shipped_base_id(d.path()).as_deref(), Some("33cc"), "the computer's base is computer.ext4, not the sandbox's rootfs.ext4");
}

/// The update installs the computer's own image, whatever the sandbox's
/// rootfs is (decisions/2026-11-13-minimal-computer-image.md).
#[test]
fn the_update_installs_the_computer_image_not_the_sandboxs() {
    let dir = Path::new("/home/sam/.magnus-teams/bin");
    let (kernel, base) = update_images(dir, Some(Path::new("/x/vmlinux")));
    assert_eq!(kernel, Path::new("/x/vmlinux"));
    assert_eq!(base, dir.join("computer.ext4"));
    let (kernel, base) = update_images(dir, None);
    assert_eq!((kernel, base), (dir.join("vmlinux"), dir.join("computer.ext4")));
}

#[test]
fn the_update_command_names_the_shipped_installer_and_the_release_directory() {
    let c = update_command(
        Path::new("/home/sam/.magnus-teams/bin"),
        Path::new("/home/sam/.magnus-teams/bin/vmlinux"),
        Path::new("/x/my rootfs"),
        "sam",
        Path::new("/s/updates/appliance"),
    );
    assert_eq!(
        c,
        "sudo sh /home/sam/.magnus-teams/bin/install-computerd.sh /home/sam/.magnus-teams/bin/magnus-computerd /home/sam/.magnus-teams/bin/vmlinux '/x/my rootfs' sam /s/updates/appliance"
    );
}

/// A computerd from before `version` and `erase_volume`: it refuses both
/// as the real one did, "bad request".
fn old_computerd(socket: &Path) {
    let listener = UnixListener::bind(socket).unwrap();
    tokio::spawn(async move {
        while let Ok((s, _)) = listener.accept().await {
            tokio::spawn(async move {
                let (r, mut w) = s.into_split();
                let mut line = String::new();
                BufReader::new(r).read_line(&mut line).await.unwrap();
                let req: Value = serde_json::from_str(&line).unwrap();
                let reply = match req["op"].as_str().unwrap() {
                    "capacity" => json!({ "ok": true, "live": 0 }),
                    op => json!({ "ok": false, "error": format!("bad request: unknown variant `{op}`, expected one of `create`, `capacity`") }),
                };
                w.write_all(format!("{reply}\n").as_bytes()).await.unwrap();
            });
        }
    });
}

#[tokio::test]
async fn an_old_computerd_is_seen_as_behind_and_its_bad_request_is_put_plainly() {
    let d = tempfile::tempdir().unwrap();
    let socket = d.path().join("computerd.sock");
    old_computerd(&socket);
    assert_eq!(check(&socket).await.unwrap(), Seen::Old);
    let e = super::super::client::ask(&socket, json!({ "op": "erase_volume", "id": "abc" })).await.unwrap_err();
    assert_eq!(e.to_string(), OUTDATED);
    assert!(!e.to_string().contains("bad request"));
    assert!(super::super::client::ask(&socket, json!({ "op": "capacity" })).await.is_ok(), "what it can do still works");
    assert!(!serves(&socket, "erase_volume").await.unwrap());
}

/// A computerd that says what it serves is not sent what it does not.
#[tokio::test]
async fn a_request_computerd_does_not_list_is_refused_before_it_is_sent() {
    let d = tempfile::tempdir().unwrap();
    let socket = d.path().join("computerd.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let asked = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let log = asked.clone();
    tokio::spawn(async move {
        while let Ok((s, _)) = listener.accept().await {
            let log = log.clone();
            tokio::spawn(async move {
                let (r, mut w) = s.into_split();
                let mut line = String::new();
                BufReader::new(r).read_line(&mut line).await.unwrap();
                let req: Value = serde_json::from_str(&line).unwrap();
                let op = req["op"].as_str().unwrap().to_string();
                log.lock().unwrap().push(op.clone());
                let reply = if op == "version" { json!({ "ok": true, "version": "1.0.0", "base_id": null, "ops": ["version", "capacity"] }) } else { json!({ "ok": true }) };
                w.write_all(format!("{reply}\n").as_bytes()).await.unwrap();
            });
        }
    });
    check(&socket).await.unwrap();
    let e = super::super::client::ask(&socket, json!({ "op": "a_request_from_the_future", "id": "abc" })).await.unwrap_err();
    assert_eq!(e.to_string(), OUTDATED);
    assert!(!asked.lock().unwrap().iter().any(|o| o == "a_request_from_the_future"), "it was sent anyway");
    super::super::client::ask(&socket, json!({ "op": "capacity" })).await.unwrap();
}
