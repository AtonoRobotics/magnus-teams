//! The independent checks a boundary action meets, played by the scripted
//! model (decisions/2026-11-08-auto-review.md, 2026-11-09-safety-checks.md,
//! 2026-11-10-permissions-asks.md). Every boundary action the person's own
//! answer does not cover goes to Auto Review, which first asks the outgoing
//! classifier. A check that cannot answer asks the person, so a test whose
//! model answers them as if they were its own run sees an ask where its
//! action should go ahead.
//!
//! The checks are played honestly and add no authority of their own. The
//! classifier reports what the content plainly carries. Auto Review judges
//! whether the action is within what the person asked (spec rev 140): in
//! these tests every action is, unless its content carries `OUTSIDE_THE_ASK`,
//! which a test uses to make the one ask Auto Review raises. What the kernel
//! decides before Auto Review (a payment, an ask-first rule, the floor, a
//! hard denial, a frozen contract, the kill switch) never reaches it.

use magnus_invariants::model::Script;
use magnus_invariants::{script, tool, Conversation, FakeModel, Reply};
use serde_json::{json, Value};

/// The start of Auto Review's instructions.
pub const AUTO_REVIEW: &str = "You are Auto Review";
/// The start of the outgoing classifier's instructions.
pub const CLASSIFIER: &str = "You scan outgoing content for sensitive data";
/// Content that marks an action as outside what the person asked: Auto
/// Review asks the person about it (spec rev 140, case 1).
pub const OUTSIDE_THE_ASK: &str = "nothing the person asked for";
/// What the classifier calls private: the revenue figure the tests use.
const PRIVATE_FIGURE: &str = "41.7M";

/// A request's system prompt, as one string.
pub fn system_of(c: &Conversation) -> String {
    match &c.body["system"] {
        Value::String(s) => s.clone(),
        Value::Array(a) => a.iter().filter_map(|b| b["text"].as_str()).collect::<Vec<_>>().join("\n"),
        other => other.to_string(),
    }
}

/// The JSON document a check is given: the text of its one message.
fn document(c: &Conversation) -> Value {
    let first = c.messages().first().cloned().unwrap_or_default();
    let text = match &first["content"] {
        Value::String(s) => s.clone(),
        Value::Array(a) => a.iter().filter_map(|b| b["text"].as_str()).collect::<Vec<_>>().join("\n"),
        _ => String::new(),
    };
    serde_json::from_str(&text).unwrap_or(Value::Null)
}

/// The reply of the check this request is for, or `None` when it is not one.
pub fn check_reply(c: &Conversation) -> Option<Reply> {
    let sys = system_of(c);
    if sys.contains(CLASSIFIER) {
        let content = document(c)["content"].to_string();
        return Some(if content.contains(PRIVATE_FIGURE) {
            tool("classify", json!({ "sensitive": true, "finding": "a private business figure (Q3 revenue)" }))
        } else {
            tool("classify", json!({ "sensitive": false, "finding": "" }))
        });
    }
    if sys.contains(AUTO_REVIEW) {
        let doc = document(c);
        let standing = doc["standing"].as_str().unwrap_or_default().to_string();
        return Some(if doc["action"]["content"].to_string().contains(OUTSIDE_THE_ASK) {
            tool("review", json!({ "decision": "ask", "reason": "This is not what the person asked for." }))
        } else {
            tool("review", json!({ "decision": "allow", "reason": format!("The action is within the request: {standing}.") }))
        });
    }
    None
}

/// A scripted model whose independent checks are answered by `check_reply`.
pub async fn start_checked(s: Script) -> FakeModel {
    FakeModel::start(with_checks(s)).await
}

/// `inner` with the independent checks answered by `check_reply`: their
/// requests never reach it, so its turns are its own run's.
pub fn with_checks(inner: Script) -> Script {
    script(move |c| {
        let inner = inner.clone();
        async move {
            match check_reply(&c) {
                Some(r) => r,
                None => inner(c).await,
            }
        }
    })
}
