//! Read-aloud in the appliance's own voice, through the HTTP API: the text
//! goes to the configured `speak` program on stdin, never as an argument, and
//! its MP3 comes back to anyone signed in. With no voice installed, 503, so
//! the app uses its system's own.

use base64::Engine;
use magnus_invariants::*;
use serde_json::json;
use std::os::unix::fs::PermissionsExt;

const TEXT: &str = "Revenue went up every quarter; nothing needs you today.";

/// A stand-in for Kokoro: records its arguments and its input, prints bytes.
fn fake_speak(dir: &std::path::Path) -> std::path::PathBuf {
    let prog = dir.join("speak");
    std::fs::write(&prog, format!("#!/bin/sh\necho \"$@\" > {0}/args\ncat > {0}/text\nprintf 'ID3 not really an mp3'\n", dir.display())).unwrap();
    std::fs::set_permissions(&prog, std::fs::Permissions::from_mode(0o755)).unwrap();
    prog
}

#[tokio::test(flavor = "multi_thread")]
async fn the_appliance_reads_text_aloud_in_its_own_voice() {
    let m = FakeModel::start(steps(vec![])).await;
    let mut app = Appliance::start(Spec::default(), &m).await;
    let alice = app.api("alice");
    let (st, v) = alice.post("/v1/speak", json!({ "text": TEXT })).await;
    assert_eq!(st, 503, "no voice is installed, yet: {v}");

    let dir = app.dir.path().to_path_buf();
    let prog = fake_speak(&dir);
    magnus_scratch::reconfigure(&mut app, |c| {
        let mut r = toml::Table::new();
        r.insert("speak".into(), prog.display().to_string().into());
        r.insert("model".into(), "/voice/kokoro.onnx".into());
        r.insert("voices".into(), "/voice/voices.bin".into());
        r.insert("voice".into(), "bf_emma".into());
        c.insert("read_aloud".into(), r.into());
    })
    .await;

    let mut nobody = app.api("alice");
    nobody.token = String::new();
    assert_eq!(nobody.post("/v1/speak", json!({ "text": TEXT })).await.0, 401);
    assert_eq!(alice.post("/v1/speak", json!({ "text": "  " })).await.0, 400);

    let (st, v) = alice.post("/v1/speak", json!({ "text": TEXT })).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(v["media_type"], "audio/mpeg");
    let audio = base64::engine::general_purpose::STANDARD.decode(v["audio_b64"].as_str().unwrap()).unwrap();
    assert_eq!(audio, b"ID3 not really an mp3");
    assert_eq!(std::fs::read_to_string(dir.join("text")).unwrap(), TEXT);
    assert_eq!(std::fs::read_to_string(dir.join("args")).unwrap().trim(), "--model /voice/kokoro.onnx --voices /voice/voices.bin --voice bf_emma");
}
