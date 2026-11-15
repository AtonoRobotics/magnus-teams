//! Every file from outside reaches the run's work safely with no person
//! asked (owner, 2026-11-14: "All file types need a secure and safe solution
//! for being downloaded and used by an agent"). A run asked for a flier from
//! an email's photos could only read their file type. Now the agent looks at
//! a picture, and `import_attachment` puts copies rebuilt from their content
//! alone in the run's workspace: a script hidden after a JPEG's end, a
//! document's macros, an archive's links, paths out and programs do not come
//! with them, and a script comes in as text.
//! Needs a real `magnus-computerd` (KVM, the guest images), as on Strix Beta.
//! The fixtures were made with Python's standard library: `details.docm`, a
//! Word document with a VBA part, and `event.zip`, holding a photo with a
//! script after it, text, a script, `../escape.txt`, a link to /etc/passwd,
//! a program (`true`) and a nested zip with a CSV.

use base64::Engine;
use magnus_invariants::*;
use serde_json::json;

/// An 8x8 JPEG.
const PHOTO_B64: &str = "/9j/4AAQSkZJRgABAQAAAQABAAD/2wBDAAYEBQYFBAYGBQYHBwYIChAKCgkJChQODwwQFxQYGBcUFhYaHSUfGhsjHBYWICwgIyYnKSopGR8tMC0oMCUoKSj/2wBDAQcHBwoIChMKChMoGhYaKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCj/wAARCAAIAAgDASIAAhEBAxEB/8QAHwAAAQUBAQEBAQEAAAAAAAAAAAECAwQFBgcICQoL/8QAtRAAAgEDAwIEAwUFBAQAAAF9AQIDAAQRBRIhMUEGE1FhByJxFDKBkaEII0KxwRVS0fAkM2JyggkKFhcYGRolJicoKSo0NTY3ODk6Q0RFRkdISUpTVFVWV1hZWmNkZWZnaGlqc3R1dnd4eXqDhIWGh4iJipKTlJWWl5iZmqKjpKWmp6ipqrKztLW2t7i5usLDxMXGx8jJytLT1NXW19jZ2uHi4+Tl5ufo6erx8vP09fb3+Pn6/8QAHwEAAwEBAQEBAQEBAQAAAAAAAAECAwQFBgcICQoL/8QAtREAAgECBAQDBAcFBAQAAQJ3AAECAxEEBSExBhJBUQdhcRMiMoEIFEKRobHBCSMzUvAVYnLRChYkNOEl8RcYGRomJygpKjU2Nzg5OkNERUZHSElKU1RVVldYWVpjZGVmZ2hpanN0dXZ3eHl6goOEhYaHiImKkpOUlZaXmJmaoqOkpaanqKmqsrO0tba3uLm6wsPExcbHyMnK0tPU1dbX2Nna4uPk5ebn6Onq8vP09fb3+Pn6/9oADAMBAAIRAxEAPwDk6KKK5j9PP//Z";
const DOCM: &[u8] = include_bytes!("../fixtures/attachments/details.docm");
const ZIP: &[u8] = include_bytes!("../fixtures/attachments/event.zip");
// Made with ffmpeg and p7zip on Debian bookworm: a second of tone and of
// test pattern, each titled META-CANARY-..., and a 7z holding a photo with a
// script after it, a CSV, a script and a link to /etc/passwd.
const WAV: &[u8] = include_bytes!("../fixtures/attachments/tone.wav");
const MOV: &[u8] = include_bytes!("../fixtures/attachments/clip.mov");
const SEVEN: &[u8] = include_bytes!("../fixtures/attachments/bundle.7z");
/// Bytes of no kind `file` names ("data"), as an unknown database would be.
const UNKNOWN: &[u8] = &[0x00, 0x9c, 0x13, 0xfe, 0x01, 0x88, 0x00, 0x7f, 0xd2, 0x00, 0x41, 0xe7, 0x00, 0x05, 0xb0, 0x3a];

#[tokio::test(flavor = "multi_thread")]
async fn every_kind_of_file_from_outside_is_imported_clean() {
    let b64 = base64::engine::general_purpose::STANDARD;
    let hidden = canary("hidden-script");
    // Split, so the check's command does not contain what it looks for.
    let (head, tail) = hidden.split_at(hidden.len() / 2);
    let mut photo = b64.decode(PHOTO_B64).unwrap();
    photo.extend(format!("\n#!/bin/sh\necho {hidden}\n").into_bytes());
    let check = format!(
        "cd /workspace/from_outside; file -b image0.jpg; A={head}; B={tail}; grep -qa \"$A$B\" image0.jpg && echo FOUND-HIDDEN; \
         grep -qa HIDDEN-IN-ZIP event/photos/image1.jpg && echo FOUND-ZIP-HIDDEN; unzip -l details.docx | grep -qi vba && echo FOUND-MACRO; \
         unzip -p details.docx word/document.xml | grep -q Riverside && echo KEPT-TEXT; \
         find . -type f -perm /111 | grep -q . && echo FOUND-EXECUTABLE; ls /workspace | grep -q escape && echo FOUND-ESCAPE; \
         find . -type l | grep -q . && echo FOUND-LINK; grep -qa META-CANARY tone.mp3 clip.mp4 && echo FOUND-METADATA; \
         grep -qa HIDDEN-IN-7Z bundle/pics/image2.jpg && echo FOUND-7Z-HIDDEN; echo AUDIO $(file -b tone.mp3); echo VIDEO $(file -b clip.mp4); \
         echo FILES; find . -type f | sort"
    );
    let model = FakeModel::start(steps(vec![
        tool("open_attachment", json!({ "name": "image0.jpeg" })),
        tool("import_attachment", json!({ "name": "image0.jpeg" })),
        tool("import_attachment", json!({ "name": "evil.sh" })),
        tool("import_attachment", json!({ "name": "details.docm" })),
        tool("import_attachment", json!({ "name": "event.zip" })),
        tool("import_attachment", json!({ "name": "tone.wav" })),
        tool("import_attachment", json!({ "name": "clip.mov" })),
        tool("import_attachment", json!({ "name": "bundle.7z" })),
        tool("open_attachment", json!({ "name": "records.db" })),
        tool("import_attachment", json!({ "name": "records.db" })),
        tool("sandbox_exec", json!({ "command": check })),
        complete("Made the flier."),
    ]))
    .await;
    let computerd = Computerd::start();
    let app = Appliance::start(Spec::default().computer(&computerd.socket), &model).await;
    let alice = app.api("alice");
    let outside = json!([
        { "name": "image0.jpeg", "media_type": "image/jpeg", "data_b64": b64.encode(&photo) },
        { "name": "evil.sh", "media_type": "text/x-shellscript", "data_b64": b64.encode(format!("#!/bin/sh\necho {hidden}\n")) },
        { "name": "details.docm", "media_type": "application/vnd.ms-word.document.macroEnabled.12", "data_b64": b64.encode(DOCM) },
        { "name": "event.zip", "media_type": "application/zip", "data_b64": b64.encode(ZIP) },
        { "name": "tone.wav", "media_type": "audio/wav", "data_b64": b64.encode(WAV) },
        { "name": "clip.mov", "media_type": "video/quicktime", "data_b64": b64.encode(MOV) },
        { "name": "bundle.7z", "media_type": "application/x-7z-compressed", "data_b64": b64.encode(SEVEN) },
        { "name": "records.db", "media_type": "application/octet-stream", "data_b64": b64.encode(UNKNOWN) },
    ]);
    let (code, made) = alice.post("/v1/tasks", json!({ "text": "Make a flier from the photos and details in this email.", "from_outside": outside })).await;
    assert!(code.is_success(), "{made}");
    let task = made["task_id"].as_str().unwrap().to_string();
    let t = alice.wait_settled(&task).await;
    assert_eq!(t["status"], "completed", "{t}");
    let result = |turn: usize| model.at_turn(turn).unwrap_or_else(|| panic!("the run stopped before turn {turn}")).last_results()[0].clone();

    let seen = model.at_turn(1).expect("the run stopped after opening");
    let opened = result(1);
    assert!(!opened.is_error && opened.text.contains("is a picture (8x8 pixels)"), "the photo was not shown as a picture: {}", opened.text);
    assert!(seen.text().contains("image/jpeg"), "the model was not shown the picture");

    let photo = result(2);
    assert!(!photo.is_error && photo.text.contains("/workspace/from_outside/image0.jpg (picture"), "the photo was not imported: {}", photo.text);
    let script = result(3);
    assert!(!script.is_error && script.text.contains("evil.sh.txt (text"), "a script did not come in as text: {}", script.text);
    let doc = result(4);
    assert!(!doc.is_error && doc.text.contains("details.docx (document"), "the document was not re-saved without macros: {}", doc.text);
    let archive = result(5);
    assert!(!archive.is_error, "the archive was not imported: {}", archive.text);
    for left_out in ["a program", "a link"] {
        assert!(archive.text.contains(left_out), "the archive's {left_out} was not said to be left out: {}", archive.text);
    }

    let audio = result(6);
    assert!(!audio.is_error && audio.text.contains("tone.mp3 (audio"), "the audio was not re-encoded: {}", audio.text);
    let video = result(7);
    assert!(!video.is_error && video.text.contains("clip.mp4 (video"), "the video was not re-encoded: {}", video.text);
    let seven = result(8);
    assert!(!seven.is_error && seven.text.contains("a link"), "the 7z was not unpacked without its link: {}", seven.text);

    // A kind no tool here reads is neither opened nor brought in (owner,
    // 2026-11-14: "If the agent can't read the file there is no reason to
    // download or open it").
    let unread = result(9);
    assert!(unread.is_error && unread.text.contains("was not opened") && unread.text.contains("cannot read"), "an unreadable file was opened: {}", unread.text);
    let unbrought = result(10);
    assert!(unbrought.is_error && unbrought.text.contains("cannot read"), "an unreadable file was brought in: {}", unbrought.text);

    let listed = result(11).text;
    assert!(listed.contains("JPEG image data"), "the imported copy is not a JPEG: {listed}");
    assert!(listed.contains("KEPT-TEXT"), "the document lost its text: {listed}");
    let line = |tag: &str| listed.lines().find(|l| l.starts_with(tag)).unwrap_or_default().to_string();
    assert!(line("AUDIO").contains("Audio file") || line("AUDIO").contains("MPEG ADTS, layer III"), "the audio is not an MP3: {listed}");
    assert!(line("VIDEO").contains("ISO Media"), "the video is not an MP4: {listed}");
    for bad in ["FOUND-HIDDEN", "FOUND-ZIP-HIDDEN", "FOUND-7Z-HIDDEN", "FOUND-METADATA", "FOUND-MACRO", "FOUND-EXECUTABLE", "FOUND-ESCAPE", "FOUND-LINK"] {
        assert!(!listed.contains(bad), "{bad}: something of an original came into the workspace: {listed}");
    }
    let files: Vec<&str> = listed.split("FILES").nth(1).unwrap_or_default().lines().map(str::trim).filter(|l| l.starts_with("./")).collect();
    let want = [
        "./bundle/hosts.csv",
        "./bundle/install.sh.txt",
        "./bundle/pics/image2.jpg",
        "./clip.mp4",
        "./details.docx",
        "./event/escape.txt",
        "./event/more/nested/guests.csv",
        "./event/notes.txt",
        "./event/photos/image1.jpg",
        "./event/setup.sh.txt",
        "./evil.sh.txt",
        "./image0.jpg",
        "./tone.mp3",
    ];
    assert_eq!(files, want, "the workspace holds other files than the clean copies: {listed}");
    assert_eq!(alice.task(&task).await["taint"]["untrusted"], json!(true), "the run that took in files from outside is not tainted");
}

/// A fetched attachment is a temporary file (owner, 2026-11-14): its bytes
/// are never written to the event log or its content, the run reads it
/// while it works, and when the run ends its bytes are gone.
#[tokio::test(flavor = "multi_thread")]
async fn a_fetched_attachment_is_held_only_while_its_run_works() {
    let b64 = base64::engine::general_purpose::STANDARD;
    let inside = canary("attachment-text");
    let bytes = b64.encode(format!("Agenda for Saturday: {inside}\n"));
    let mail = mcp::mail().tool("get_attachment", "Fetch one attachment.", json!({ "type": "object", "properties": {} }), {
        let bytes = bytes.clone();
        move |_, _, _| mcp::ok(json!({ "name": "plan.txt", "media_type": "text/plain", "data_b64": bytes }))
    });
    let mail = mail.start().await;
    let model = FakeModel::start(steps(vec![
        tool("mail__get_attachment", json!({ "message_id": "m1", "filename": "plan.txt" })),
        tool("open_attachment", json!({ "name": "plan.txt" })),
        complete("Read the plan."),
    ]))
    .await;
    let computerd = Computerd::start();
    let spec = Spec::default().computer(&computerd.socket).app("mail", &mail, "public", "private", &[("get_attachment", "read")]).filter_login_codes("mail");
    let app = Appliance::start(spec, &model).await;
    let alice = app.api("alice");
    let task = alice.submit("Read the plan attached to my email.").await;
    let t = alice.wait_settled(&task).await;
    assert_eq!(t["status"], "completed", "{t}");

    let fetched = &model.at_turn(1).expect("the run stopped after fetching").last_results()[0];
    assert!(fetched.text.contains("Attachments from outside: plan.txt") && !fetched.text.contains(&bytes), "the fetch was not lifted: {}", fetched.text);
    let opened = &model.at_turn(2).expect("the run stopped after opening").last_results()[0];
    assert!(!opened.is_error && opened.text.contains(&inside), "the run could not read what it fetched: {}", opened.text);

    let admin = app.db.admin().await;
    let like = format!("%{}%", &bytes[..bytes.len().min(40)]);
    for table in ["events", "content"] {
        let rows = admin.query(&format!("SELECT count(*) FROM {table} t WHERE t::text LIKE $1"), &[&like]).await.unwrap();
        let n: i64 = rows[0].get(0);
        assert_eq!(n, 0, "the attachment's bytes were written to {table}");
    }
    let (code, said) = alice.post(&format!("/v1/tasks/{task}/attachments/plan.txt/save"), json!({})).await;
    assert_eq!(code.as_u16(), 404, "the attachment's bytes outlived the run: {said}");
}
