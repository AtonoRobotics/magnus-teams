//! Additional installed desktop app scenarios (multi-user, connectors, updates,
//! looks, channels and frame links), kept in a submodule for file size.

use super::*;

/// The sign-in link crosses into the shell, and so does the kept session:
/// signing out must leave nothing of the first person for the second.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs the built desktop app, tauri-driver, WebKitWebDriver and xvfb-run"]
async fn two_people_sign_in_on_one_installed_app_each_with_their_own_rights() {
    let (f, _idp, url, fingerprint) = appliance().await;
    let title = format!("Plan the offsite budget {}", canary("people"));
    f.app.api(PERSON).submit(&title).await;
    let d = Driver::start().await;
    let (ok, log) = d.script("app-people.mjs", &url, &fingerprint, &[("MAGNUS_OTHER", "carol"), ("MAGNUS_TASK_TITLE", &title)]).await;
    assert!(ok, "two people on one app failed:\n{log}");
}

/// Connectors live in Settings: the owner connects an app there, and the app
/// sign-in's code comes back to a loopback address the shell opens on this
/// computer (only the installed app can catch it). A tool whose definition
/// changes is blocked until the owner accepts the change on its connector.
/// Fails if the Connectors page or its connection flow disappears, or if
/// the app drops the server's name from the sign-in's answer (RFC 9207).
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs the built desktop app, tauri-driver, WebKitWebDriver and xvfb-run"]
async fn the_owner_connects_an_app_through_its_screen_and_keeps_its_new_tools() {
    let (f, _idp, url, fingerprint) = appliance().await;
    // It names itself in its answer, as Google does: the app must carry
    // that back (2026-11-02: every Google sign-in from the app was refused
    // because the app dropped it).
    let cal = FakeOAuthApp::naming_itself(3600).await;
    // At first it offers only list_events; it adds tools that change later.
    cal.state.lock().unwrap().retired = vec!["create_event".into(), "delete_all".into()];
    let alice = f.app.api(PERSON);
    // The admin adds a server by its address in the admin console, never in
    // the app (connectors spec rev 46): here, through the same API.
    let (st, v) = alice.post("/v1/connectors", json!({ "url": cal.url, "audience": "public", "label": "private+untrusted" })).await;
    assert!(st.is_success(), "{v}");
    let d = Driver::start().await;
    let envs = [("MAGNUS_APP_URL", cal.url.as_str())];
    let script = d.script("app-apps.mjs", &url, &fingerprint, &envs);
    // Connected through its screen, the app's tools are classed by its own
    // hints with no review (spec rev 140); a task uses them, and the app then
    // adds two that change, which put one card before the owner. When the
    // script fails before connecting, its log says where: this side waits
    // for it rather than panicking first.
    let change = async {
        let end = std::time::Instant::now() + std::time::Duration::from_secs(120);
        loop {
            let (_, l) = alice.get("/v1/connectors").await;
            let read = l["connectors"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|c| c["name"] == "calendar")
                .and_then(|c| c["tools"].as_array().cloned())
                .is_some_and(|t| t.iter().any(|x| x["name"] == "list_events" && x["risk"] == "read"));
            if read {
                break;
            }
            if std::time::Instant::now() > end {
                return None;
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
        let first = alice.submit("Check my calendar.").await;
        alice.wait_settled(&first).await;
        cal.state.lock().unwrap().retired.clear();
        // The kernel keeps an app's tool list for two seconds.
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        alice.submit("Check my calendar again.").await;
        Some(eventually("the app's card in the inbox", std::time::Duration::from_secs(60), || async { alice.inbox().await.into_iter().find(|i| i["kind"] == "app_tools") }).await)
    };
    let ((ok, log), item) = tokio::join!(script, change);
    assert!(ok, "connecting an app failed:\n{log}");
    let item = item.unwrap_or_else(|| panic!("the app was never connected through its screen:\n{log}"));
    let item = alice.inbox().await.into_iter().find(|i| i["id"] == item["id"]).unwrap();
    assert_eq!(item["resolution"]["decision"], "keep", "the new tools were not kept on the connector: {item}");
    let (_, list) = alice.get("/v1/connectors").await;
    let c = &list["connectors"][0];
    assert_eq!((c["name"].as_str(), c["signed_in"].as_bool()), (Some("calendar"), Some(true)), "{list}");
    let risk = |t: &str| c["tools"].as_array().unwrap().iter().find(|x| x["name"] == t).unwrap()["risk"].clone();
    assert_eq!((risk("list_events"), risk("create_event"), risk("delete_all")), (json!("read"), json!("reversible_write"), json!("irreversible")), "{list}");
}

/// The processes of the app at `path`: an AppImage's own runtime, and what
/// it runs (which carries the AppImage's path in `APPIMAGE`).
fn processes_of(path: &Path) -> Vec<i32> {
    let mark = format!("APPIMAGE={}", path.display());
    std::fs::read_dir("/proc")
        .unwrap()
        .filter_map(|e| e.ok()?.file_name().to_str()?.parse::<i32>().ok())
        .filter(|pid| {
            let cmd = std::fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
            let env = std::fs::read(format!("/proc/{pid}/environ")).unwrap_or_default();
            cmd.split(|b| *b == 0).next() == Some(path.as_os_str().as_encoded_bytes()) || env.split(|b| *b == 0).any(|v| v == mark.as_bytes())
        })
        .collect()
}

/// The update tests run the app as an AppImage that unpacks itself
/// (APPIMAGE_EXTRACT_AND_RUN) into `appimage_extracted_<md5>` under TMPDIR,
/// about 260 MB, and stop it; stopped, it never removed that, and 296 runs
/// left 75 GB in /tmp (#203). Started and ended as every test's session is,
/// it leaves nothing.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs an AppImage build (MAGNUS_UPDATE_BUILDS) and xvfb-run"]
async fn an_appimage_a_test_stops_leaves_no_extraction_behind() {
    let builds = PathBuf::from(std::env::var("MAGNUS_UPDATE_BUILDS").expect("MAGNUS_UPDATE_BUILDS: old.AppImage"));
    let app = builds.join("old.AppImage");
    let sum = std::process::Command::new("md5sum").arg(&app).output().expect("md5sum");
    assert!(sum.status.success(), "{}", String::from_utf8_lossy(&sum.stderr));
    let name = format!("appimage_extracted_{}", String::from_utf8_lossy(&sum.stdout).split_whitespace().next().unwrap());
    // Where it unpacks without a TMPDIR of the session's own.
    let shared = std::env::temp_dir().join(&name);
    let modified = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    let before = modified(&shared);
    let unpacked_there = move || modified(&shared).is_some_and(|m| Some(m) != before);

    let home = tempfile::tempdir().unwrap();
    let mine = home.path().join("tmp").join(&name);
    let child = session(home.path(), &app).env("APPIMAGE_EXTRACT_AND_RUN", "1").stderr(Stdio::null()).spawn().expect("start the AppImage");
    eventually("the AppImage unpacked and running", PATIENCE, || {
        let found = mine.join("AppRun").exists() || unpacked_there();
        async move { found.then_some(()) }
    })
    .await;
    drop(Driver { wd: String::new(), home, _driver: child });
    assert!(!unpacked_there(), "the stopped AppImage left {name} in {}", std::env::temp_dir().display());
    assert!(!mine.exists(), "the session's temporary directory outlived it: {}", mine.display());
}

/// Requested with the update (owner, 2026-10-29): an update downloads,
/// installs on restart, and a task running on the appliance is unaffected.
/// The app restarts outside the driver's session; the test stops that copy
/// before it shows anything, and a new session opens the updated app.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs two AppImage builds (MAGNUS_UPDATE_BUILDS), tauri-driver, WebKitWebDriver and xvfb-run"]
async fn the_installed_app_updates_itself_from_the_appliance_while_a_task_waits() {
    let builds = PathBuf::from(std::env::var("MAGNUS_UPDATE_BUILDS").expect("MAGNUS_UPDATE_BUILDS: old.AppImage, new.AppImage and its .sig, new.version"));
    let fresh = std::fs::read_to_string(builds.join("new.version")).unwrap().trim().to_string();
    let notes = format!("Tasks now show their sources. {}", canary("notes"));

    // The appliance serves the new release to the apps.
    let mut f = Flow::start().await;
    let updates = f.app.dir.path().join("updates");
    let path = f.app.dir.path().join("magnus.toml");
    let mut cfg: toml::Table = std::fs::read_to_string(&path).unwrap().parse().unwrap();
    cfg.insert("updates".into(), toml::Value::Table(toml::toml! { dir = (updates.display().to_string()) }));
    std::fs::write(&path, toml::to_string(&cfg).unwrap()).unwrap();
    let _kc = keycloak(&mut f.app).await;
    let (url, fingerprint) = f.with_tls().await;
    let desktop = updates.join("desktop");
    std::fs::create_dir_all(&desktop).unwrap();
    let file = format!("Magnus_{fresh}_amd64.AppImage");
    std::fs::copy(builds.join("new.AppImage"), desktop.join(&file)).unwrap();
    let signature = std::fs::read_to_string(builds.join("new.AppImage.sig")).unwrap();
    let manifest = json!({ "version": fresh, "notes": notes, "platforms": { "linux-x86_64": { "file": file, "signature": signature } } });
    std::fs::write(desktop.join("latest.json"), manifest.to_string()).unwrap();

    // A task waits on the appliance for the person's approval.
    let alice = f.app.api(PERSON);
    let task = alice.submit(TASK).await;
    alice.wait_item(&task, "approval").await;
    let before = f.model.requests().len();

    // The installed app, an AppImage the person can write.
    let d = Driver::start_with(&[("APPIMAGE_EXTRACT_AND_RUN", "1")]).await;
    let app = d.home.path().join("Magnus.AppImage");
    std::fs::copy(builds.join("old.AppImage"), &app).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&app, std::fs::Permissions::from_mode(0o755)).unwrap();
    let app_s = app.display().to_string();
    let envs = [("MAGNUS_APP", app_s.as_str()), ("MAGNUS_NEW", fresh.as_str())];

    let (ok, log) = d.script("app-update.mjs", &url, &fingerprint, &[envs[0], envs[1], ("MAGNUS_PHASE", "before")]).await;
    assert!(ok, "the update was not offered:\n{log}");
    let new = std::fs::read(builds.join("new.AppImage")).unwrap();
    eventually("the AppImage is replaced by the new release", PATIENCE, || {
        let (app, new) = (app.clone(), new.clone());
        async move { (std::fs::read(&app).ok()? == new).then_some(()) }
    })
    .await;
    let whats_new = d.config().join("app.magnus.teams/whats-new.json");
    let kept: serde_json::Value = serde_json::from_slice(&std::fs::read(&whats_new).expect("what's new was not kept for after the restart")).unwrap();
    assert_eq!((kept["version"].as_str(), kept["notes"].as_str()), (Some(fresh.as_str()), Some(notes.as_str())), "{kept}");
    // The copy the app restarted, outside the session.
    let end = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while std::time::Instant::now() < end {
        for pid in processes_of(&app) {
            let _ = std::process::Command::new("kill").arg(pid.to_string()).status();
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert!(whats_new.exists(), "the restarted copy showed what's new before it was stopped; nothing left to check");

    let (ok, log) = d.script("app-update.mjs", &url, &fingerprint, &[envs[0], envs[1], ("MAGNUS_PHASE", "after"), ("MAGNUS_NOTES", &notes)]).await;
    assert!(ok, "the updated app failed:\n{log}\n{}", f.report().await);
    assert!(!whats_new.exists(), "what's new was not shown once");

    // The task waited through the update untouched, then finished.
    let done = alice.wait_status(&task, &["completed"]).await;
    assert_eq!(done["status"], "completed", "{done}");
    assert!(f.model.requests().len() > before, "the task did not go on after its approval");
    let db = f.app.db.admin().await;
    let stops: Vec<String> = db
        .query("SELECT body->>'reason' FROM public.events WHERE kind = 'suspended' AND run_id = (SELECT run_id FROM public.events WHERE kind = 'task_created' ORDER BY seq DESC LIMIT 1) ORDER BY seq", &[])
        .await
        .unwrap()
        .iter()
        .map(|r| r.get(0))
        .collect();
    assert_eq!(stops, ["approval"], "the task was stopped other than for the person's approval");
}

/// One workspace (owner, 2026-10-30): research started in a conversation,
/// approved there, its result read and its file downloaded there, and a
/// schedule created and its status checked there, with no navigation. The
/// decisions are the same cards under Needs you. Checked again on the
/// appliance afterwards.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs the built desktop app, tauri-driver, WebKitWebDriver and xvfb-run"]
async fn research_approval_result_file_and_schedule_happen_in_one_conversation_in_the_installed_app() {
    use magnus_invariants::{script, steps, tool, FakeWeb, Reply};
    const RESEARCH: &str = "Look into Q3 revenue and send me a one-line summary file.";
    const SCHEDULE: &str = "Check the finance page every morning at nine.";
    let web = FakeWeb::start(&[("/finance/q3", "<p>Q3 revenue: 41.7M. Q2 revenue: 38.2M.</p>")]).await;
    let task = steps(replies(web.url("/finance/q3")));
    let s = script(move |c| {
        let task = task.clone();
        async move {
            if !c.body["system"].to_string().contains("in a chat") {
                return task(c).await;
            }
            let first = c.messages().first().cloned().unwrap_or_default();
            let said = first["content"].as_array().and_then(|b| b.last()).and_then(|b| b["text"].as_str()).unwrap_or_default().to_string();
            match (said.as_str(), c.turn()) {
                (RESEARCH, 0) => tool("create_task", json!({ "title": "Q3 revenue summary", "goal": TASK })),
                (SCHEDULE, 0) => tool("create_schedule", json!({ "title": "Morning finance check", "goal": "Read the finance page and note Q3 revenue.", "daily_at": "09:00" })),
                _ => Reply::Text("Started; it shows here as it goes.".into()),
            }
        }
    });
    let mut f = Flow::start_with(web, s).await;
    let _kc = keycloak(&mut f.app).await;
    let (url, fingerprint) = f.with_tls().await;
    let d = Driver::start().await;
    let (ok, log) = d.script("app-workspace.mjs", &url, &fingerprint, &[("MAGNUS_RESEARCH", RESEARCH), ("MAGNUS_SCHEDULE", SCHEDULE)]).await;
    assert!(ok, "the workspace flow failed:\n{log}\n{}", f.report().await);
    // What each flow took, against its ease-of-use budget.
    println!("{log}");

    let alice = f.app.api(PERSON);
    let tasks = alice.tasks().await;
    let t = &tasks["tasks"][0];
    assert_eq!(t["status"], "completed", "{tasks}");
    assert!(t["thread"].is_string(), "the task did not come from the conversation: {t}");
    let decided: Vec<_> = alice.inbox().await.into_iter().filter(|i| i["kind"] == "approval").collect();
    assert_eq!(decided.len(), 1, "{decided:?}");
    assert!(decided.iter().all(|i| i["resolution"]["by"] == format!("person:{PERSON}") && i["thread"] == t["thread"]), "{decided:?}");
    let (_, sc) = alice.get("/v1/schedules").await;
    assert_eq!(sc["schedules"][0]["daily_at"], "09:00", "{sc}");
    let saved = std::fs::read_to_string(d.downloads().join("summary.txt")).expect("the file was not downloaded");
    assert!(saved.contains("41.7M"), "{saved}");
}

/// The two looks: light and dark, chosen in Settings > Appearance; every
/// surface solid, every text on screen at 4.5:1 or more against what is
/// under it;
/// the accent and the motion setting take effect, and the look is kept on
/// the device.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs the built desktop app, tauri-driver, WebKitWebDriver and xvfb-run"]
async fn each_look_keeps_text_readable_and_every_surface_solid_in_the_installed_app() {
    use magnus_invariants::{complete, plan, script, FakeWeb, Reply};
    let s = script(|c| async move {
        if c.body["system"].to_string().contains("in a chat") {
            return Reply::Text("Hello. Ask me anything, or give me work to do.".into());
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
    let (ok, log) = d.script("app-looks.mjs", &url, &fingerprint, &[]).await;
    assert!(ok, "a look failed:\n{log}");
}

/// Release channels (owner, 2026-10-31): the owner's app, a release (0.7.0
/// by default), asks with its session and gets development's build.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs two AppImage builds (MAGNUS_UPDATE_BUILDS), tauri-driver, WebKitWebDriver and xvfb-run"]
async fn the_owners_installed_app_follows_development() {
    let builds = PathBuf::from(std::env::var("MAGNUS_UPDATE_BUILDS").expect("MAGNUS_UPDATE_BUILDS: old.AppImage, new.AppImage and its .sig, new.version"));
    let fresh = std::fs::read_to_string(builds.join("new.version")).unwrap().trim().to_string();

    // The appliance has a build only on development.
    let mut f = Flow::start().await;
    let updates = f.app.dir.path().join("updates");
    let path = f.app.dir.path().join("magnus.toml");
    let mut cfg: toml::Table = std::fs::read_to_string(&path).unwrap().parse().unwrap();
    cfg.insert("updates".into(), toml::Value::Table(toml::toml! { dir = (updates.display().to_string()) }));
    std::fs::write(&path, toml::to_string(&cfg).unwrap()).unwrap();
    let _kc = keycloak(&mut f.app).await;
    let (url, fingerprint) = f.with_tls().await;
    let pr = updates.join("desktop/development");
    std::fs::create_dir_all(&pr).unwrap();
    let file = format!("Magnus_{fresh}_amd64.AppImage");
    std::fs::copy(builds.join("new.AppImage"), pr.join(&file)).unwrap();
    let signature = std::fs::read_to_string(builds.join("new.AppImage.sig")).unwrap();
    let notes = "Sources in every answer\n\nDevelopment build of pull request #7.";
    let manifest = json!({ "version": fresh, "notes": notes, "platforms": { "linux-x86_64": { "file": file, "signature": signature } } });
    std::fs::write(pr.join("latest.json"), manifest.to_string()).unwrap();

    let d = Driver::start_with(&[("APPIMAGE_EXTRACT_AND_RUN", "1")]).await;
    let app = d.home.path().join("Magnus.AppImage");
    std::fs::copy(builds.join("old.AppImage"), &app).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&app, std::fs::Permissions::from_mode(0o755)).unwrap();
    let (ok, log) = d.script("app-channels.mjs", &url, &fingerprint, &[("MAGNUS_APP", &app.display().to_string()), ("MAGNUS_NEW", &fresh)]).await;
    assert!(ok, "the owner's app did not follow development:\n{log}");
}

#[test]
fn the_update_script_waits_two_minutes_to_allow_the_task_that_waited() {
    let src = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../desktop/tests/app-update.mjs")).unwrap();
    assert!(src.contains("seen(\"Allowed.\", 120_000)"), "{src}");
}

/// The Library step reaches it through the sidebar's link, not its address:
/// setting the hash before the app's router is ready loses to its initial
/// route, and the step then waits 60 s for the card on the wrong page (run
/// 42059799939, #200: the kept FAILED screenshot shows New conversation,
/// not the Library). The address was for reaching the Library from
/// Settings, which has no sidebar; the app is added through the API now.
#[test]
fn the_apps_script_reaches_the_library_through_the_sidebar() {
    let src = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../desktop/tests/app-apps.mjs")).unwrap();
    assert!(src.contains("//nav//a[normalize-space()='Apps']"), "{src}");
    assert!(!src.contains("location.hash"), "hash navigation is back:\n{src}");
}

/// A click lands where the element was when the driver measured it. A card
/// sizes its outcome frame once the frame has loaded, which moves the
/// buttons below it; on a busy runner the update test's Accept was clicked
/// just before, landed on the frame, and nothing was accepted (runs
/// 41969612463 and 42105845733, #207). The driver clicks an element once no
/// outcome frame in its card is still loading and it has stopped moving. A
/// stand-in WebDriver answers as such a page: the card's frame loads 400 ms
/// after the button is found, and the click must come after.
#[test]
fn the_app_driver_clicks_a_button_once_its_card_has_stopped_moving() {
    let tests = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../desktop/tests");
    let driver = url::Url::from_file_path(tests.join("appdriver.mjs").canonicalize().expect("desktop/tests/appdriver.mjs")).unwrap();
    let check = format!(
        r#"
import("node:http").then(async ({{ createServer }}) => {{
  const E = "element-6066-11e4-a52e-4f735466cecf";
  let found = 0, sized = Infinity, clicked = 0;
  const server = createServer((req, res) => {{
    let body = "";
    req.on("data", c => body += c);
    req.on("end", () => {{
      const send = value => {{ res.setHeader("content-type", "application/json"); res.end(JSON.stringify({{ value }})); }};
      if (req.url === "/session") return send({{ sessionId: "s" }});
      if (req.url === "/session/s/elements") {{ found = Date.now(); sized = found + 400; return send([{{ [E]: "accept" }}]); }}
      // The page: until its frame has loaded, the frame is unsized and the button is higher up.
      if (req.url === "/session/s/execute/sync") return send(Date.now() < sized ? null : "0,700,80,40");
      if (req.url === "/session/s/element/accept/click") {{ clicked = Date.now(); return send(null); }}
      res.statusCode = 404; send({{ error: "unknown command", message: req.url }});
    }});
  }}).listen(0, "127.0.0.1", async () => {{
    const {{ open }} = await import("{driver}");
    const d = await open(`http://127.0.0.1:${{server.address().port}}`, "app");
    await d.click(d.button("Accept"));
    server.close();
    if (!clicked) throw new Error("no click");
    if (clicked < sized) throw new Error(`clicked ${{clicked - found}} ms after the button was found, before its card's frame loaded at 400 ms`);
    console.log("ok");
  }});
}});
"#
    );
    let out = std::process::Command::new("node").args(["--input-type=module", "-e", &check]).output().expect("run node");
    assert!(out.status.success(), "{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
}

/// Real native scrolling and click behavior; fixed responses are UI fixtures.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs the built desktop app, tauri-driver, WebKitWebDriver and xvfb-run"]
async fn completed_work_links_remain_operable_while_native_content_resizes() {
    use magnus_invariants::{plan, script, tool, FakeWeb, Reply};
    let model = script(|c| async move {
        if c.body["system"].to_string().contains("in a chat") {
            let first = c.messages().first().cloned().unwrap_or_default();
            let text = first["content"].as_array().and_then(|b| b.last()).and_then(|b| b["text"].as_str()).unwrap_or_default();
            if text == "Create anchor work" && c.turn() == 0 {
                return tool("create_task", json!({"title":"Anchor work","goal":"Show a readable result"}));
            }
            return Reply::Text("Recorded in the actual conversation.".into());
        }
        match c.turn() {
            0 => plan("Readable work"),
            _ => tool(
                "task_complete",
                json!({"summary":"Anchor work finished","deliverable":[{"type":"text","text":"Actual stored anchor deliverable"},{"type":"bar_chart","title":"Comparison","labels":["A","B"],"values":[1,2]}]}),
            ),
        }
    });
    let mut f = Flow::start_with(FakeWeb::start(&[]).await, model).await;
    let _kc = keycloak(&mut f.app).await;
    let (url, fingerprint) = f.with_tls().await;
    let d = Driver::start().await;
    let (ok, log) = d.script("app-continuity.mjs", &url, &fingerprint, &[]).await;
    assert!(ok, "Native work link failed:\n{log}");
}

/// Real native table scrolling and OS browser handoff, with a recorded website.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs the built desktop app, tauri-driver, WebKitWebDriver and xvfb-run"]
async fn frame_links_remain_aligned_and_open_the_actual_browser_after_table_scrolling() {
    let links = magnus_invariants::FakeWeb::start(&[("/first-link", "<h1>First supplier reference</h1>"), ("/last-link", "<h1>Last supplier reference</h1>")]).await;
    let (_f, _idp, url, fingerprint) = appliance().await;
    let d = Driver::start().await;
    let (first, last) = (links.url("/first-link"), links.url("/last-link"));
    let (ok, log) = d.script("app-frame-links.mjs", &url, &fingerprint, &[("MAGNUS_FIRST_LINK", &first), ("MAGNUS_LAST_LINK", &last)]).await;
    assert!(ok, "Native frame links failed:\n{log}");
    for _ in 0..300 {
        let hits = links.hits();
        if hits.iter().any(|p| p == "/first-link") && hits.iter().any(|p| p == "/last-link") {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    let hits = links.hits();
    assert!(hits.iter().any(|p| p == "/first-link") && hits.iter().any(|p| p == "/last-link"), "The real default browser did not visit both clicked links: {hits:?}");
}
