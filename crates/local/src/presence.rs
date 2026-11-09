//! Whether the person is using this computer, so the agent can keep out of
//! the way. When the system offers no way to tell, the agent is not held
//! back: the person sees and can stop what it does in the app.
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

/// In use until the mouse and keyboard have been still this long.
const IDLE_AFTER: Duration = Duration::from_secs(30);

fn output(program: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(program).args(args).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// GNOME answers `(uint64 12345,)`: the second run of digits, after the type.
fn parse_mutter(text: &str) -> Option<u64> {
    text.split(|c: char| !c.is_ascii_digit()).filter(|s| !s.is_empty()).nth(1).and_then(|s| s.parse().ok())
}

/// macOS answers `"HIDIdleTime" = 123456789` in nanoseconds.
fn parse_ioreg(text: &str) -> Option<Duration> {
    let line = text.lines().find(|l| l.contains("\"HIDIdleTime\""))?;
    let digits = line.rsplit('=').next()?.trim();
    Some(Duration::from_nanos(digits.parse().ok()?))
}

fn idle_for() -> Option<Duration> {
    if cfg!(target_os = "macos") {
        return parse_ioreg(&output("ioreg", &["-c", "IOHIDSystem"])?);
    }
    if let Some(ms) = output("xprintidle", &[]).and_then(|t| t.trim().parse().ok()) {
        return Some(Duration::from_millis(ms));
    }
    let mutter = [
        "call",
        "--session",
        "--dest",
        "org.gnome.Mutter.IdleMonitor",
        "--object-path",
        "/org/gnome/Mutter/IdleMonitor/Core",
        "--method",
        "org.gnome.Mutter.IdleMonitor.GetIdletime",
    ];
    output("gdbus", &mutter).and_then(|t| parse_mutter(&t)).map(Duration::from_millis)
}

/// A pause file is the app's own switch, for "keep off my computer for now".
pub fn in_use(pause_file: Option<&Path>) -> bool {
    pause_file.is_some_and(Path::exists) || idle_for().is_some_and(|idle| idle < IDLE_AFTER)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_systems_answer_reads() {
        assert_eq!(parse_mutter("(uint64 12345,)\n"), Some(12345));
        assert_eq!(parse_ioreg("  | \"HIDIdleTime\" = 2000000000\n  x"), Some(Duration::from_secs(2)));
    }

    #[test]
    fn the_apps_switch_holds_the_agent_back() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("paused");
        std::fs::write(&f, b"").unwrap();
        assert!(in_use(Some(&f)));
    }
}
