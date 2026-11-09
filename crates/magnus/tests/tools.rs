//! Integration tests for the browser sidecar and the sandbox desktop, using
//! real Chromium and a real Firecracker VM.

use magnus::{boundary::computer_command, browser::Browser, config, sandbox::Sandboxes};
use serde_json::json;
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

async fn page_server() -> String {
    use axum::{response::Html, routing::get, Router};
    let app = Router::new()
        .route("/", get(|| async { Html("<html><head><title>Shop</title></head><body><h1>Welcome</h1><a href='/next'>Go next</a><input placeholder='Your name'></body></html>") }))
        .route("/next", get(|| async { Html("<html><body><p>Second page</p></body></html>") }))
        .route("/sign-in", get(|| async { Html("<html><body><input id='user' value='dana@example.com'><input type='password' value='PASSWORD-CANARY'><input autocomplete='one-time-code' value='OTP-CANARY'><input autocomplete='cc-number' value='CARD-CANARY'><input type='password'></body></html>") }))
        .route("/non-web-link", get(|| async { Html("<html><body><a id='open'>Open document</a><script>document.querySelector('a').href = URL.createObjectURL(new Blob(['NON-WEB-DOCUMENT'], {type:'text/html'}));</script></body></html>") }));
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    url
}

#[tokio::test(flavor = "multi_thread")]
async fn browser_navigates_clicks_types_and_screenshots() {
    let chromium = std::env::var("MAGNUS_CHROMIUM")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(std::env::var("HOME").unwrap()).join(".cache/ms-playwright/chromium-1234/chrome-linux64/chrome"));
    let dir = tempfile::tempdir().unwrap();
    let b = Browser::new(
        Some(config::Browser { command: vec!["node".into(), root().join("browser/sidecar.js").to_string_lossy().into()], chromium, allow: vec!["127.0.0.1".into()] }),
        dir.path(),
    );
    let url = page_server().await;
    let v = b.request("r1", json!({ "op": "navigate", "url": url })).await.unwrap();
    let snap = v["text"].as_str().unwrap();
    assert!(snap.contains("Title: Shop") && snap.contains("Welcome"), "{snap}");
    assert!(snap.contains("[e1] link \"Go next\""), "{snap}");
    let v = b.request("r1", json!({ "op": "type", "target": "e2", "text": "Dana" })).await.unwrap();
    assert!(v["text"].as_str().unwrap().contains("= \"Dana\""), "{}", v["text"]);
    let v = b.request("r1", json!({ "op": "click", "target": "Go next" })).await.unwrap();
    assert!(v["text"].as_str().unwrap().contains("Second page"), "{}", v["text"]);
    let v = b.request("r1", json!({ "op": "screenshot" })).await.unwrap();
    assert!(v["image_b64"].as_str().unwrap().starts_with("iVBOR"), "not a PNG");
    // Runs are isolated: a second run starts blank.
    let v = b.request("r2", json!({ "op": "snapshot" })).await.unwrap();
    assert!(v["text"].as_str().unwrap().contains("about:blank"));
}

/// The snapshot is the model's view of the page. A password, a one-time
/// code or a card number a person filled in must not reach it as text.
#[tokio::test(flavor = "multi_thread")]
async fn browser_snapshot_never_shows_a_secret_field_value() {
    let chromium = std::env::var("MAGNUS_CHROMIUM")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(std::env::var("HOME").unwrap()).join(".cache/ms-playwright/chromium-1234/chrome-linux64/chrome"));
    let dir = tempfile::tempdir().unwrap();
    let b = Browser::new(
        Some(config::Browser { command: vec!["node".into(), root().join("browser/sidecar.js").to_string_lossy().into()], chromium, allow: vec!["127.0.0.1".into()] }),
        dir.path(),
    );
    let url = page_server().await;
    let v = b.request("r1", json!({ "op": "navigate", "url": format!("{url}/sign-in") })).await.unwrap();
    let snap = v["text"].as_str().unwrap();
    assert!(snap.contains("= \"dana@example.com\""), "{snap}");
    for secret in ["PASSWORD-CANARY", "OTP-CANARY", "CARD-CANARY"] {
        assert!(!snap.contains(secret), "{secret} reached the snapshot:\n{snap}");
    }
    // A filled secret field is still there for the model to target and
    // shows that it holds a value; an empty one shows no marker.
    assert!(snap.contains("[e2] password input \"\" = (hidden)"), "{snap}");
    assert!(snap.contains("[e5] password input \"\"\n") || snap.ends_with("[e5] password input \"\""), "{snap}");
    // Typing into one through the browser tool is not shown back either.
    let v = b.request("r1", json!({ "op": "type", "target": "e5", "text": "TYPED-CANARY" })).await.unwrap();
    let snap = v["text"].as_str().unwrap();
    assert!(!snap.contains("TYPED-CANARY") && snap.contains("[e5] password input \"\" = (hidden)"), "{snap}");
    b.close("r1").await;
}

/// The click that failed release-checks gave up after 10 seconds. The sidecar
/// waits long enough for a build box that is also compiling.
#[test]
fn a_browser_action_waits_out_a_busy_runner() {
    let src = std::fs::read_to_string(root().join("browser/sidecar.js")).unwrap();
    let rest = src.split("const ACTION_MS = ").nth(1).expect("ACTION_MS");
    let ms: u64 = rest.split(|c: char| !c.is_ascii_digit()).next().unwrap().parse().unwrap();
    assert!(ms >= 30_000, "a click timed out at 10s on the build box; ACTION_MS is {ms}");
    assert!(src.contains("timeout: ACTION_MS"), "click, type and select use ACTION_MS");
}

#[tokio::test(flavor = "multi_thread")]
async fn browser_cannot_read_the_appliances_files_or_execute_a_navigation_script() {
    let chromium = std::env::var("MAGNUS_CHROMIUM")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(std::env::var("HOME").unwrap()).join(".cache/ms-playwright/chromium-1234/chrome-linux64/chrome"));
    let dir = tempfile::tempdir().unwrap();
    let canary = dir.path().join("private.txt");
    std::fs::write(&canary, "PRIVATE-HOST-FILE-CANARY").unwrap();
    let b = Browser::new(
        Some(config::Browser { command: vec!["node".into(), root().join("browser/sidecar.js").to_string_lossy().into()], chromium, allow: vec!["127.0.0.1".into()] }),
        dir.path(),
    );
    let url = page_server().await;
    b.request("r1", json!({ "op": "navigate", "url": url })).await.unwrap();
    for forbidden in [
        url::Url::from_file_path(&canary).unwrap().to_string(),
        "javascript:document.body.textContent='SCRIPT-RAN'".into(),
        "data:text/html,INLINE-DOCUMENT".into(),
        "chrome://version".into(),
    ] {
        let result = b.request("r1", json!({ "op": "navigate", "url": forbidden })).await;
        assert!(result.is_err(), "the browser accepted {forbidden}: {result:?}");
    }
    let v = b.request("r1", json!({ "op": "snapshot" })).await.unwrap();
    let text = v["text"].as_str().unwrap();
    assert!(text.contains("Welcome") && !text.contains("PRIVATE-HOST-FILE-CANARY") && !text.contains("SCRIPT-RAN"), "{text}");
    // A page can navigate indirectly. The observation guards must also
    // refuse a non-web document reached without the navigate command.
    b.request("r1", json!({ "op": "navigate", "url": format!("{url}/non-web-link") })).await.unwrap();
    let error = b.request("r1", json!({ "op": "click", "target": "Open document" })).await.unwrap_err();
    assert!(error.to_string().contains("only open http and https"), "{error}");
    for op in ["snapshot", "screenshot"] {
        let error = b.request("r1", json!({ "op": op })).await.unwrap_err();
        assert!(error.to_string().contains("only open http and https"), "{error}");
    }
    b.close("r1").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn sandbox_desktop_takes_screenshots_and_types() {
    let out = root().join("sandbox/out");
    let dir = tempfile::tempdir().unwrap();
    let s = Sandboxes::new(
        Some(config::Sandbox {
            firecracker: "/usr/local/bin/firecracker".into(),
            kernel_image: out.join("vmlinux"),
            rootfs: out.join("rootfs.ext4"),
            jailer: None,
            vcpus: 2,
            memory_mib: 1024,
            workspace_mib: 512,
        }),
        dir.path(),
    );
    let cmd = computer_command("computer_screenshot", &json!({})).unwrap();
    let v = s.exec("r1", &cmd, 60).await.unwrap();
    assert_eq!(v["exit_code"], 0, "{v}");
    let png = s.read("r1", "/tmp/magnus-shot.png").await.unwrap();
    assert_eq!(&png[..4], b"\x89PNG");
    // xterm is not focused after a fixed sleep when the guest is busy
    // (run 41929170386: /workspace/typed.txt was never written). Wait until
    // its window is the one that receives keys, then until the file appears.
    let v = s
        .exec(
            "r1",
            "export DISPLAY=:0; (xterm &); ok=; for i in $(seq 40); do xdotool search --onlyvisible --class xterm windowactivate --sync && ok=1 && break; sleep 0.25; done; test -n \"$ok\"",
            30,
        )
        .await
        .unwrap();
    assert_eq!(v["exit_code"], 0, "{v}");
    let cmd = computer_command("computer_type", &json!({ "text": "echo hi > /workspace/typed.txt\n" })).unwrap();
    let v = s.exec("r1", &cmd, 60).await.unwrap();
    assert_eq!(v["exit_code"], 0, "{v}");
    let mut typed = Vec::new();
    for _ in 0..40 {
        if let Ok(b) = s.read("r1", "/workspace/typed.txt").await {
            if !b.is_empty() {
                typed = b;
                break;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
    assert_eq!(String::from_utf8_lossy(&typed).trim(), "hi");
    s.destroy("r1").await;
}

/// The kill switch closes every run's pages with the browser; a run that
/// goes on afterwards gets a new browser and starts blank.
#[tokio::test(flavor = "multi_thread")]
async fn closing_the_browser_closes_every_runs_pages_and_a_later_request_starts_blank() {
    let chromium = std::env::var("MAGNUS_CHROMIUM")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(std::env::var("HOME").unwrap()).join(".cache/ms-playwright/chromium-1234/chrome-linux64/chrome"));
    let dir = tempfile::tempdir().unwrap();
    let b = Browser::new(
        Some(config::Browser { command: vec!["node".into(), root().join("browser/sidecar.js").to_string_lossy().into()], chromium, allow: vec!["127.0.0.1".into()] }),
        dir.path(),
    );
    let url = page_server().await;
    for run in ["r1", "r2"] {
        b.request(run, json!({ "op": "navigate", "url": url })).await.unwrap();
    }
    b.close_all().await;
    for run in ["r1", "r2"] {
        let v = b.request(run, json!({ "op": "snapshot" })).await.unwrap();
        let text = v["text"].as_str().unwrap();
        assert!(text.contains("about:blank") && !text.contains("Welcome"), "{run} kept its page: {text}");
    }
    let v = b.request("r1", json!({ "op": "navigate", "url": url })).await.unwrap();
    assert!(v["text"].as_str().unwrap().contains("Welcome"), "{}", v["text"]);
}

// Where the browser may connect (#185). The sites below are on this machine;
// on master the browser opened every one of them.

fn sidecar(allow: &[String]) -> config::Browser {
    let chromium = std::env::var("MAGNUS_CHROMIUM")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(std::env::var("HOME").unwrap()).join(".cache/ms-playwright/chromium-1234/chrome-linux64/chrome"));
    config::Browser { command: vec!["node".into(), root().join("browser/sidecar.js").to_string_lossy().into()], chromium, allow: allow.to_vec() }
}

/// A site on every address of this machine, IPv4 and IPv6, that counts
/// each request it gets: what a page on the appliance's host or LAN is.
async fn private_site() -> (u16, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let hits = std::sync::Arc::new(AtomicUsize::new(0));
    let h = hits.clone();
    let app = axum::Router::new().fallback(move || {
        let h = h.clone();
        async move {
            h.fetch_add(1, Ordering::SeqCst);
            axum::response::Html("<html><body><p>PRIVATE-CANARY</p></body></html>")
        }
    });
    let l = tokio::net::TcpListener::bind("[::]:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    (port, hits)
}

/// A site the owner allows (`[browser] allow = ["127.0.0.1:<port>"]`),
/// standing in for a public one, that sends the browser on to `private`.
async fn allowed_site(private: u16) -> (String, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
    use axum::{response::Html, routing::get, Router};
    use std::sync::atomic::{AtomicUsize, Ordering};
    let done = std::sync::Arc::new(AtomicUsize::new(0));
    let d = done.clone();
    let subrequests = format!(
        "<html><body><h1>Subrequests</h1>\
         <img src='http://127.0.0.1:{private}/img'><iframe src='http://localhost:{private}/frame'></iframe>\
         <script>Promise.all([\
           fetch('http://127.0.0.1:{private}/fetch', {{ mode: 'no-cors' }}).catch(() => {{}}),\
           new Promise(r => {{ const w = new WebSocket('ws://127.0.0.1:{private}/ws'); w.onopen = w.onerror = w.onclose = r; }}),\
           new Promise(r => {{ const i = new Image(); i.onload = i.onerror = r; i.src = 'http://[::ffff:127.0.0.1]:{private}/mapped'; }}),\
         ]).then(() => fetch('/done'));</script></body></html>"
    );
    let app = Router::new()
        .route("/", get(|| async { Html("<html><body><h1>Allowed page</h1></body></html>") }))
        .route("/redirect", get(move || async move { axum::response::Redirect::temporary(&format!("http://127.0.0.1:{private}/secret")) }))
        .route("/subrequests", get(move || async move { Html(subrequests) }))
        .route(
            "/done",
            get(move || async move {
                d.fetch_add(1, Ordering::SeqCst);
                ""
            }),
        );
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    (url, done)
}

async fn refused(b: &Browser, url: &str) {
    match b.request("r1", json!({ "op": "navigate", "url": url })).await {
        Ok(v) => panic!("the browser opened {url}: {}", v["text"]),
        Err(e) => assert!(e.to_string().contains("opens only public websites"), "{url} was not refused as a private address: {e}"),
    }
}

/// This machine's name, which resolves to its own (non-public) addresses.
async fn own_name(port: u16) -> (String, Vec<std::net::IpAddr>) {
    let name = String::from_utf8(std::process::Command::new("hostname").output().unwrap().stdout).unwrap().trim().to_string();
    let ips = tokio::net::lookup_host((name.as_str(), port)).await.unwrap().map(|a| a.ip()).collect();
    (name, ips)
}

#[tokio::test(flavor = "multi_thread")]
async fn the_browser_refuses_loopback_however_it_is_written_including_mapped_ipv4() {
    let (port, hits) = private_site().await;
    let dir = tempfile::tempdir().unwrap();
    let b = Browser::new(Some(sidecar(&[])), dir.path());
    for host in [
        "localhost",
        "LocalHost.",
        "127.0.0.1",
        "127.1",
        "2130706433",
        "127.9.9.9",
        "[::1]",
        "0.0.0.0",
        "[::]",
        "[::ffff:127.0.0.1]",
        "[::ffff:7f00:1]",
        "[0:0:0:0:0:ffff:127.0.0.1]",
    ] {
        refused(&b, &format!("http://{host}:{port}/")).await;
    }
    assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 0, "a loopback page was fetched");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_browser_refuses_metadata_link_local_and_private_network_addresses() {
    let (port, hits) = private_site().await;
    let dir = tempfile::tempdir().unwrap();
    let b = Browser::new(Some(sidecar(&[])), dir.path());
    // This machine's own LAN address first: a private address that answers.
    let (_, ips) = own_name(port).await;
    let lan: Vec<String> = ips.iter().filter(|ip| matches!(ip, std::net::IpAddr::V4(v) if v.is_private())).map(|ip| ip.to_string()).collect();
    for host in lan.iter().map(String::as_str).chain([
        "169.254.169.254",
        "[::ffff:169.254.169.254]",
        "[fe80::1]",
        "10.0.0.1",
        "172.16.0.1",
        "192.168.1.1",
        "[::ffff:192.168.1.1]",
        "[fd00::1]",
        "100.64.0.1",
    ]) {
        refused(&b, &format!("http://{host}:{port}/")).await;
        refused(&b, &format!("https://{host}/")).await;
    }
    assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 0, "a LAN page was fetched");
}

/// DNS rebinding's target: a name, not an address, that resolves to a
/// private address is judged by that address. The owner can allow it.
#[tokio::test(flavor = "multi_thread")]
async fn a_name_resolving_to_a_private_address_is_refused_unless_the_owner_allows_it() {
    use std::sync::atomic::Ordering;
    let (port, hits) = private_site().await;
    // `localhost`, not this machine's name: DNS here answers the machine's
    // name with a stale address it does not have (#426), so navigating to
    // it is roulette. localhost resolves to loopback everywhere, which is
    // still a name resolving to a private address.
    let name = "localhost".to_string();
    let ips: Vec<_> = tokio::net::lookup_host((name.as_str(), port)).await.unwrap().map(|a| a.ip()).collect();
    assert!(!ips.is_empty(), "{name} does not resolve");
    let dir = tempfile::tempdir().unwrap();
    let b = Browser::new(Some(sidecar(&[])), dir.path());
    refused(&b, &format!("http://{name}:{port}/")).await;
    refused(&b, &format!("http://rebind.localhost:{port}/")).await;
    assert_eq!(hits.load(Ordering::SeqCst), 0, "{name} ({ips:?}) was fetched");

    let dir = tempfile::tempdir().unwrap();
    let b = Browser::new(Some(sidecar(&[format!("{name}:{port}")])), dir.path());
    let v = b.request("r1", json!({ "op": "navigate", "url": format!("http://{name}:{port}/") })).await.unwrap();
    assert!(v["text"].as_str().unwrap().contains("PRIVATE-CANARY"), "{}", v["text"]);
    // Allowing the name allows that site, not every private address.
    refused(&b, &format!("http://127.0.0.1:{port}/")).await;
    b.close("r1").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn an_allowed_sites_redirect_to_a_private_address_is_refused() {
    let (port, hits) = private_site().await;
    let (site, _) = allowed_site(port).await;
    let dir = tempfile::tempdir().unwrap();
    let b = Browser::new(Some(sidecar(&[site.trim_start_matches("http://").to_string()])), dir.path());
    let v = b.request("r1", json!({ "op": "navigate", "url": site })).await.unwrap();
    assert!(v["text"].as_str().unwrap().contains("Allowed page"), "{}", v["text"]);
    refused(&b, &format!("{site}/redirect")).await;
    assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 0, "the redirect reached the private address");
    b.close("r1").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn an_allowed_pages_images_frames_fetches_and_websockets_never_reach_a_private_address() {
    use std::sync::atomic::Ordering;
    let (port, hits) = private_site().await;
    let (site, done) = allowed_site(port).await;
    let dir = tempfile::tempdir().unwrap();
    let b = Browser::new(Some(sidecar(&[site.trim_start_matches("http://").to_string()])), dir.path());
    let v = b.request("r1", json!({ "op": "navigate", "url": format!("{site}/subrequests") })).await.unwrap();
    assert!(v["text"].as_str().unwrap().contains("Subrequests"), "{}", v["text"]);
    for _ in 0..200 {
        if done.load(Ordering::SeqCst) > 0 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(done.load(Ordering::SeqCst) > 0, "the page's subrequests never settled");
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    assert_eq!(hits.load(Ordering::SeqCst), 0, "the page's subrequests reached the private address");
    b.close("r1").await;
}

/// Public addresses are websites; the special-purpose ranges are not, in
/// either IP version or written as IPv6.
#[test]
fn public_addresses_are_websites_and_special_purpose_ones_are_not() {
    let public = ["8.8.8.8", "1.1.1.1", "172.32.0.1", "100.128.0.1", "2606:4700:4700::1111", "::ffff:8.8.8.8"];
    let not_public = [
        "127.0.0.1",
        "::1",
        "::ffff:127.0.0.1",
        "0.0.0.0",
        "::",
        "169.254.169.254",
        "fe80::1",
        "10.1.2.3",
        "172.16.0.1",
        "172.31.255.255",
        "192.168.1.1",
        "fd00::1",
        "fc00::1",
        "100.64.0.1",
        "224.0.0.1",
        "255.255.255.255",
        "198.18.0.1",
        "192.0.2.1",
        "2001:db8::1",
        "64:ff9b::a00:1",
        "2002:7f00:1::",
        "::ffff:10.0.0.1",
        "::ffff:169.254.169.254",
    ];
    let script =
        format!("const {{ notPublic }} = require({:?}); console.log(JSON.stringify(process.argv.slice(1).map(ip => [ip, notPublic(ip)])))", root().join("browser/destinations.js"));
    let out = std::process::Command::new("node").arg("-e").arg(&script).args(public).args(not_public).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let judged: Vec<(String, Option<String>)> = serde_json::from_slice(&out.stdout).unwrap();
    for (ip, why) in judged {
        assert_eq!(why.is_none(), public.contains(&ip.as_str()), "{ip}: {why:?}");
    }
}

/// A name with two allowed addresses whose first refuses connections, as a
/// dual-stack site with an unreachable IPv6 endpoint: the browser reaches
/// the site at the second address, as Chromium would without the proxy.
#[tokio::test(flavor = "multi_thread")]
async fn a_name_whose_first_allowed_address_refuses_is_reached_at_the_next() {
    // `*.localhost` resolves to ::1 and 127.0.0.1 (RFC 6761).
    let name = "dual-stack.localhost";
    let ips: Vec<std::net::IpAddr> = tokio::net::lookup_host((name, 0)).await.unwrap().map(|a| a.ip()).collect();
    assert!(ips.len() >= 2, "{name} resolves to {ips:?}, not two addresses");
    let l = tokio::net::TcpListener::bind((ips[1], 0)).await.unwrap();
    let port = l.local_addr().unwrap().port();
    assert!(tokio::net::TcpStream::connect((ips[0], port)).await.is_err(), "{}:{port} accepts connections", ips[0]);
    let app = axum::Router::new().fallback(|| async { axum::response::Html("<html><body><p>SECOND-ADDRESS</p></body></html>") });
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    let dir = tempfile::tempdir().unwrap();
    let b = Browser::new(Some(sidecar(&[name.to_string()])), dir.path());
    let v = b.request("r1", json!({ "op": "navigate", "url": format!("http://{name}:{port}/") })).await.unwrap();
    assert!(v["text"].as_str().unwrap().contains("SECOND-ADDRESS"), "{ips:?}: {}", v["text"]);
    b.close("r1").await;
}

/// An origin that promises 100 bytes and disconnects after a few: the
/// page's request for it fails when the origin goes, instead of waiting for
/// bytes that never come.
#[tokio::test(flavor = "multi_thread")]
async fn a_request_whose_origin_disconnects_mid_body_fails_promptly() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    const PAGE: &str =
        "<html><body><h1>Cut</h1><script>fetch('/cut').then(r => r.text()).then(() => 'read', () => 'failed').then(x => fetch('/done?' + x));</script></body></html>";
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let site = format!("http://{}", l.local_addr().unwrap());
    let (settled, mut outcome) = tokio::sync::mpsc::unbounded_channel::<String>();
    tokio::spawn(async move {
        loop {
            let (mut s, _) = l.accept().await.unwrap();
            let settled = settled.clone();
            tokio::spawn(async move {
                loop {
                    let mut req = Vec::new();
                    let mut buf = [0u8; 4096];
                    while !req.windows(4).any(|w| w == b"\r\n\r\n") {
                        match s.read(&mut buf).await {
                            Ok(0) | Err(_) => return,
                            Ok(n) => req.extend_from_slice(&buf[..n]),
                        }
                    }
                    let line = String::from_utf8_lossy(&req).lines().next().unwrap_or_default().to_string();
                    if line.contains(" /cut ") {
                        let _ = s.write_all(b"HTTP/1.1 200 OK\r\ncontent-type: text/plain\r\ncontent-length: 100\r\n\r\nCUT").await;
                        return; // the origin closes 97 bytes short
                    }
                    if line.contains(" /done?") {
                        let _ = settled.send(line.clone());
                    }
                    let body = if line.contains(" / ") { PAGE } else { "" };
                    let head = format!("HTTP/1.1 200 OK\r\ncontent-type: text/html\r\ncontent-length: {}\r\n\r\n", body.len());
                    if s.write_all(format!("{head}{body}").as_bytes()).await.is_err() {
                        return;
                    }
                }
            });
        }
    });
    let dir = tempfile::tempdir().unwrap();
    let b = Browser::new(Some(sidecar(&[site.trim_start_matches("http://").to_string()])), dir.path());
    let started = std::time::Instant::now();
    let v = b.request("r1", json!({ "op": "navigate", "url": format!("{site}/") })).await.unwrap();
    assert!(v["text"].as_str().unwrap().contains("Cut"), "{}", v["text"]);
    let line = tokio::time::timeout(std::time::Duration::from_secs(15), outcome.recv()).await;
    let line = line.unwrap_or_else(|_| panic!("the page's request still waited {:?} after the origin disconnected mid-body", started.elapsed()));
    assert!(line.unwrap().contains("/done?failed"), "the cut body was read as complete");
    b.close("r1").await;
}
