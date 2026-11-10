//! What only the installed app does, in the real WebKit app against a real
//! appliance and computer: `desktop/tests/app-native.mjs`. Signs in with a
//! seeded session, over plain http on loopback. Run with `--ignored`; needs
//! `cargo tauri build --no-bundle`, `magnus-local` on the PATH, tauri-driver,
//! WebKitWebDriver and xvfb-run.

// Test code: a failed unwrap is a failed test (AGENTS.md: none outside tests).
#![allow(clippy::unwrap_used, clippy::expect_used)]
use magnus_invariants::*;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

const DRIVER_START: Duration = Duration::from_secs(30);

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

/// The store's own layout: where it looks when there is no keyring.
fn seed_session(config: &Path, url: &str, token: &str) {
    let dir = config.join("magnus");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("app.json"), json!({ "appliance": url, "pins": {} }).to_string()).unwrap();
    let hash = std::process::Command::new("sh").arg("-c").arg(format!("printf %s '{url}' | sha256sum")).output().unwrap().stdout;
    let name = format!("session-{}", &String::from_utf8(hash).unwrap()[..16]);
    std::fs::write(dir.join(name), token).unwrap();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs the built desktop app, tauri-driver, WebKitWebDriver and xvfb-run"]
async fn the_installed_app_runs_frames_connects_this_computer_and_carries_a_takeover() {
    let computerd = Computerd::start();
    let page = "Here.\n```magnus-html id=page\n<body><script>parent.postMessage(\"script ran\",\"*\")</script>\n```".to_string();
    let model = FakeModel::start(steps(vec![Reply::Text(page)])).await;
    let spec = Spec::default().computer(&computerd.socket);
    let token = spec.token("alice");
    let app = Appliance::start(spec, &model).await;
    let alice = app.api("alice");
    let (_, made) = alice.post("/v1/computers", json!({})).await;
    let computer = made["id"].as_str().unwrap().to_string();
    assert_eq!(alice.post(&format!("/v1/computers/{computer}/start"), json!({})).await.0, 200);

    let home = tempfile::tempdir().unwrap();
    let config = home.path().join(".config");
    seed_session(&config, &app.base, &token);
    let folder = home.path().join("granted");
    std::fs::create_dir_all(&folder).unwrap();
    let tmp = home.path().join("tmp");
    std::fs::create_dir_all(&tmp).unwrap();

    let path = format!("{}:{}:{}", root().join("target/debug").display(), std::env::var("HOME").map(|h| format!("{h}/.cargo/bin")).unwrap(), std::env::var("PATH").unwrap());
    let (port, native_port) = (free_port(), free_port());
    let mut driver = tokio::process::Command::new("xvfb-run")
        .args(["-a", "-s", "-screen 0 1280x900x24", "tauri-driver", "--port", &port.to_string(), "--native-port", &native_port.to_string()])
        .env("PATH", &path)
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", &config)
        .env("TMPDIR", &tmp)
        .env("DBUS_SESSION_BUS_ADDRESS", "unix:path=/nonexistent")
        .env("NO_AT_BRIDGE", "1")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let wd = format!("http://127.0.0.1:{port}");
    eventually("tauri-driver", DRIVER_START, || {
        let wd = wd.clone();
        async move { reqwest::get(format!("{wd}/status")).await.ok().map(|_| ()) }
    })
    .await;

    let out = tokio::process::Command::new("node")
        .arg(root().join("desktop/tests/app-native.mjs"))
        .env("MAGNUS_WEBDRIVER", &wd)
        .env("MAGNUS_APP", std::env::var_os("MAGNUS_DESKTOP_APP").unwrap_or_else(|| root().join("desktop/src-tauri/target/release/magnus-desktop").into()))
        .env("MAGNUS_COMPUTER", &computer)
        .env("MAGNUS_FOLDER", &folder)
        .envs(std::env::var("MAGNUS_SHOTS").ok().map(|s| ("MAGNUS_SHOTS", s)))
        .output()
        .await
        .unwrap();
    let said = format!("{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    if let Some(pid) = driver.id() {
        let _ = std::process::Command::new("kill").args(["-TERM", "--", &format!("-{pid}")]).status();
    }
    let _ = driver.kill().await;
    assert!(out.status.success() && said.contains("native ok"), "{said}\n{}", app.logs().lines().rev().take(20).collect::<Vec<_>>().join("\n"));
}
