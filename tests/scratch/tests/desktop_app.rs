//! The installed desktop app, driven through tauri-driver against a real
//! appliance over HTTPS. Each test pins the certificate and signs in through
//! a real Keycloak in the computer's default browser, then does what crosses
//! into the desktop shell:
//! - `desktop/tests/app.mjs`: connect Claude, type a task, approve in the
//!   inbox, see the outcome with its evidence, download the delivered file.
//! - `desktop/tests/app-library.mjs`: download a file from the library.
//! - `desktop/tests/app-people.mjs`: two people sign in on one app, one after
//!   the other, each with their own rights.
//! - `desktop/tests/app-apps.mjs`: the owner adds one of the person's apps
//!   and signs in to it through a loopback address the shell opens.
//! - `desktop/tests/app-update.mjs`: the app, an AppImage, updates itself
//!   from the appliance while a task waits there, and restarts as the new
//!   version (two builds signed with a test key: MAGNUS_UPDATE_BUILDS).
//! - `desktop/tests/app-workspace.mjs`: research, approval, result, file and
//!   schedule in one conversation, each flow held to its ease-of-use budget
//!   (`desktop/tests/ease-budgets.json`).
//! - `desktop/tests/app-looks.mjs`: light and dark, each readable, every
//!   surface solid.
//! - `desktop/tests/app-channels.mjs`: the owner's app updates from the
//!   development channel (pull requests' builds), asking with its session.
//!
//! Needs the built app (`cargo tauri build` in desktop/src-tauri, or
//! MAGNUS_DESKTOP_APP), `tauri-driver`, `WebKitWebDriver`
//! (MAGNUS_WEBKIT_DRIVER) and `xvfb-run`. Run with `--ignored`.

fn workspace_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}
use magnus_invariants::{canary, eventually, PATIENCE};
use magnus_scratch::*;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::process::Stdio;

/// A free loopback port below the ephemeral range: outgoing connections take
/// their local ports from that range (`ip_local_port_range`), and the tests
/// make many, so a port picked there can be taken by one before the listener
/// it was picked for binds it.
fn listener_port() -> u16 {
    let low: u16 =
        std::fs::read_to_string("/proc/sys/net/ipv4/ip_local_port_range").ok().and_then(|r| r.split_whitespace().next()?.parse().ok()).expect("the ephemeral port range");
    loop {
        let p = 1024 + (uuid::Uuid::new_v4().as_u128() % u128::from(low - 1024)) as u16;
        if std::net::TcpListener::bind(("127.0.0.1", p)).is_ok() {
            return p;
        }
    }
}

/// What listens on loopback `port`, as `ss` says.
fn listener(port: u16) -> String {
    let out = std::process::Command::new("ss").args(["-Hltnp", &format!("sport = :{port}")]).output().expect("ss");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// Whether the process listening on loopback `port` descends from `root`;
/// None while nothing listens there, or while `ss` does not yet name its
/// owner: it reads the processes' open files before it lists the sockets, so
/// a socket bound in between is listed without one (as this driver's own
/// were, three times in a row, in CI on 2026-10-31). A driver that cannot
/// bind its port exits, which the caller sees.
fn held_by(port: u16, root: u32) -> Option<bool> {
    let text = listener(port);
    let pids: Vec<u32> = text.split("pid=").skip(1).filter_map(|s| s.split(|c: char| !c.is_ascii_digit()).next()?.parse().ok()).collect();
    if pids.is_empty() {
        return None;
    }
    Some(pids.iter().any(|&p| {
        let mut p = p;
        while p > 1 {
            if p == root {
                return true;
            }
            // The parent pid is the second field after the parenthesised name.
            let stat = std::fs::read_to_string(format!("/proc/{p}/stat")).unwrap_or_default();
            p = stat.rsplit(')').next().and_then(|r| r.split_whitespace().nth(1)).and_then(|s| s.parse().ok()).unwrap_or(0);
        }
        false
    }))
}

fn which(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|p| std::env::split_paths(&p).map(|d| d.join(name)).find(|f| f.is_file()))
}

fn app_bin() -> PathBuf {
    let app = std::env::var("MAGNUS_DESKTOP_APP").map(PathBuf::from).unwrap_or_else(|_| workspace_root().join("desktop/src-tauri/target/release/magnus-desktop"));
    assert!(app.is_file(), "build the app first: {}", app.display());
    app
}

/// tauri-driver under a virtual display, for a fresh user: its own home,
/// settings, Downloads and temporary directory, and no keyring, so the
/// session goes to the private file.
struct Driver {
    wd: String,
    home: tempfile::TempDir,
    _driver: tokio::process::Child,
}

impl Drop for Driver {
    fn drop(&mut self) {
        if let Some(pid) = self._driver.id() {
            end_group(pid);
        }
        // Then `home` is removed, with what the session left in its
        // temporary directory.
    }
}

/// `program` under a virtual display for the user whose home is `home`, in
/// its own process group, so the driver, the app and whatever the app opens
/// (the sign-in page in a browser) end with it. Its temporary directory is in
/// that home: an AppImage run with APPIMAGE_EXTRACT_AND_RUN unpacks itself,
/// about 260 MB, under TMPDIR and removes that only when it exits on its own;
/// the tests stop it, and each run left one in /tmp until the build box's
/// disk was full (#203).
fn session(home: &Path, program: &Path) -> tokio::process::Command {
    let tmp = home.join("tmp");
    std::fs::create_dir_all(&tmp).unwrap();
    let mut c = tokio::process::Command::new("xvfb-run");
    c.args(["-a", "-s", "-screen 0 1280x900x24"])
        .arg(program)
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("TMPDIR", &tmp)
        .env("DBUS_SESSION_BUS_ADDRESS", "unix:path=/nonexistent")
        .env("NO_AT_BRIDGE", "1")
        .stdout(Stdio::null())
        .process_group(0)
        .kill_on_drop(true);
    c
}

/// Whether a process of group `group`, other than one that has exited and
/// waits for its parent, still runs.
fn group_runs(group: u32) -> bool {
    let group = group.to_string();
    std::fs::read_dir("/proc").unwrap().filter_map(|e| e.ok()?.file_name().to_str()?.parse::<u32>().ok()).any(|pid| {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap_or_default();
        // After the parenthesised name: state, parent, process group.
        let f: Vec<&str> = stat.rsplit(')').next().unwrap_or_default().split_whitespace().take(3).collect();
        f.len() == 3 && f[0] != "Z" && f[2] == group
    })
}

/// Ends process group `root` and waits until none of it runs, so nothing
/// still writes in the session's home when the home is removed.
fn end_group(root: u32) {
    for (signal, wait) in [("-TERM", 10), ("-KILL", 5)] {
        let _ = std::process::Command::new("kill").args([signal, "--", &format!("-{root}")]).status();
        let end = std::time::Instant::now() + std::time::Duration::from_secs(wait);
        while group_runs(root) && std::time::Instant::now() < end {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        if !group_runs(root) {
            return;
        }
    }
}

impl Driver {
    async fn start() -> Driver {
        Driver::start_with(&[]).await
    }

    /// With more environment for the app it launches.
    async fn start_with(envs: &[(&str, &str)]) -> Driver {
        let native = std::env::var("MAGNUS_WEBKIT_DRIVER").map(PathBuf::from).ok().or_else(|| which("WebKitWebDriver")).expect("WebKitWebDriver (set MAGNUS_WEBKIT_DRIVER)");
        let driver = which("tauri-driver").or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cargo/bin/tauri-driver"))).expect("tauri-driver");
        let home = tempfile::tempdir().unwrap();
        let (config, downloads) = (home.path().join(".config"), home.path().join("Downloads"));
        std::fs::create_dir_all(&config).unwrap();
        std::fs::create_dir_all(&downloads).unwrap();
        std::fs::write(config.join("user-dirs.dirs"), format!("XDG_DOWNLOAD_DIR=\"{}\"\n", downloads.display())).unwrap();
        // The computer's default browser, as a person sets one: the real
        // Chromium, signing in at the provider as whoever the test names in
        // the sign-in-as file (tests/scratch/sign-in-browser.mjs).
        let apps = home.path().join(".local/share/applications");
        std::fs::create_dir_all(&apps).unwrap();
        let node = which("node").expect("node");
        let browser = workspace_root().join("tests/scratch/sign-in-browser.mjs");
        std::fs::write(
            apps.join("magnus-test-browser.desktop"),
            format!(
                "[Desktop Entry]\nType=Application\nName=Test browser\nExec={} {} %u\nMimeType=x-scheme-handler/http;x-scheme-handler/https;\n",
                node.display(),
                browser.display()
            ),
        )
        .unwrap();
        std::fs::write(
            config.join("mimeapps.list"),
            "[Default Applications]\nx-scheme-handler/http=magnus-test-browser.desktop\nx-scheme-handler/https=magnus-test-browser.desktop\n",
        )
        .unwrap();
        // tauri-driver cannot take port 0 (it must tell WebKitWebDriver its
        // port, and reports neither), so a start counts only when both ports
        // are held by this driver's own processes; otherwise pick again, and
        // say why if no try does.
        let mut tries = Vec::new();
        for _ in 0..3 {
            let (port, native_port) = (listener_port(), listener_port());
            let log = home.path().join("tauri-driver.log");
            let mut child = session(home.path(), &driver)
                .args(["--port", &port.to_string(), "--native-port", &native_port.to_string(), "--native-driver"])
                .arg(&native)
                .env("MAGNUS_SIGN_IN_AS", home.path().join("sign-in-as"))
                .env("MAGNUS_CHROMIUM", chromium())
                .envs(envs.iter().copied())
                .stderr(std::fs::File::create(&log).unwrap())
                .spawn()
                .expect("start tauri-driver");
            let root = child.id().expect("tauri-driver's pid");
            let start = std::time::Instant::now();
            let lost = loop {
                if let Ok(Some(status)) = child.try_wait() {
                    break Some(format!("it exited ({status}): {}", std::fs::read_to_string(&log).unwrap_or_default().trim()));
                }
                match (held_by(port, root), held_by(native_port, root)) {
                    (Some(true), Some(true)) => break None,
                    (Some(false), _) | (_, Some(false)) => break Some(format!("another process listens on {port} or {native_port}: {} {}", listener(port), listener(native_port))),
                    _ => {}
                }
                assert!(start.elapsed() < PATIENCE, "tauri-driver did not listen on {port} and {native_port}");
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            };
            if let Some(why) = lost {
                // End its process group, as Drop does, and pick again.
                end_group(root);
                tries.push(why);
                continue;
            }
            let wd = format!("http://127.0.0.1:{port}");
            eventually("tauri-driver", PATIENCE, || {
                let wd = wd.clone();
                async move { reqwest::get(format!("{wd}/status")).await.ok().map(|_| ()) }
            })
            .await;
            return Driver { wd, home, _driver: child };
        }
        panic!("tauri-driver did not start in three tries: {tries:#?}");
    }

    fn config(&self) -> PathBuf {
        self.home.path().join(".config")
    }

    fn downloads(&self) -> PathBuf {
        self.home.path().join("Downloads")
    }

    /// The X display xvfb-run gave the driver and the app it launches, and
    /// the authority file that opens it.
    fn display(&self) -> (String, String) {
        let root = self._driver.id().expect("the driver is running");
        let parent = |pid: u32| std::fs::read_to_string(format!("/proc/{pid}/stat")).ok().and_then(|s| s.rsplit(')').next()?.split_whitespace().nth(1)?.parse::<u32>().ok());
        let under = |mut pid: u32| loop {
            if pid == root {
                return true;
            }
            match parent(pid) {
                Some(p) if p > 1 => pid = p,
                _ => return false,
            }
        };
        std::fs::read_dir("/proc")
            .unwrap()
            .filter_map(|e| e.ok()?.file_name().to_str()?.parse::<u32>().ok())
            .filter(|pid| *pid != root && under(*pid))
            .find_map(|pid| {
                let env = std::fs::read(format!("/proc/{pid}/environ")).ok()?;
                let var = |k: &[u8]| env.split(|b| *b == 0).find_map(|v| v.strip_prefix(k).map(|d| String::from_utf8_lossy(d).into_owned()));
                Some((var(b"DISPLAY=")?, var(b"XAUTHORITY=").unwrap_or_default()))
            })
            .expect("no display under the driver")
    }

    /// Run an installed-app script against the appliance at `url`; returns
    /// whether it passed and what it printed.
    async fn script(&self, name: &str, url: &str, fingerprint: &str, envs: &[(&str, &str)]) -> (bool, String) {
        let out = tokio::process::Command::new("node")
            .arg(workspace_root().join("desktop/tests").join(name))
            .env("MAGNUS_WEBDRIVER", &self.wd)
            .env("MAGNUS_APP", app_bin())
            .env("MAGNUS_URL", url)
            .env("MAGNUS_FINGERPRINT", fingerprint)
            .env("MAGNUS_PERSON", PERSON)
            .env("MAGNUS_PROVIDER", IDP_LABEL)
            .env("MAGNUS_SIGN_IN_AS", self.home.path().join("sign-in-as"))
            .env("MAGNUS_PASSWORDS", json!(REALM_PEOPLE.iter().map(|p| (p.to_string(), password_of(p))).collect::<std::collections::BTreeMap<_, _>>()).to_string())
            .envs(envs.iter().copied())
            .envs(shots())
            .output()
            .await
            .unwrap();
        (out.status.success(), format!("{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)))
    }
}

/// The people who can sign in at the test Keycloak.
const REALM_PEOPLE: [&str; 2] = [PERSON, "carol"];

/// A Keycloak realm with the test's people, which the appliance signs in through.
async fn keycloak(app: &mut magnus_invariants::Appliance) -> Keycloak {
    let kc = Keycloak::start().await;
    for p in REALM_PEOPLE {
        kc.add_person(p).await;
    }
    kc.configure(app).await;
    kc
}

/// An appliance serving HTTPS with sign-in through Keycloak.
async fn appliance() -> (Flow, Keycloak, String, String) {
    let mut f = Flow::start().await;
    let kc = keycloak(&mut f.app).await;
    let (url, fingerprint) = f.with_tls().await;
    (f, kc, url, fingerprint)
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs the built desktop app, tauri-driver, WebKitWebDriver and xvfb-run"]
async fn the_linux_app_pins_signs_in_through_the_provider_approves_and_shows_evidence() {
    let (f, _idp, url, fingerprint) = appliance().await;
    let d = Driver::start().await;
    let key = canary("anthropic-key");
    let (ok, log) = d.script("app.mjs", &url, &fingerprint, &[("MAGNUS_API_KEY", &key), ("MAGNUS_TASK", TASK)]).await;
    assert!(ok, "the app flow failed:\n{log}\n{}", f.report().await);
    f.check_outcome(&key).await;

    // The app kept the session in a file only this user can read, and the
    // pinned fingerprint beside the address; nothing else secret.
    let dir = d.config().join("magnus");
    let session = std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok()).find(|e| e.file_name().to_string_lossy().starts_with("session-")).expect("a session file");
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(session.metadata().unwrap().permissions().mode() & 0o777, 0o600);
    let settings = std::fs::read_to_string(dir.join("app.json")).unwrap();
    assert!(settings.contains(&fingerprint), "{settings}");
    assert!(!settings.contains("mgs_") && !settings.contains(&key), "a secret reached the settings: {settings}");
}

/// Found on 0.4.0 (2026-10-29): the library listed files the app could not
/// save, because the shell fetched bytes only from task outputs.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs the built desktop app, tauri-driver, WebKitWebDriver and xvfb-run"]
async fn the_installed_app_downloads_a_file_from_the_library() {
    let (f, _idp, url, fingerprint) = appliance().await;
    let (name, content) = ("kitchen-notes.txt", format!("Tiles from the corner shop. {}\n", canary("library")));
    use base64::Engine;
    let (st, v) =
        f.app.api(PERSON).post("/v1/files", json!({ "name": name, "media_type": "text/plain", "data_b64": base64::engine::general_purpose::STANDARD.encode(&content) })).await;
    assert_eq!(st.as_u16(), 201, "{v}");
    let d = Driver::start().await;
    let (ok, log) = d.script("app-library.mjs", &url, &fingerprint, &[("MAGNUS_FILE", name), ("MAGNUS_FILE_TEXT", &content)]).await;
    assert!(ok, "the library download failed:\n{log}");
    assert_eq!(std::fs::read_to_string(Path::new(&d.downloads()).join(name)).unwrap(), content);
}

/// Settings in the installed app: every section is there for the owner and
/// does what it says, checked again on the appliance afterwards.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs the built desktop app, tauri-driver, WebKitWebDriver and xvfb-run"]
async fn settings_holds_every_section_and_each_works_in_the_installed_app() {
    let (f, _idp, url, fingerprint) = appliance().await;
    // The owner is signed in elsewhere too (their backup, with a code from
    // the appliance's machine): a second device to sign out.
    let r = reqwest::Client::new()
        .post(format!("{}/v1/sign-in", f.app.base))
        .header("x-magnus-app", "0.5.0")
        .json(&json!({ "person": PERSON, "password": PASSWORD, "machine_code": magnus_scratch::sign_in_code(&f.app), "client": "app" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let elsewhere = r.json::<serde_json::Value>().await.unwrap()["token"].as_str().unwrap().to_string();
    let d = Driver::start().await;
    let (ok, log) = d.script("app-settings.mjs", &url, &fingerprint, &[("MAGNUS_OTHER", "carol")]).await;
    assert!(ok, "Settings failed:\n{log}");

    let mut other = f.app.api(PERSON);
    other.token = elsewhere;
    assert_eq!(other.get("/v1/me").await.0, 401, "the device signed out in Settings still works");
    let alice = f.app.api(PERSON);
    assert_eq!(f.app.api("carol").get("/v1/me").await.1["roles"], json!(["developer"]));
    let (_, p) = alice.get("/v1/preferences").await;
    assert_eq!((p["notify_batched"].clone(), p["feedback_consent"].clone()), (json!(true), json!(true)), "{p}");
    let (_, v) = alice.get("/v1/deletion-requests").await;
    assert_eq!(v["requests"][0]["what"], "my chats", "{v}");
    let (_, b) = alice.get("/v1/backups").await;
    assert!(b["latest"]["restored"] == true, "{b}");
}

/// Every message action in the installed app, each checked again on the
/// appliance: copy, delete, edit and resend, retry, save to the library,
/// export as a file, make a task, share with a colleague, branch.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs the built desktop app, tauri-driver, WebKitWebDriver and xvfb-run"]
async fn every_message_action_works_in_the_installed_app() {
    use magnus_invariants::{complete, plan, script, FakeWeb, Reply};
    let s = script(|c| async move {
        if c.body["system"].to_string().contains("in a chat") {
            let first = c.messages().first().cloned().unwrap_or_default();
            let said = first["content"].as_array().and_then(|b| b.last()).and_then(|b| b["text"].as_str()).unwrap_or_default().to_string();
            return Reply::Text(format!("Answer to: {said}"));
        }
        match c.turn() {
            0 => plan("do the work"),
            _ => complete("done"),
        }
    });
    let mut f = Flow::start_with(FakeWeb::start(&[]).await, s).await;
    let _kc = keycloak(&mut f.app).await;
    let (url, fingerprint) = f.with_tls().await;
    let d = Driver::start().await;
    let (display, xauth) = d.display();
    let (ok, log) = d.script("app-chat-actions.mjs", &url, &fingerprint, &[("MAGNUS_OTHER_NAME", "carol"), ("MAGNUS_DISPLAY", &display), ("MAGNUS_XAUTHORITY", &xauth)]).await;
    assert!(ok, "a message action failed:\n{log}");

    let alice = f.app.api(PERSON);
    let (_, threads) = alice.get("/v1/chat").await;
    assert_eq!(threads["threads"].as_array().unwrap().len(), 2, "no branch: {threads}");
    let shared = f.app.api("carol").inbox().await.into_iter().find(|i| i["kind"] == "shared").expect("nothing reached carol");
    assert_eq!(shared["details"]["text"], "Answer to: Second, edited");
    let (_, files) = alice.get("/v1/files").await;
    assert!(files["files"].as_array().unwrap().iter().any(|f| f["media_type"] == "text/markdown" && f["origin"]["kind"] == "chat"), "{files}");
    let exported = std::fs::read_dir(d.downloads()).unwrap().filter_map(|e| e.ok()).find(|e| e.file_name().to_string_lossy().ends_with(".md")).expect("no exported file");
    assert!(std::fs::read_to_string(exported.path()).unwrap().contains("Answer to: Second, edited"));
    assert!(alice.tasks().await["tasks"].as_array().unwrap().iter().any(|t| t["text"] == "Second, edited"), "no task from the message");
}

/// The trash in the installed app (deletion plan, step 2): a conversation
/// deleted, restored, deleted again and the trash emptied; then its words
/// are in no view and not in the content store.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs the built desktop app, tauri-driver, WebKitWebDriver and xvfb-run"]
async fn a_conversation_goes_to_the_trash_comes_back_and_is_removed_in_the_installed_app() {
    use magnus_invariants::{complete, plan, script, FakeWeb, Reply};
    let s = script(|c| async move {
        if c.body["system"].to_string().contains("in a chat") {
            let first = c.messages().first().cloned().unwrap_or_default();
            let said = first["content"].as_array().and_then(|b| b.last()).and_then(|b| b["text"].as_str()).unwrap_or_default().to_string();
            return Reply::Text(format!("Answer to: {said}"));
        }
        match c.turn() {
            0 => plan("do the work"),
            _ => complete("done"),
        }
    });
    let mut f = Flow::start_with(FakeWeb::start(&[]).await, s).await;
    let _kc = keycloak(&mut f.app).await;
    let (url, fingerprint) = f.with_tls().await;
    let d = Driver::start().await;
    let (ok, log) = d.script("app-trash.mjs", &url, &fingerprint, &[]).await;
    assert!(ok, "the trash failed in the app:\n{log}");

    let alice = f.app.api(PERSON);
    assert!(!alice.get("/v1/chat").await.1.to_string().contains("walruses"), "the conversation is still listed");
    assert!(alice.get("/v1/trash").await.1["items"].as_array().unwrap().is_empty());
    let rows = f.app.db.admin().await.query("SELECT body::text FROM public.content", &[]).await.unwrap();
    assert!(!rows.iter().any(|r| r.get::<_, String>(0).contains("walruses")), "the conversation's words are still in the content store");
}

/// Requested by the owner (2026-10-29, U8): a data question gets its text
/// streamed and then a chart and a table; a plain question gets text only;
/// and composed content makes no network request, even markup forced into
/// its frame.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs the built desktop app, tauri-driver, WebKitWebDriver and xvfb-run"]
async fn composed_answers_stream_text_first_and_reach_no_network_in_the_installed_app() {
    use magnus_invariants::{complete, plan, script, tool, tools, FakeWeb, Reply};
    const QUESTION: &str = "How did revenue go by quarter?";
    const ANSWER: &str = "Revenue rose every quarter, to 41.7M in Q3.";
    const HOSTILE: &str = "Show me the office report";
    let beacon = FakeWeb::start(&[("/beacon", "seen")]).await;
    let b = beacon.url("/beacon");
    let hostile = json!([
        { "type": "table", "title": format!("url({b}/title)"), "columns": ["<img src=x>"], "rows": [[format!("<img src=\"{b}/table\">")]] },
        { "type": "code", "code": format!("<script>fetch(\"{b}/code\")</script>") },
        { "type": "map", "places": [{ "name": format!("<link rel=stylesheet href=\"{b}/map\">"), "lat": 51.5, "lon": -0.12 }, { "name": "Paris", "lat": 48.86, "lon": 2.35 }] },
        { "type": "timeline", "events": [{ "when": "2025", "label": format!("<iframe src=\"{b}/timeline\"></iframe>") }] },
        { "type": "comparison", "options": [format!("<style>@import url({b}/cmp)</style>"), "B"], "aspects": [{ "name": "<button>Approve</button>", "values": [1, 2], "best": 0 }] }
    ]);
    let s = script(move |c| {
        let hostile = hostile.clone();
        async move {
            if c.body["system"].to_string().contains("in a chat") {
                let first = c.messages().first().cloned().unwrap_or_default();
                let said = first["content"].as_array().and_then(|b| b.last()).and_then(|b| b["text"].as_str()).unwrap_or_default().to_string();
                return match said.as_str() {
                    QUESTION => tools(vec![
                        magnus_scratch::say(ANSWER),
                        (
                            "show",
                            json!({ "components": [
                            { "type": "bar_chart", "title": "Revenue by quarter", "labels": ["Q1", "Q2", "Q3"], "values": [35.1, 38.2, 41.7], "unit": "M" },
                            { "type": "table", "columns": ["Quarter", "Revenue (M)"], "rows": [["Q1", 35.1], ["Q2", 38.2], ["Q3", 41.7]] }
                        ] }),
                        ),
                    ]),
                    HOSTILE => tools(vec![magnus_scratch::say("Here is what you asked for."), ("show", json!({ "components": hostile }))]),
                    text if text.starts_with("Show local chart ") => {
                        Reply::Text(format!("![Local chart]({})\n\n[Supplier reference](https://example.com/reference)", text.trim_start_matches("Show local chart ")))
                    }
                    _ => Reply::Text(format!("Answer to: {said}")),
                };
            }
            match c.turn() {
                0 => plan("do the work"),
                1 => tool("task_fail", json!({ "reason": "not this test" })),
                _ => complete("done"),
            }
        }
    });
    let mut f = Flow::start_with(FakeWeb::start(&[]).await, s).await;
    let _kc = keycloak(&mut f.app).await;
    let (url, fingerprint) = f.with_tls().await;
    let d = Driver::start().await;
    let (display, xauth) = d.display();
    let (ok, log) = d
        .script(
            "app-composed.mjs",
            &url,
            &fingerprint,
            &[
                ("MAGNUS_DATA_QUESTION", QUESTION),
                ("MAGNUS_DATA_ANSWER", ANSWER),
                ("MAGNUS_HOSTILE_QUESTION", HOSTILE),
                ("MAGNUS_BEACON", &b),
                ("MAGNUS_DISPLAY", &display),
                ("MAGNUS_XAUTHORITY", &xauth),
            ],
        )
        .await;
    assert!(ok, "composed answers failed:\n{log}");
    assert!(beacon.hits().is_empty(), "composed content reached the network: {:?}", beacon.hits());
    // The recorder records: a request the test makes itself is seen.
    reqwest::get(format!("{b}/control")).await.unwrap();
    assert_eq!(beacon.hits().len(), 1, "the beacon records nothing, so the check above proves nothing");
}

/// Requested by the owner (2026-10-29): Magnus reads aloud in its own voice,
/// made on the appliance, not the system's. A stand-in voice speaks each
/// sentence for a second, so the test hears how long the app played.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs the built desktop app, tauri-driver, WebKitWebDriver, xvfb-run and ffmpeg"]
async fn the_installed_app_reads_aloud_in_the_appliances_voice() {
    use magnus_invariants::{complete, plan, script, FakeWeb, Reply};
    use std::os::unix::fs::PermissionsExt;
    const SCRIPT: &str = "Revenue went up every quarter. The third was the best, at forty one point seven million. Nothing needs you today.";
    let s = script(|c| async move {
        let system = c.body["system"].to_string();
        if system.contains("to be heard rather than read") {
            return Reply::Text(SCRIPT.into());
        }
        if system.contains("in a chat") {
            return Reply::Text("Revenue rose every quarter, to 41.7M in Q3.".into());
        }
        match c.turn() {
            0 => plan("do the work"),
            _ => complete("done"),
        }
    });
    let mut f = Flow::start_with(FakeWeb::start(&[]).await, s).await;
    let dir = f.app.dir.path().to_path_buf();
    let (speak, log) = (dir.join("speak"), dir.join("spoken.log"));
    let seconds = 1;
    std::fs::write(&speak, format!("#!/bin/sh\n{{ cat; echo; }} >> {}\nexec ffmpeg -loglevel error -f lavfi -i anullsrc=r=24000:cl=mono -t {seconds} -f mp3 -\n", log.display()))
        .unwrap();
    std::fs::set_permissions(&speak, std::fs::Permissions::from_mode(0o755)).unwrap();
    magnus_scratch::reconfigure(&mut f.app, |c| {
        let mut r = toml::Table::new();
        r.insert("speak".into(), speak.display().to_string().into());
        for (k, v) in [("model", "kokoro.onnx"), ("voices", "voices.bin"), ("voice", "af_heart")] {
            r.insert(k.into(), v.into());
        }
        c.insert("read_aloud".into(), r.into());
    })
    .await;
    let _kc = keycloak(&mut f.app).await;
    let (url, fingerprint) = f.with_tls().await;
    let d = Driver::start().await;
    let sentences = SCRIPT.split(". ").count().to_string();
    let (ok, out) = d
        .script(
            "app-read-aloud.mjs",
            &url,
            &fingerprint,
            &[("MAGNUS_SPOKEN_LOG", &log.display().to_string()), ("MAGNUS_SENTENCES", &sentences), ("MAGNUS_SECONDS", &seconds.to_string())],
        )
        .await;
    assert!(ok, "read aloud failed:\n{out}");
    let read: Vec<String> = std::fs::read_to_string(&log).unwrap().lines().take(3).map(String::from).collect();
    assert_eq!(read, ["Revenue went up every quarter.", "The third was the best, at forty one point seven million.", "Nothing needs you today."]);
}

#[path = "desktop_app/flows.rs"]
mod flows;
