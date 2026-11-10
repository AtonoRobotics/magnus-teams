//! `ci/on-strix-beta.sh`, run against stand-ins for `ssh` and `rsync` that
//! record what they were asked to do.

use std::path::{Path, PathBuf};
use std::process::Command;

const STAND_IN_SSH: &str = "#!/bin/sh\nfor a; do last=$a; done\ncat >/dev/null\nprintf '%s\\n' \"$last\" >> \"$CI_LOG/ssh\"\n";
const STAND_IN_RSYNC: &str = "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$CI_LOG/rsync\"\n";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The remote commands and rsync calls of one run of the script.
fn run(name: &str, command: &[&str]) -> (Vec<String>, Vec<String>) {
    use std::os::unix::fs::PermissionsExt;
    let (stand_ins, log) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    for (file, body) in [("ssh", STAND_IN_SSH), ("rsync", STAND_IN_RSYNC)] {
        let p = stand_ins.path().join(file);
        std::fs::write(&p, body).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let path = format!("{}:{}", stand_ins.path().display(), std::env::var("PATH").unwrap_or_default());
    let out = Command::new("sh")
        .arg(root().join("ci/on-strix-beta.sh"))
        .arg(name)
        .args(command)
        .env("PATH", path)
        .env("CI_LOG", log.path())
        .env("STRIX_BETA", "nobody@ci-test.invalid")
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let read = |f: &str| std::fs::read_to_string(log.path().join(f)).unwrap_or_default().lines().map(str::to_string).collect::<Vec<_>>();
    (read("ssh"), read("rsync"))
}

fn target_dir(remote: &str) -> &str {
    let at = remote.find("CARGO_TARGET_DIR=").unwrap() + "CARGO_TARGET_DIR=".len();
    remote[at..].split_whitespace().next().unwrap()
}

#[test]
fn each_name_builds_into_its_own_directory() {
    let (a, _) = run("ci-test-one", &["true"]);
    let (b, _) = run("ci-test-two", &["true"]);
    let (a, b) = (target_dir(a.last().unwrap()), target_dir(b.last().unwrap()));
    assert_ne!(a, b, "two names share one build directory");
    assert!(a.ends_with("/ci-test-one"), "{a}");
}

#[test]
fn a_test_command_builds_the_programs_first() {
    let (ssh, _) = run("ci-test-one", &["cargo", "test", "-p", "magnus-scratch"]);
    let remote = ssh.last().unwrap();
    let build = remote.find("cargo build --workspace --bins").expect("the programs are not built");
    assert!(build < remote.find("'cargo' 'test'").unwrap(), "{remote}");
    let (ssh, _) = run("ci-test-one", &["cargo", "fmt", "--all", "--check"]);
    assert!(!ssh.last().unwrap().contains("cargo build"), "only a test command builds first");
}

/// `ci/acceptance.sh invariants` against a stand-in `cargo` that records its
/// arguments and the Chromium it was given.
#[test]
fn the_invariants_run_one_at_a_time_with_a_chromium() {
    use std::os::unix::fs::PermissionsExt;
    let (bin, log, home) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let cargo = bin.path().join("cargo");
    std::fs::write(&cargo, "#!/bin/sh\nprintf '%s|%s\\n' \"$*\" \"$MAGNUS_CHROMIUM\" >> \"$CI_LOG/cargo\"\n").unwrap();
    std::fs::set_permissions(&cargo, std::fs::Permissions::from_mode(0o755)).unwrap();
    let chrome = home.path().join(".cache/ms-playwright/chromium-1234/chrome-linux64/chrome");
    std::fs::create_dir_all(chrome.parent().unwrap()).unwrap();
    std::fs::write(&chrome, "").unwrap();
    std::fs::set_permissions(&chrome, std::fs::Permissions::from_mode(0o755)).unwrap();
    let path = format!("{}:{}", bin.path().display(), std::env::var("PATH").unwrap_or_default());
    let out = Command::new("sh")
        .arg(root().join("ci/acceptance.sh"))
        .arg("invariants")
        .env("PATH", path)
        .env("CI_LOG", log.path())
        .env("HOME", home.path())
        .env_remove("MAGNUS_CHROMIUM")
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let calls = std::fs::read_to_string(log.path().join("cargo")).unwrap();
    let test = calls.lines().find(|l| l.starts_with("test -p magnus-invariants")).unwrap_or_else(|| panic!("{calls}"));
    assert!(test.contains("--test-threads=1"), "the invariants run in parallel and run out of computers: {test}");
    assert!(test.ends_with(&format!("|{}", chrome.display())), "the invariants get no Chromium: {test}");
}

#[test]
fn the_copy_does_not_keep_times_cargo_would_skip() {
    let (_, rsync) = run("ci-test-one", &["true"]);
    let copy = rsync.iter().find(|c| c.contains("--exclude target/")).unwrap();
    let short: String = copy.split_whitespace().filter(|w| w.starts_with('-') && !w.starts_with("--")).collect();
    assert!(!short.contains('a') && !short.contains('t'), "the copy keeps file times: {copy}");
    assert!(copy.contains("--checksum"), "the copy compares by time, not content: {copy}");
}
