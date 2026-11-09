//! The screen, camera and microphone, reached only when a task asks. Each is
//! run on its own so the operating system asks the person at that moment.
use crate::Folders;
use anyhow::{bail, Context, Result};
use base64::Engine;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

const CAPTURE_LIMIT: Duration = Duration::from_secs(40);
const MAX_RECORD_SECONDS: u64 = 30;
const DEFAULT_RECORD_SECONDS: u64 = 5;
/// Prefixed to an image's base64, which the server turns back into an image.
pub const IMAGE: &str = "IMAGE:";
const MAX_IMAGE_BYTES: usize = 384 * 1024;

fn linux_screen(out: &str) -> Vec<Vec<String>> {
    [vec!["grim", out], vec!["gnome-screenshot", "-f", out], vec!["import", "-window", "root", out]].iter().map(|c| c.iter().map(|s| s.to_string()).collect()).collect()
}

/// The commands to try, in order, for one kind of capture on one system.
fn commands(kind: &str, os: &str, out: &str, seconds: u64) -> Vec<Vec<String>> {
    let v = |c: &[&str]| vec![c.iter().map(|s| s.to_string()).collect::<Vec<String>>()];
    let t = seconds.to_string();
    match (kind, os) {
        ("screen", "macos") => v(&["screencapture", "-x", "-t", "jpg", out]),
        ("screen", "linux") => linux_screen(out),
        ("camera", "macos") => v(&["ffmpeg", "-y", "-loglevel", "error", "-f", "avfoundation", "-framerate", "30", "-i", "0", "-frames:v", "1", out]),
        ("camera", "linux") => v(&["ffmpeg", "-y", "-loglevel", "error", "-f", "v4l2", "-i", "/dev/video0", "-frames:v", "1", out]),
        ("microphone", "macos") => v(&["ffmpeg", "-y", "-loglevel", "error", "-f", "avfoundation", "-i", ":0", "-t", &t, out]),
        ("microphone", "linux") => v(&["ffmpeg", "-y", "-loglevel", "error", "-f", "pulse", "-i", "default", "-t", &t, out]),
        _ => vec![],
    }
}

async fn first_that_works(tried: Vec<Vec<String>>) -> Result<()> {
    let mut why = String::from("this computer has no tool for it");
    for c in &tried {
        let Some((program, args)) = c.split_first() else { continue };
        let run = tokio::process::Command::new(program).args(args).stdin(Stdio::null()).kill_on_drop(true).output();
        match tokio::time::timeout(CAPTURE_LIMIT, run).await {
            Ok(Ok(out)) if out.status.success() => return Ok(()),
            Ok(Ok(out)) => why = format!("{program}: {}", String::from_utf8_lossy(&out.stderr)),
            Ok(Err(e)) => why = format!("{program}: {e}"),
            Err(_) => bail!("{program} ran past {CAPTURE_LIMIT:?} and was stopped"),
        }
    }
    bail!("the capture did not work: {why}")
}

/// An image comes back to the agent. A recording stays on this computer, in
/// the first granted folder, because the agent cannot listen to it.
pub async fn capture(kind: &str, seconds: Option<u64>, folders: &Folders) -> Result<String> {
    let os = std::env::consts::OS;
    let seconds = seconds.unwrap_or(DEFAULT_RECORD_SECONDS).clamp(1, MAX_RECORD_SECONDS);
    if kind == "microphone" {
        let dir = folders.first()?;
        let out = dir.join(format!("recording-{}.wav", uuid::Uuid::new_v4().simple()));
        first_that_works(commands(kind, os, &out.display().to_string(), seconds)).await?;
        return Ok(format!("Recorded {seconds} seconds to {}", out.display()));
    }
    let dir = tempfile::tempdir()?;
    let out = dir.path().join("capture.jpg");
    first_that_works(commands(kind, os, &out.display().to_string(), seconds)).await?;
    encoded(&out)
}

fn encoded(file: &Path) -> Result<String> {
    let bytes = std::fs::read(file).context("the capture left no file")?;
    if bytes.len() > MAX_IMAGE_BYTES {
        bail!("the capture is {} bytes, more than {MAX_IMAGE_BYTES} can be sent", bytes.len());
    }
    Ok(format!("{IMAGE}{}", base64::engine::general_purpose::STANDARD.encode(bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_kind_has_commands_on_each_system_and_nothing_else_does() {
        for os in ["linux", "macos"] {
            for kind in ["screen", "camera", "microphone"] {
                assert!(!commands(kind, os, "/tmp/o", 5).is_empty(), "{kind} on {os}");
            }
        }
        assert!(commands("keychain", "linux", "/tmp/o", 5).is_empty());
    }

    #[test]
    fn recording_length_goes_to_the_tool() {
        let c = commands("microphone", "linux", "/tmp/o", 7);
        assert!(c[0].windows(2).any(|w| w[0] == "-t" && w[1] == "7"), "{c:?}");
    }

    #[test]
    fn too_big_an_image_is_refused_rather_than_cut() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("big");
        std::fs::write(&f, vec![0u8; MAX_IMAGE_BYTES + 1]).unwrap();
        assert!(encoded(&f).is_err());
        std::fs::write(&f, b"x").unwrap();
        assert!(encoded(&f).unwrap().starts_with(IMAGE));
    }
}
