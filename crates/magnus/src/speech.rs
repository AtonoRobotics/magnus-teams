//! Local speech to text with whisper.cpp, so voice never leaves the appliance.
//! The `[speech]` config names `whisper-cli` and a model; audio is converted
//! with ffmpeg. Read-aloud is the other way, with Kokoro (`[read_aloud]`).

use crate::config::{ReadAloud, Speech};
use anyhow::{anyhow, Context, Result};
use base64::Engine;
use std::path::Path;

pub async fn transcribe(speech: Option<&Speech>, state_dir: &Path, data_b64: &str) -> Result<String> {
    let cfg = speech.ok_or_else(|| anyhow!("voice input is not installed on this server"))?;
    let (bin, model) = (&cfg.whisper, &cfg.model);
    let audio = base64::engine::general_purpose::STANDARD.decode(data_b64).context("audio is not base64")?;
    let dir = state_dir.join("speech").join(uuid::Uuid::new_v4().simple().to_string());
    std::fs::create_dir_all(&dir)?;
    let input = dir.join("in.audio");
    let wav = dir.join("in.wav");
    std::fs::write(&input, audio)?;
    let ff = tokio::process::Command::new("ffmpeg")
        .args(["-loglevel", "error", "-y", "-i"])
        .arg(&input)
        .args(["-ar", "16000", "-ac", "1"])
        .arg(&wav)
        .output()
        .await
        .context("run ffmpeg")?;
    anyhow::ensure!(ff.status.success(), "could not read the audio: {}", String::from_utf8_lossy(&ff.stderr));
    let out = tokio::process::Command::new(&bin).arg("-m").arg(model).args(["-nt", "-np", "-f"]).arg(&wav).output().await.with_context(|| format!("run {}", bin.display()))?;
    let _ = std::fs::remove_dir_all(&dir);
    anyhow::ensure!(out.status.success(), "speech to text failed: {}", String::from_utf8_lossy(&out.stderr));
    Ok(String::from_utf8_lossy(&out.stdout).split_whitespace().collect::<Vec<_>>().join(" "))
}

/// The most text one request reads aloud: the app asks a sentence at a time.
pub const MAX_SPEAK: usize = 2000;
/// Read-aloud pieces made at once: each loads the voice's model.
const SPEAKING: usize = 2;

/// Why a piece was not read aloud.
#[derive(Debug)]
pub enum SpeakError {
    TooLong,
    Busy,
    TimedOut,
    Failed(anyhow::Error),
}

/// Text to speech on the appliance: MP3 of `text` in the configured voice,
/// at most `SPEAKING` at a time and each within its time.
pub async fn speak_limited(cfg: &ReadAloud, text: &str) -> std::result::Result<Vec<u8>, SpeakError> {
    static SLOTS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(SPEAKING);
    if text.chars().count() > MAX_SPEAK {
        return Err(SpeakError::TooLong);
    }
    let Ok(_slot) = SLOTS.try_acquire() else { return Err(SpeakError::Busy) };
    match tokio::time::timeout(std::time::Duration::from_secs(cfg.timeout_s), speak(cfg, text)).await {
        Ok(r) => r.map_err(SpeakError::Failed),
        // The child is killed as its future is dropped (kill_on_drop).
        Err(_) => Err(SpeakError::TimedOut),
    }
}

/// Text to speech on the appliance: MP3 of `text` in the configured voice.
pub async fn speak(cfg: &ReadAloud, text: &str) -> Result<Vec<u8>> {
    use tokio::io::AsyncWriteExt;
    let mut c = tokio::process::Command::new(&cfg.speak)
        .arg("--model")
        .arg(&cfg.model)
        .arg("--voices")
        .arg(&cfg.voices)
        .args(["--voice", &cfg.voice])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("run {}", cfg.speak.display()))?;
    let mut stdin = c.stdin.take().expect("piped");
    stdin.write_all(text.as_bytes()).await?;
    drop(stdin);
    let out = c.wait_with_output().await?;
    anyhow::ensure!(out.status.success() && !out.stdout.is_empty(), "read-aloud failed: {}", String::from_utf8_lossy(&out.stderr));
    Ok(out.stdout)
}
