//! The desktop app's screens, in a browser against a real appliance with a
//! real computer behind it.

// Test code: a failed unwrap is a failed test (AGENTS.md: none outside tests).
#![allow(clippy::unwrap_used, clippy::expect_used)]
use magnus_invariants::model::monitor;
use magnus_invariants::{mcp, *};
use serde_json::json;

const VIEW: &str = r#"<body><script>
addEventListener("message", e => {
  const m = e.data;
  if (m.id === 1) parent.postMessage({ jsonrpc: "2.0", method: "ui/notifications/initialized" }, "*");
  if (m.method === "ui/notifications/tool-result") document.body.textContent = "view got " + m.params.content[0].text.slice(0, 20);
});
parent.postMessage({ jsonrpc: "2.0", id: 1, method: "ui/initialize", params: {} }, "*");
</script>"#;

#[tokio::test(flavor = "multi_thread")]
async fn the_desktop_screens_work_on_a_real_computer() {
    let Ok(chromium) = std::env::var("MAGNUS_CHROMIUM") else { panic!("MAGNUS_CHROMIUM is not set") };
    let computerd = Computerd::start();
    let web = FakeWeb::start(&[("/reached", "x")]).await;
    let crm = mcp::crm().view("get_record", "ui://crm/record", VIEW).start().await;
    let page = format!(
        "Here.\n```magnus-html id=page\n<body><script>fetch(\"{}\").then(()=>document.body.textContent=\"reached\",()=>document.body.textContent=\"script ran\")</script>\n```",
        web.url("/reached")
    );
    let model = FakeModel::start(steps(vec![])).await;
    model.set_script(script(move |c| {
        let (text, turn, results) = (c.text(), c.turn(), c.last_results());
        let page = page.clone();
        async move {
            if text.contains("Make a page") {
                return Reply::Text(page);
            }
            if text.contains("Look at record 42") {
                let told = results.iter().find(|r| r.text.contains("app view="));
                return match told {
                    Some(r) => {
                        let id = r.text.split("app view=").nth(1).unwrap().split('`').next().unwrap();
                        Reply::Text(format!("Here.\n```magnus-ui\napp view={id}\n```"))
                    }
                    None => tool("crm__get_record", json!({ "id": "42" })),
                };
            }
            if text.contains("privately") {
                return if turn == 0 { contract_unchecked("say", "said") } else { complete("A private answer") };
            }
            match turn {
                0 => contract_unchecked("read", "read"),
                1 => tool("crm__get_record", json!({ "id": "held-by-monitor" })),
                _ => complete("done"),
            }
        }
    }));
    model.set_monitor(script(|c| {
        let hold = c.text().contains("held-by-monitor");
        async move {
            if hold {
                monitor("review", "this one needs a look")
            } else {
                monitor("allow", "fine")
            }
        }
    }));
    let spec = Spec::default()
        .app("crm", &crm, "public", "private", &[("get_record", "read")])
        .section("safety", json!({ "monitor_model": "anthropic/monitor-model" }))
        .computer(&computerd.socket);
    let token = spec.token("alice");
    let app = Appliance::start(spec, &model).await;
    let alice = app.api("alice");
    let held = alice.submit("Read record 77.").await;
    alice.wait_item(&held, "monitor").await;

    let script_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").join("desktop/tests/screens.mjs");
    let out = tokio::process::Command::new("node")
        .arg(script_path)
        .env("MAGNUS_URL", &app.base)
        .env("MAGNUS_TOKEN", &token)
        .env("MAGNUS_CHROMIUM", chromium)
        .envs(std::env::var("MAGNUS_SHOTS").ok().map(|s| ("MAGNUS_SHOTS", s)))
        .output()
        .await
        .unwrap();
    let said = format!("{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success(), "{said}\n{}", app.logs().lines().rev().take(30).collect::<Vec<_>>().join("\n"));
    assert!(said.contains("screens ok"), "{said}");
    assert!(web.hits().is_empty(), "agent-made HTML reached the network: {:?}", web.hits());
    alice.wait_status(&held, &["completed", "suspended"]).await;
}
