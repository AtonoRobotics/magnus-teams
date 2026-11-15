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
         find . -type l | grep -q . && echo FOUND-LINK; echo FILES; find . -type f | sort"
    );
    let model = FakeModel::start(steps(vec![
        tool("open_attachment", json!({ "name": "image0.jpeg" })),
        tool("import_attachment", json!({ "name": "image0.jpeg" })),
        tool("import_attachment", json!({ "name": "evil.sh" })),
        tool("import_attachment", json!({ "name": "details.docm" })),
        tool("import_attachment", json!({ "name": "event.zip" })),
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

    let listed = result(6).text;
    assert!(listed.contains("JPEG image data"), "the imported copy is not a JPEG: {listed}");
    assert!(listed.contains("KEPT-TEXT"), "the document lost its text: {listed}");
    for bad in ["FOUND-HIDDEN", "FOUND-ZIP-HIDDEN", "FOUND-MACRO", "FOUND-EXECUTABLE", "FOUND-ESCAPE", "FOUND-LINK"] {
        assert!(!listed.contains(bad), "{bad}: something of an original came into the workspace: {listed}");
    }
    let files: Vec<&str> = listed.split("FILES").nth(1).unwrap_or_default().lines().map(str::trim).filter(|l| l.starts_with("./")).collect();
    let want = [
        "./details.docx",
        "./event/escape.txt",
        "./event/more/nested/guests.csv",
        "./event/notes.txt",
        "./event/photos/image1.jpg",
        "./event/setup.sh.txt",
        "./evil.sh.txt",
        "./image0.jpg",
    ];
    assert_eq!(files, want, "the workspace holds other files than the clean copies: {listed}");
    assert_eq!(alice.task(&task).await["taint"]["untrusted"], json!(true), "the run that took in files from outside is not tainted");
}
