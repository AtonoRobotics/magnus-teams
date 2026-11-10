//! Release channels of the desktop app (DECISIONS.md, "Release channels"),
//! from publishing to serving: `release/publish.sh` puts a pull request's
//! build in development, master's in test and a tag in production, and
//! never moves a channel backwards, in version or in content (a build
//! without the work of the one before is refused); the appliance decides
//! which channel each app gets, never the app: the owner's apps
//! development, testers' test, everyone else's production; a pull request
//! closed without merging, or pushed again, stops no later build, nor does
//! one squash-merged, whose head master never holds.

use magnus_invariants::*;
use reqwest::StatusCode;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

fn release(script: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../release").join(script)
}

/// A build's two installers, as CI collects them, in `dist`.
fn build(dist: &Path, version: &str) {
    let _ = std::fs::remove_dir_all(dist);
    std::fs::create_dir_all(dist).unwrap();
    for f in [format!("Magnus_{version}_amd64.AppImage"), format!("Magnus_{version}_x64-setup.exe")] {
        std::fs::write(dist.join(f), format!("the {version} build")).unwrap();
    }
}

/// `release/publish.sh desktop` as CI runs it, in this repository's
/// checkout; its exit code.
fn publish(key: &Path, updates: &Path, tmp: &Path, version: &str, channel: &str) -> i32 {
    publish_from(key, updates, tmp, version, channel, Path::new(env!("CARGO_MANIFEST_DIR")), None)
}

/// The same, built from the checkout `repo`; `head` is a pull request's
/// head commit when the checkout is its merge with master.
fn publish_from(key: &Path, updates: &Path, tmp: &Path, version: &str, channel: &str, repo: &Path, head: Option<&str>) -> i32 {
    publish_as(key, updates, tmp, version, channel, repo, head, None)
}

/// The same, as CI publishes a pull request's build: `pr` is its number and
/// the pull requests CI found closed without merging (`MAGNUS_GIVEN_UP`).
#[allow(clippy::too_many_arguments)]
fn publish_as(key: &Path, updates: &Path, tmp: &Path, version: &str, channel: &str, repo: &Path, head: Option<&str>, pr: Option<(u32, &str)>) -> i32 {
    publish_landed(key, updates, tmp, version, channel, repo, head, pr, "")
}

/// The same, with the merged pull requests CI found and the master commit
/// each landed as (`MAGNUS_LANDED`, `number=commit`).
#[allow(clippy::too_many_arguments)]
fn publish_landed(key: &Path, updates: &Path, tmp: &Path, version: &str, channel: &str, repo: &Path, head: Option<&str>, pr: Option<(u32, &str)>, landed: &str) -> i32 {
    let (dist, notes) = (tmp.join("dist"), tmp.join("notes"));
    build(&dist, version);
    std::fs::write(&notes, format!("Build {version}.\n")).unwrap();
    let out = Command::new(release("publish.sh"))
        .args(["desktop", version])
        .args([updates, &notes, &dist])
        .arg(channel)
        .env("MAGNUS_RELEASE_KEY", key)
        .env_remove("MAGNUS_COMMIT")
        .env_remove("MAGNUS_PR")
        .env_remove("MAGNUS_GIVEN_UP")
        .env("MAGNUS_LANDED", landed)
        .envs(head.map(|h| ("MAGNUS_COMMIT", h)))
        .envs(pr.map(|(n, given_up)| [("MAGNUS_PR", n.to_string()), ("MAGNUS_GIVEN_UP", given_up.to_string())]).into_iter().flatten())
        .current_dir(repo)
        .output()
        .unwrap();
    assert!(out.status.code() != Some(1), "publish {version} to {channel}: {}", String::from_utf8_lossy(&out.stderr));
    out.status.code().unwrap()
}

/// What an app at `version` is offered, asking as the apps do: with the
/// session of `who` if signed in (apps from before channels ask without).
async fn offered(app: &Appliance, who: Option<&str>, version: &str) -> (StatusCode, Value) {
    let mut r = reqwest::Client::new().get(format!("{}/v1/updates/desktop/latest.json", app.base)).header("x-magnus-app", version);
    if let Some(w) = who {
        r = r.bearer_auth(app.api(w).token);
    }
    let r = r.send().await.unwrap();
    let st = r.status();
    (st, r.json().await.unwrap_or(Value::Null))
}

async fn offered_version(app: &Appliance, who: Option<&str>, version: &str) -> Option<String> {
    let (st, m) = offered(app, who, version).await;
    match st {
        StatusCode::OK => Some(m["version"].as_str().unwrap().to_string()),
        StatusCode::NO_CONTENT => None,
        st => panic!("{who:?} at {version}: {st} {m}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_owner_gets_development_testers_get_test_and_everyone_else_production() {
    let tmp = tempfile::tempdir().unwrap();
    let key = tmp.path().join("test.key");
    let made = Command::new("cargo").args(["tauri", "signer", "generate", "--ci", "-p", "", "-f", "-w"]).arg(&key).output().unwrap();
    assert!(made.status.success(), "a test release key: {}", String::from_utf8_lossy(&made.stderr));

    let m = FakeModel::start(steps(vec![])).await;
    let mut app = Appliance::start(Spec::default(), &m).await;
    let updates = app.dir.path().join("updates");
    let path = app.dir.path().join("magnus.toml");
    let mut cfg: toml::Table = std::fs::read_to_string(&path).unwrap().parse().unwrap();
    cfg.insert("updates".into(), toml::Value::Table(toml::toml! { dir = (updates.display().to_string()) }));
    std::fs::write(&path, toml::to_string(&cfg).unwrap()).unwrap();
    app.restart().await;
    let (st, _) = app.api("alice").put("/v1/people/carol/roles", serde_json::json!({ "roles": ["tester"] })).await;
    assert_eq!(st, StatusCode::NO_CONTENT);

    // Nothing published: nothing offered.
    for who in ["alice", "bob", "carol"] {
        assert_eq!(offered_version(&app, Some(who), "0.7.0").await, None, "{who}");
    }

    // A pull request, master and a tag, each to its channel: the owner's
    // apps get development, the tester's test, everyone else's production.
    let w = tmp.path().join("work");
    assert_eq!(publish(&key, &updates, &w, "0.7.0", "production"), 0);
    assert_eq!(publish(&key, &updates, &w, "0.7.1-test.10", "test"), 0);
    assert_eq!(publish(&key, &updates, &w, "0.7.1-review.11.pr.7", "development"), 0);
    let offers = |who| offered_version(&app, Some(who), "0.7.0");
    assert_eq!(offers("alice").await.as_deref(), Some("0.7.1-review.11.pr.7"));
    assert_eq!(offers("carol").await.as_deref(), Some("0.7.1-test.10"));
    assert_eq!(offers("bob").await.as_deref(), Some("0.7.0"));

    // Each later build replaces its channel's; a slower run finishing last
    // does not move a channel back.
    assert_eq!(publish(&key, &updates, &w, "0.7.1-review.13.pr.8", "development"), 0);
    assert_eq!(publish(&key, &updates, &w, "0.7.1-review.12.pr.7", "development"), 0);
    assert_eq!(publish(&key, &updates, &w, "0.7.1-test.14", "test"), 0);
    assert_eq!(publish(&key, &updates, &w, "0.7.1-test.9", "test"), 0);
    assert_eq!(offers("alice").await.as_deref(), Some("0.7.1-review.13.pr.8"));
    assert_eq!(offers("carol").await.as_deref(), Some("0.7.1-test.14"));
    // Development and test keep only their latest build's packages.
    for (c, v) in [("development", "0.7.1-review.13.pr.8"), ("test", "0.7.1-test.14")] {
        let mut kept: Vec<String> = std::fs::read_dir(updates.join("desktop").join(c)).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
        kept.sort();
        let want: Vec<String> = [
            format!("Magnus_{v}_amd64.AppImage"),
            format!("Magnus_{v}_amd64.AppImage.sig"),
            format!("Magnus_{v}_x64-setup.exe"),
            format!("Magnus_{v}_x64-setup.exe.sig"),
            "latest.json".into(),
        ]
        .into();
        assert_eq!(kept, want, "{c}");
    }
    for c in ["stable", "pr-7", "../test"] {
        assert_eq!(publish(&key, &updates, &w, "0.7.1-review.15.pr.7", c), 2, "{c}");
    }

    // A tag does not take an app off its channel's build, newer or not: it
    // need not hold that build's work (a pull request not yet merged). The
    // channel's next build moves it forward.
    assert_eq!(publish(&key, &updates, &w, "0.7.1", "production"), 0);
    assert_eq!(offers("alice").await.as_deref(), Some("0.7.1-review.13.pr.8"));
    assert_eq!(offers("carol").await.as_deref(), Some("0.7.1-test.14"));
    assert_eq!(offers("bob").await.as_deref(), Some("0.7.1"));
    assert_eq!(publish(&key, &updates, &w, "0.7.2-review.16.pr.9", "development"), 0);
    assert_eq!(publish(&key, &updates, &w, "0.7.2-test.17", "test"), 0);
    assert_eq!(offers("alice").await.as_deref(), Some("0.7.2-review.16.pr.9"));
    assert_eq!(offers("carol").await.as_deref(), Some("0.7.2-test.17"));
    assert_eq!(offers("bob").await.as_deref(), Some("0.7.1"));

    // The packages are where the manifest says, signed for their version.
    for (who, files, v) in [("alice", "development/files", "0.7.2-review.16.pr.9"), ("carol", "test/files", "0.7.2-test.17"), ("bob", "files", "0.7.1")] {
        let (_, man) = offered(&app, Some(who), "0.7.0").await;
        let linux = &man["platforms"]["linux-x86_64"];
        let url = linux["url"].as_str().unwrap();
        assert_eq!(url, format!("{}/v1/updates/desktop/{files}/Magnus_{v}_amd64.AppImage", app.base));
        // The updater asks for the package with the session it asked with.
        let got = reqwest::Client::new().get(url).bearer_auth(app.api(who).token).send().await.unwrap();
        assert_eq!(got.text().await.unwrap(), format!("the {v} build"));
        // A development or test package is only for the apps given it.
        if files != "files" {
            assert_eq!(reqwest::get(url).await.unwrap().status(), StatusCode::NOT_FOUND, "{files} without a session");
            for other in ["alice", "bob", "carol"].into_iter().filter(|o| *o != who) {
                let st = reqwest::Client::new().get(url).bearer_auth(app.api(other).token).send().await.unwrap().status();
                assert_eq!(st, StatusCode::NOT_FOUND, "{other} got {files}");
            }
        }
        let sig = String::from_utf8(base64_decode(linux["signature"].as_str().unwrap())).unwrap();
        assert!(sig.contains(&format!("version:{v}")), "the signature does not bind the version: {sig}");
    }

    // Without a session that holds (signed out, ended, or an app from before
    // channels), the version an app says it runs chooses nothing: a release
    // gets production, a pre-release nothing until it asks signed in (one
    // too old to sign in at all, production: its only way forward). An
    // ended session is not sent to production, which need not hold its
    // build's work (review of master, 2026-10-31).
    for token in [Some("mgs_ended-session"), None] {
        for (v, offer) in [
            ("0.7.0", Some("0.7.1")),
            ("", Some("0.7.1")),
            ("0.7.1-review.107.pr.31", None),
            ("0.7.2-review.16.pr.9", None),
            ("0.7.1-test.3", None),
            ("0.4.9-test.3", Some("0.7.1")),
        ] {
            let mut r = reqwest::Client::new().get(format!("{}/v1/updates/desktop/latest.json", app.base)).header("x-magnus-app", v);
            if let Some(t) = token {
                r = r.bearer_auth(t);
            }
            let r = r.send().await.unwrap();
            let got = match r.status() {
                StatusCode::OK => Some(r.json::<Value>().await.unwrap()["version"].as_str().unwrap().to_string()),
                StatusCode::NO_CONTENT => None,
                st => panic!("an app at {v:?}: {st}"),
            };
            assert_eq!(got.as_deref(), offer, "an app at {v:?} with {token:?}");
        }
    }

    // No app names its own channel, and nothing outside the channels is served.
    for p in [
        "/v1/updates/desktop/test/latest.json",
        "/v1/updates/desktop/development/latest.json",
        "/v1/updates/desktop/files/latest.json",
        "/v1/updates/desktop/test/files/latest.json",
        "/v1/updates/desktop/pr-9/files/Magnus_0.7.2-review.16.pr.9_amd64.AppImage",
        "/v1/updates/desktop/test/files/..%2Flatest.json",
        "/v1/updates/desktop/..%2F..%2Fappliance/latest.json",
    ] {
        let st = reqwest::get(format!("{}{p}", app.base)).await.unwrap().status();
        assert_eq!(st, StatusCode::NOT_FOUND, "{p}");
    }
}

/// git in `repo`; its output.
fn git(repo: &Path, args: &[&str]) -> String {
    let out = Command::new("git").args(["-c", "user.name=Test", "-c", "user.email=test@example.com"]).args(args).current_dir(repo).output().unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

/// A commit on the current branch of `repo`; its id.
fn commit(repo: &Path, file: &str) -> String {
    std::fs::write(repo.join(file), file).unwrap();
    git(repo, &["add", file]);
    git(repo, &["commit", "-qm", file]);
    git(repo, &["rev-parse", "HEAD"])
}

/// What the owner saw (2026-10-31): their app ran pull request A's build,
/// then got pull request B's, numbered later but built without A's work.
/// A channel's next build must hold the work of the one there; one that
/// does not is not published, whatever its version. On development that
/// leaves the job green (another open pull request may merge); on test
/// and production the job fails.
#[test]
fn a_build_without_the_work_already_in_its_channel_is_never_published() {
    let tmp = tempfile::tempdir().unwrap();
    let key = tmp.path().join("test.key");
    let made = Command::new("cargo").args(["tauri", "signer", "generate", "--ci", "-p", "", "-f", "-w"]).arg(&key).output().unwrap();
    assert!(made.status.success(), "a test release key: {}", String::from_utf8_lossy(&made.stderr));
    let (updates, w, repo) = (tmp.path().join("updates"), tmp.path().join("work"), tmp.path().join("repo"));
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "master"]);
    let base = commit(&repo, "base");
    git(&repo, &["checkout", "-q", "-b", "a"]);
    let a = commit(&repo, "a");
    git(&repo, &["checkout", "-q", "-b", "b", &base]);
    let b = commit(&repo, "b");
    let channel = |c: &str| -> (String, String) {
        let dir = if c == "production" { updates.join("desktop") } else { updates.join("desktop").join(c) };
        let m: Value = serde_json::from_slice(&std::fs::read(dir.join("latest.json")).unwrap()).unwrap();
        (m["version"].as_str().unwrap().into(), m["commit"].as_str().unwrap().into())
    };

    // A's build is in development; B's, later but without A's work, is not
    // published, and the job still passes.
    git(&repo, &["checkout", "-q", &a]);
    assert_eq!(publish_from(&key, &updates, &w, "0.7.1-review.20.pr.1", "development", &repo, None), 0);
    git(&repo, &["checkout", "-q", &b]);
    assert_eq!(publish_from(&key, &updates, &w, "0.7.1-review.21.pr.2", "development", &repo, None), 0);
    assert_eq!(channel("development"), ("0.7.1-review.20.pr.1".into(), a.clone()));

    // A merges. B, built as CI builds a pull request (its merge with
    // master), holds A's work and goes out, recorded by its own head.
    git(&repo, &["checkout", "-q", "master"]);
    git(&repo, &["merge", "-q", "--no-ff", "--no-edit", "a"]);
    let merged = git(&repo, &["rev-parse", "HEAD"]);
    assert_eq!(publish_from(&key, &updates, &w, "0.7.1-test.22", "test", &repo, None), 0);
    git(&repo, &["checkout", "-q", "--detach", "master"]);
    git(&repo, &["merge", "-q", "--no-ff", "--no-edit", "b"]);
    assert_eq!(publish_from(&key, &updates, &w, "0.7.1-review.23.pr.2", "development", &repo, Some(&b)), 0);
    assert_eq!(channel("development"), ("0.7.1-review.23.pr.2".into(), b.clone()));

    // A's old branch, rebuilt later, lacks B: not published. A test build
    // or a tag of a commit older than the one its channel holds fails the job.
    git(&repo, &["checkout", "-q", &a]);
    assert_eq!(publish_from(&key, &updates, &w, "0.7.1-review.24.pr.1", "development", &repo, None), 0);
    git(&repo, &["checkout", "-q", &base]);
    assert_eq!(publish_from(&key, &updates, &w, "0.7.1-test.25", "test", &repo, None), 3);
    assert_eq!(channel("test"), ("0.7.1-test.22".into(), merged.clone()));
    git(&repo, &["checkout", "-q", &merged]);
    assert_eq!(publish_from(&key, &updates, &w, "0.7.1", "production", &repo, None), 0);
    git(&repo, &["checkout", "-q", &base]);
    assert_eq!(publish_from(&key, &updates, &w, "0.7.2", "production", &repo, None), 3);
    assert_eq!(channel("production"), ("0.7.1".into(), merged));
    assert_eq!(channel("development"), ("0.7.1-review.23.pr.2".into(), b));
}

/// Review of master (2026-10-31): a pull request whose build reached
/// development and was then closed without merging made every later pull
/// request's publish fail (its head is in no later build), and so did a
/// pull request pushed again with its commits replaced. Its work was given
/// up on purpose; the next build must still hold master's.
#[test]
fn a_pull_request_closed_unmerged_or_pushed_again_stops_no_later_build() {
    let tmp = tempfile::tempdir().unwrap();
    let key = tmp.path().join("test.key");
    let made = Command::new("cargo").args(["tauri", "signer", "generate", "--ci", "-p", "", "-f", "-w"]).arg(&key).output().unwrap();
    assert!(made.status.success(), "a test release key: {}", String::from_utf8_lossy(&made.stderr));
    let (updates, w, repo) = (tmp.path().join("updates"), tmp.path().join("work"), tmp.path().join("repo"));
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "master"]);
    let base = commit(&repo, "base");
    let dev = || -> Value { serde_json::from_slice(&std::fs::read(updates.join("desktop/development/latest.json")).unwrap()).unwrap() };
    // A pull request's build as CI makes it: its branch from `from`, merged
    // with master; its head.
    let pr = |name: &str, from: &str| -> String {
        git(&repo, &["checkout", "-q", "-b", name, from]);
        let head = commit(&repo, name);
        git(&repo, &["checkout", "-q", "--detach", "master"]);
        git(&repo, &["merge", "-q", "--no-ff", "--no-edit", name]);
        head
    };

    // Pull request 1's build is in development; it is closed unmerged.
    let a = pr("a", &base);
    assert_eq!(publish_as(&key, &updates, &w, "0.7.1-review.20.pr.1", "development", &repo, Some(&a), Some((1, ""))), 0);
    // Pull request 2, from master: not published while CI has not found 1
    // closed (the job still passes), published once it has.
    let b = pr("b", "master");
    assert_eq!(publish_as(&key, &updates, &w, "0.7.1-review.21.pr.2", "development", &repo, Some(&b), Some((2, ""))), 0);
    assert_eq!(dev()["commit"], a.as_str(), "an open pull request's build was replaced");
    assert_eq!(publish_as(&key, &updates, &w, "0.7.1-review.21.pr.2", "development", &repo, Some(&b), Some((2, "1"))), 0, "a pull request closed unmerged stopped the next");
    assert_eq!((dev()["commit"].as_str(), dev()["pr"].as_u64(), dev()["base"].as_str()), (Some(b.as_str()), Some(2), Some(base.as_str())));

    // Pull request 2 pushed again with its commits replaced: published.
    let b2 = pr("b2", "master");
    assert_eq!(publish_as(&key, &updates, &w, "0.7.1-review.22.pr.2", "development", &repo, Some(&b2), Some((2, ""))), 0);
    assert_eq!(dev()["commit"], b2.as_str());

    // Master moves on, and 2 is rebuilt on it. A pull request whose build
    // lacks master's work is still refused, given up or not.
    git(&repo, &["checkout", "-q", "master"]);
    let m = commit(&repo, "m");
    let b3 = pr("b3", "master");
    assert_eq!(publish_as(&key, &updates, &w, "0.7.1-review.23.pr.2", "development", &repo, Some(&b3), Some((2, ""))), 0);
    assert_eq!(dev()["base"], m.as_str());
    git(&repo, &["checkout", "-q", "-b", "c", &base]);
    let c = commit(&repo, "c");
    git(&repo, &["checkout", "-q", "--detach", &base]);
    git(&repo, &["merge", "-q", "--no-ff", "--no-edit", "c"]);
    assert_eq!(publish_as(&key, &updates, &w, "0.7.1-review.24.pr.3", "development", &repo, Some(&c), Some((3, "2"))), 3);
    assert_eq!(dev()["commit"], b3.as_str());
}

/// What blocked pull request #120 (2026-11-03): #119's build was in
/// development and #119 was squash-merged, so master holds its work in a
/// new commit but never its head, and every later build was refused as
/// lacking it. A merged pull request's work is where it landed in master.
#[test]
fn a_pull_request_squash_merged_stops_no_later_build() {
    let tmp = tempfile::tempdir().unwrap();
    let key = tmp.path().join("test.key");
    let made = Command::new("cargo").args(["tauri", "signer", "generate", "--ci", "-p", "", "-f", "-w"]).arg(&key).output().unwrap();
    assert!(made.status.success(), "a test release key: {}", String::from_utf8_lossy(&made.stderr));
    let (updates, w, repo) = (tmp.path().join("updates"), tmp.path().join("work"), tmp.path().join("repo"));
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "master"]);
    let base = commit(&repo, "base");
    let dev = || -> Value { serde_json::from_slice(&std::fs::read(updates.join("desktop/development/latest.json")).unwrap()).unwrap() };
    let pr = |name: &str, from: &str| -> String {
        git(&repo, &["checkout", "-q", "-b", name, from]);
        let head = commit(&repo, name);
        git(&repo, &["checkout", "-q", "--detach", "master"]);
        git(&repo, &["merge", "-q", "--no-ff", "--no-edit", name]);
        head
    };

    // Pull request 1's build is in development; it is squash-merged.
    let a = pr("a", &base);
    assert_eq!(publish_as(&key, &updates, &w, "0.7.1-review.20.pr.1", "development", &repo, Some(&a), Some((1, ""))), 0);
    git(&repo, &["checkout", "-q", "master"]);
    git(&repo, &["merge", "-q", "--squash", "a"]);
    git(&repo, &["commit", "-qm", "a (#1)"]);
    let landed = git(&repo, &["rev-parse", "HEAD"]);

    // Pull request 2, from master, holds 1's work where it landed: published.
    let b = pr("b", "master");
    let one = format!("1={landed}");
    assert_eq!(publish_landed(&key, &updates, &w, "0.7.1-review.21.pr.2", "development", &repo, Some(&b), Some((2, "")), &one), 0, "a squash-merged pull request stopped the next");
    assert_eq!(dev()["commit"], b.as_str());

    // 2 is squash-merged too. A build without where it landed is refused,
    // and so is one told of another pull request's landing than the build
    // there; one from master goes out.
    git(&repo, &["checkout", "-q", "master"]);
    git(&repo, &["merge", "-q", "--squash", "b"]);
    git(&repo, &["commit", "-qm", "b (#2)"]);
    let landed = git(&repo, &["rev-parse", "HEAD"]);
    let (two, other) = (format!("2={landed}"), format!("9={landed}"));
    let c = pr("c", &base);
    git(&repo, &["checkout", "-q", "--detach", &base]);
    git(&repo, &["merge", "-q", "--no-ff", "--no-edit", "c"]);
    assert_eq!(publish_landed(&key, &updates, &w, "0.7.1-review.22.pr.3", "development", &repo, Some(&c), Some((3, "")), &two), 3);
    let d = pr("d", &base);
    git(&repo, &["checkout", "-q", "--detach", &base]);
    git(&repo, &["merge", "-q", "--no-ff", "--no-edit", "d"]);
    assert_eq!(publish_landed(&key, &updates, &w, "0.7.1-review.23.pr.4", "development", &repo, Some(&d), Some((4, "")), &other), 3);
    assert_eq!(dev()["commit"], b.as_str());
    let e = pr("e", "master");
    assert_eq!(publish_landed(&key, &updates, &w, "0.7.1-review.24.pr.5", "development", &repo, Some(&e), Some((5, "")), &two), 0);
    assert_eq!(dev()["commit"], e.as_str());
}

/// Redeploy of 0.7.1-test.1 (2026-11-09): the build step left the release
/// key in TAURI_SIGNING_PRIVATE_KEY, the signer refused the key file beside
/// it, and both channels were published with empty signatures, which no app
/// can install. A key in the environment is ignored; a package the key file
/// cannot sign is never published.
#[test]
fn a_release_is_never_published_without_its_signatures() {
    let tmp = tempfile::tempdir().unwrap();
    let key = tmp.path().join("test.key");
    let made = Command::new("cargo").args(["tauri", "signer", "generate", "--ci", "-p", "", "-f", "-w"]).arg(&key).output().unwrap();
    assert!(made.status.success(), "a test release key: {}", String::from_utf8_lossy(&made.stderr));
    let (updates, dist, notes) = (tmp.path().join("updates"), tmp.path().join("dist"), tmp.path().join("notes"));
    std::fs::write(&notes, "Build.\n").unwrap();
    let run = |key: &Path, version: &str| {
        build(&dist, version);
        Command::new(release("publish.sh"))
            .args(["desktop", version])
            .args([&updates, &notes, &dist])
            .arg("test")
            .env("MAGNUS_RELEASE_KEY", key)
            .env("TAURI_SIGNING_PRIVATE_KEY", std::fs::read_to_string(key).unwrap_or_default())
            .env_remove("MAGNUS_COMMIT")
            .env_remove("MAGNUS_PR")
            .env_remove("MAGNUS_LANDED")
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .output()
            .unwrap()
    };
    let manifest = updates.join("desktop/test/latest.json");

    let out = run(&key, "0.7.1-test.1");
    assert!(out.status.success(), "publish with a key in the environment: {}", String::from_utf8_lossy(&out.stderr));
    let m: Value = serde_json::from_slice(&std::fs::read(&manifest).unwrap()).unwrap();
    for platform in ["linux-x86_64", "windows-x86_64"] {
        let sig = base64_decode(m["platforms"][platform]["signature"].as_str().unwrap());
        assert!(String::from_utf8_lossy(&sig).starts_with("untrusted comment:"), "{platform} is signed: {m}");
    }

    let broken = tmp.path().join("broken.key");
    std::fs::write(&broken, "not a key").unwrap();
    let out = run(&broken, "0.7.1-test.2");
    assert!(!out.status.success(), "a package the key cannot sign is not published");
    let m: Value = serde_json::from_slice(&std::fs::read(&manifest).unwrap()).unwrap();
    assert_eq!(m["version"], "0.7.1-test.1");
}

/// First release run (2026-11-09): the workflow's checkout held one commit,
/// so publish.sh could not see that main held the commit the test channel
/// was built from, and refused a build that did hold it. The release job
/// checks out the whole history.
#[test]
fn the_release_job_checks_out_the_history_publish_compares_against() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.github/workflows/release.yml");
    let workflow = std::fs::read_to_string(path).unwrap();
    let checkout = workflow.split("uses: actions/checkout").nth(1).expect("the release job checks out the repository");
    let step = checkout.split("\n      - ").next().unwrap();
    assert!(step.contains("fetch-depth: 0"), "the release checkout fetches all history: {step}");
}

/// Release run 42741994831 (2026-11-09): the AppImage failed to bundle
/// ("failed to run linuxdeploy") because the release job let the bundler
/// fetch the moving AppImage runtime mid-build, which the pinned runtime
/// (release/ensure-appimage-runtime.sh) exists to prevent.
#[test]
fn the_release_bundles_the_appimage_with_the_pinned_runtime() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.github/workflows/release.yml");
    let workflow = std::fs::read_to_string(path).unwrap();
    let (pinned, bundled) = (
        workflow.find("LDAI_RUNTIME_FILE=\"$(sh release/ensure-appimage-runtime.sh)\"").expect("the release pins the AppImage runtime"),
        workflow.find("--bundles deb,appimage").unwrap(),
    );
    assert!(pinned < bundled, "pinned before the AppImage is bundled");
}

/// Release run 42746287718 (2026-11-09): the sandbox image copies the
/// browser's packages, which the release job's fresh checkout never
/// installed, so putting the release on the server stopped. to-appliance.sh
/// installs them before it builds the image.
#[test]
fn the_server_release_installs_the_browser_packages_before_the_image() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../release/to-appliance.sh");
    let script = std::fs::read_to_string(path).unwrap();
    let install = script.find("|| npm ci --prefix browser\n").expect("to-appliance.sh installs the browser's packages when they are missing");
    let image = script.find("image=$(sandbox/image.sh)").unwrap();
    assert!(install < image, "the packages are installed before the image is built");
}

/// The new magnus-teams server (2026-11-09) never had Google's or Slack's
/// client secret: only the admin console's deploy provisioned them, and the
/// release never ran it, so no one could connect those apps. The release
/// provisions every catalog provider's client (2026-11-10: it gave only
/// Google's and Slack's secrets, and no client ID), from the repository's
/// secrets, after the server updates.
#[test]
fn the_release_provisions_every_providers_client_after_the_server_updates() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let workflow = std::fs::read_to_string(root.join(".github/workflows/release.yml")).unwrap();
    let script = std::fs::read_to_string(root.join("release/provision-registrations.sh")).unwrap();
    let (server, provision) =
        (workflow.find("release/to-appliance.sh").unwrap(), workflow.find("release/provision-registrations.sh").expect("the release provisions the provider secrets"));
    assert!(provision > server, "provisioned after the server runs the release's programs");
    let catalog: toml::Table = std::fs::read_to_string(root.join("catalog/registrations.toml")).unwrap().parse().unwrap();
    assert!(catalog.len() >= 6, "read the catalog's registrations");
    for (provider, r) in &catalog {
        let prefix =
            script.lines().find_map(|l| l.strip_prefix(&format!("provision {provider} "))).unwrap_or_else(|| panic!("the script provisions {provider}")).trim().to_string();
        let mut vars = vec![format!("{prefix}_CLIENT_ID")];
        if r["kind"].as_str() != Some("public") {
            vars.push(format!("{prefix}_CLIENT_SECRET"));
        }
        for v in vars {
            assert!(workflow.contains(&format!("{v}: ${{{{ secrets.{v} }}}}")), "the step is given {v} for {provider}");
        }
    }
}

/// The script sends each provider's client ID and secret to the server's
/// control socket and prints, per provider, only whether the repository's ID
/// matches the catalog's, and the answer: never a value.
#[tokio::test(flavor = "multi_thread")]
async fn the_provisioning_sends_each_client_and_never_prints_a_value() {
    use std::sync::{Arc, Mutex};
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let catalog: toml::Table = std::fs::read_to_string(root.join("catalog/registrations.toml")).unwrap().parse().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("magnus.toml");
    std::fs::write(&config, format!("[appliance]\nstate_dir = \"{}\"\n", dir.path().display())).unwrap();
    let got: Arc<Mutex<Vec<(String, Value)>>> = Arc::default();
    let seen = got.clone();
    let app = axum::Router::new().route(
        "/v1/admin/registrations/{provider}",
        axum::routing::put(move |axum::extract::Path(p): axum::extract::Path<String>, axum::Json(b): axum::Json<Value>| {
            let seen = seen.clone();
            async move {
                seen.lock().unwrap().push((p, b));
                axum::Json(serde_json::json!({ "secret": true }))
            }
        }),
    );
    let l = tokio::net::UnixListener::bind(dir.path().join("control.sock")).unwrap();
    tokio::spawn(async move { axum::serve(l, app).await });
    let google = catalog["google"]["client_id"].as_str().unwrap();
    let values = [
        ("GOOGLE_DESKTOP_CLIENT_ID", google.to_string()),
        ("GOOGLE_DESKTOP_CLIENT_SECRET", "secret-google-desktop".into()),
        ("GH_OAUTH_CLIENT_ID", "id-github-teams".into()),
        ("GH_OAUTH_CLIENT_SECRET", "secret-github".into()),
        ("VERCEL_CLIENT_ID", "id-vercel-teams".into()),
        ("VERCEL_CLIENT_SECRET", "secret-vercel-never-sent".into()),
        ("SLACK_CLIENT_SECRET", "secret-slack".into()),
        ("GOOGLE_WEB_CLIENT_ID", "id-google-web".into()),
        ("ASANA_CLIENT_ID", "id-asana-teams".into()),
        ("ASANA_CLIENT_SECRET", "secret-asana".into()),
    ];
    let mut cmd = tokio::process::Command::new(root.join("release/provision-registrations.sh"));
    cmd.arg(&config);
    for v in ["GOOGLE", "GOOGLE_DESKTOP", "GOOGLE_WEB", "MICROSOFT", "GH_OAUTH", "VERCEL", "SLACK", "SENTRY", "ASANA"] {
        cmd.env_remove(format!("{v}_CLIENT_ID")).env_remove(format!("{v}_CLIENT_SECRET"));
    }
    cmd.env_remove("MICROSOFT_TENANT_ID");
    for (k, v) in &values {
        cmd.env(k, v);
    }
    let out = cmd.output().await.unwrap();
    let printed = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success(), "{printed}");
    for (k, v) in &values {
        // The catalog's own Google ID is public; every other value is not.
        if *k != "GOOGLE_DESKTOP_CLIENT_ID" {
            assert!(!printed.contains(v.as_str()), "{k}'s value was printed:\n{printed}");
        }
    }
    for line in [
        "google: GOOGLE_DESKTOP_CLIENT_ID match",
        "github: GH_OAUTH_CLIENT_ID differs",
        "slack: SLACK_CLIENT_ID not set",
        "google: GOOGLE_WEB_CLIENT_ID differs",
        "asana: ASANA_CLIENT_ID set (the catalog names no client ID",
    ] {
        assert!(printed.contains(line), "said {line:?}:\n{printed}");
    }
    let got = got.lock().unwrap().clone();
    let body = |p: &str| got.iter().find(|(q, _)| q == p).map(|(_, b)| b.clone()).unwrap_or_else(|| panic!("{p} provisioned:\n{printed}"));
    assert_eq!(body("google"), serde_json::json!({ "actor": "release", "client_id": google, "client_secret": "secret-google-desktop" }));
    assert_eq!(body("github"), serde_json::json!({ "actor": "release", "client_id": "id-github-teams", "client_secret": "secret-github" }));
    assert_eq!(body("vercel"), serde_json::json!({ "actor": "release", "client_id": "id-vercel-teams" }), "a public client is sent no secret");
    assert_eq!(body("slack"), serde_json::json!({ "actor": "release", "client_secret": "secret-slack" }));
    // Asana's client is only ever provisioned: the catalog names no ID.
    assert_eq!(body("asana"), serde_json::json!({ "actor": "release", "client_id": "id-asana-teams", "client_secret": "secret-asana" }));
    assert!(got.iter().all(|(p, _)| p != "microsoft" && p != "sentry"), "nothing set, nothing sent: {got:?}");
}

/// Every script the release job runs is committed executable: a fresh
/// checkout runs it as committed (the first release with the provider
/// secrets step, 2026-11-09: "release/provision-registrations.sh: Permission
/// denied").
#[test]
fn every_script_the_release_runs_is_committed_executable() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let workflow = std::fs::read_to_string(root.join(".github/workflows/release.yml")).unwrap();
    let out = Command::new("git").args(["ls-files", "-s", "release"]).current_dir(&root).output().unwrap();
    let index = String::from_utf8(out.stdout).unwrap();
    let mut checked = 0;
    for line in index.lines() {
        let (mode, path) = (line.split_whitespace().next().unwrap(), line.split('\t').nth(1).unwrap());
        if path.ends_with(".sh") && workflow.contains(&format!("{path} ")) {
            assert_eq!(mode, "100755", "{path} is run by the release job but committed without its execute bit");
            checked += 1;
        }
    }
    assert!(checked >= 3, "found the scripts the release job runs ({checked})");
}

fn base64_decode(s: &str) -> Vec<u8> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.decode(s).unwrap()
}
