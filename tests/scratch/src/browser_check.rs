//! Running a browser check script (`desktop/tests/*.mjs`) from a test, so a
//! check that stops making progress fails the test instead of hanging it.
//!
//! A check runs in a process group of its own. When the script exits,
//! whatever it left behind in that group is killed, so a browser that outlives
//! it cannot hold the output pipes open. A script that runs past its limit is
//! killed with its group, and the test fails with what it printed so far.

use std::process::{Output, Stdio};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;

/// How long one browser check may run. The slowest checks sign in, click
/// through several screens and wait on the server, in about two minutes on
/// Strix Beta. A run on 2026-11-13 hung for over 90 minutes instead.
pub const BROWSER_CHECK_LIMIT_SECS: u64 = 300;

/// Runs the browser check `cmd` within `BROWSER_CHECK_LIMIT_SECS`, and
/// panics with its output if it runs past it.
pub async fn browser_check(cmd: Command) -> Output {
    run_within(cmd, Duration::from_secs(BROWSER_CHECK_LIMIT_SECS)).await.unwrap_or_else(|said| panic!("{said}"))
}

/// Runs `cmd` in a process group of its own and returns its output once it
/// exits. Anything it left running in that group is killed then. Past
/// `limit`, the whole group is killed, and the error carries what it printed.
pub async fn run_within(mut cmd: Command, limit: Duration) -> Result<Output, String> {
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).process_group(0).kill_on_drop(true);
    let mut child = cmd.spawn().map_err(|e| format!("could not start the check: {e}"))?;
    let group = child.id();
    let stdout = collect(child.stdout.take());
    let stderr = collect(child.stderr.take());
    let exited = tokio::time::timeout(limit, child.wait()).await;
    if let Some(group) = group {
        kill_group(group).await;
    }
    let status = match exited {
        Ok(status) => Some(status.map_err(|e| format!("waiting on the check: {e}"))?),
        Err(_) => {
            let _ = child.wait().await;
            None
        }
    };
    let (stdout, stderr) = (stdout.await.unwrap_or_default(), stderr.await.unwrap_or_default());
    match status {
        Some(status) => Ok(Output { status, stdout, stderr }),
        None => Err(format!("the check ran past {}s and was stopped\n{}{}", limit.as_secs(), String::from_utf8_lossy(&stdout), String::from_utf8_lossy(&stderr))),
    }
}

/// Reads `pipe` to its end in a task of its own.
fn collect(pipe: Option<impl AsyncRead + Unpin + Send + 'static>) -> tokio::task::JoinHandle<Vec<u8>> {
    tokio::spawn(async move {
        let mut all = Vec::new();
        if let Some(mut pipe) = pipe {
            let _ = pipe.read_to_end(&mut all).await;
        }
        all
    })
}

/// Kills every process left in group `group`. An empty group is no error.
async fn kill_group(group: u32) {
    let _ = Command::new("kill").args(["-KILL", "--", &format!("-{group}")]).stderr(Stdio::null()).status().await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn sh(script: &str) -> Command {
        let mut c = Command::new("sh");
        c.args(["-c", script]);
        c
    }

    /// The hang of 2026-11-13: the script is done, but a process it started
    /// still holds its output open, so reading to the end never ends.
    #[tokio::test]
    async fn a_check_that_leaves_a_process_holding_its_output_returns_when_it_exits() {
        let started = Instant::now();
        let out = run_within(sh("echo done; sleep 600 & exit 0"), Duration::from_secs(60)).await.unwrap();
        assert!(out.status.success());
        assert_eq!(String::from_utf8_lossy(&out.stdout), "done\n");
        assert!(started.elapsed() < Duration::from_secs(20), "took {:?}", started.elapsed());
    }

    #[tokio::test]
    async fn a_check_that_runs_past_its_limit_is_stopped_with_what_it_said() {
        let started = Instant::now();
        let said = run_within(sh("echo stuck at sign-in >&2; sleep 600"), Duration::from_secs(2)).await.unwrap_err();
        assert!(said.contains("ran past 2s") && said.contains("stuck at sign-in"), "{said}");
        assert!(started.elapsed() < Duration::from_secs(20), "took {:?}", started.elapsed());
    }
}
