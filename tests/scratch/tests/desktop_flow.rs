//! The desktop app's frontend, driven in a real browser against a real
//! appliance: sign in (with a password, or through a real Keycloak),
//! connect Claude, type a task, approve in the inbox,
//! see the outcome with its evidence (`desktop/tests/flow.mjs`).

use magnus_invariants::appliance::workspace_root;
use magnus_invariants::canary;
use magnus_scratch::*;

/// How the person signs in for a run of the flow.
enum Sign {
    Password,
    Provider,
}

async fn run(how: Sign) {
    let mut f = Flow::start().await;
    let kc = Keycloak::start().await;
    kc.add_person(PERSON).await;
    let mut cmd = tokio::process::Command::new("node");
    match how {
        Sign::Password => {
            cmd.env("MAGNUS_PASSWORD", PASSWORD);
        }
        Sign::Provider => {
            // Someone with only a Keycloak account: no password set on the
            // appliance, so their first sign-in goes to Keycloak's page.
            f.app.db.admin().await.execute("DELETE FROM public.account_passwords WHERE person = $1", &[&PERSON]).await.unwrap();
            kc.configure(&mut f.app).await;
            cmd.env("MAGNUS_PROVIDER", IDP_LABEL).env("MAGNUS_PROVIDER_PASSWORD", password_of(PERSON));
        }
    }
    let key = canary("anthropic-key");
    let out = cmd
        .arg(workspace_root().join("desktop/tests/flow.mjs"))
        .env("MAGNUS_URL", &f.app.base)
        .env("MAGNUS_PERSON", PERSON)
        .env("MAGNUS_API_KEY", &key)
        .env("MAGNUS_TASK", TASK)
        .env("MAGNUS_CHAT_QUESTION", CHAT_QUESTION)
        .env("MAGNUS_CHAT_ANSWER", CHAT_ANSWER)
        .env("MAGNUS_CHROMIUM", chromium())
        .envs(shots())
        .output()
        .await
        .unwrap();
    let log = format!("{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success(), "the flow failed:\n{log}\n{}", f.report().await);
    f.check_outcome(&key).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_person_signs_in_connects_claude_approves_and_sees_evidence() {
    run(Sign::Password).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_person_signs_in_through_the_identity_provider_and_does_the_same() {
    run(Sign::Provider).await;
}
