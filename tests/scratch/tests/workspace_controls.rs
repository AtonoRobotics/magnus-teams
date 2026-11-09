//! Workspace v1.2 controls on the real appliance/browser route. The provider
//! is a fixed UI fixture; these assertions make no agent reasoning claim.
use magnus_invariants::*;
fn workspace_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}
use magnus_scratch::*;

#[test]
fn markdown_is_safe_in_answers_and_composed_files() {
    let out = std::process::Command::new("node").arg(workspace_root().join("desktop/tests/markdown-check.mjs")).output().unwrap();
    assert!(out.status.success(), "{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
}

#[tokio::test(flavor = "multi_thread")]
async fn message_menus_tooltips_and_markdown_work_in_the_browser() {
    let m = FakeModel::start(script(|_| async {
        Reply::Text("# Readable answer\n\n**Bold** and [documentation](https://example.com).\n\n| Item | Value |\n| --- | --- |\n| One | Two |\n\n```js\nconst answer = 42;\n```\n\n<script>window.pwned=true</script> [unsafe](javascript:alert(1))".into())
    })).await;
    let app = Appliance::start(Spec::default(), &m).await;
    assert!(set_password(&app, "alice", PASSWORD));
    let out = tokio::process::Command::new("node")
        .arg(workspace_root().join("desktop/tests/workspace-controls.mjs"))
        .env("MAGNUS_URL", &app.base)
        .env("MAGNUS_PERSON", "alice")
        .env("MAGNUS_PASSWORD", PASSWORD)
        .env("MAGNUS_CHROMIUM", chromium())
        .envs(shots())
        .output()
        .await
        .unwrap();
    assert!(out.status.success(), "{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    println!("{}", String::from_utf8_lossy(&out.stdout));
}
