use super::personal::{Personal, CAPS};
use super::*;
use crate::team::Role;

/// A conversation of two entries: one the viewer may delete, one they may not.
struct Two;

impl Conversation for Two {
    type Entry = (&'static str, May);

    fn entries(&self) -> Vec<Self::Entry> {
        vec![("mine", May { delete: true, ..May::default() }), ("theirs", May::default())]
    }
    fn find(&self, id: &str) -> Option<Self::Entry> {
        self.entries().into_iter().find(|e| e.0 == id)
    }
    fn may(&self, e: &Self::Entry) -> Option<May> {
        Some(e.1)
    }
    fn speaker_name(&self, _: &Self::Entry) -> String {
        "Mo".into()
    }
    fn text(&self, e: &Self::Entry) -> String {
        format!("the {} words", e.0)
    }
    fn said_at(&self, _: &Self::Entry) -> Option<At> {
        None
    }
    fn caps(&self) -> Caps {
        CAPS
    }
}

/// Both conversations refuse alike: what the viewer does not have is a 404,
/// what `may` does not allow a 403 saying why.
#[test]
fn an_entry_not_had_is_404_and_one_not_allowed_403() {
    let c = Two;
    assert_eq!(c.act("nobody's", |m| m.delete, "no").map(|_| ()).unwrap_err().status(), StatusCode::NOT_FOUND);
    assert_eq!(c.act("theirs", |m| m.delete, "no").map(|_| ()).unwrap_err().status(), StatusCode::FORBIDDEN);
    assert_eq!(c.act("mine", |m| m.delete, "no").map(|e| e.0).ok(), Some("mine"));
    let file = c.export("theirs").unwrap();
    assert_eq!(file.headers()[CONTENT_DISPOSITION], "attachment; filename=\"the-theirs-words.md\"");
}

/// The personal chat has one person in it; a channel has several, and a
/// guest there has no Magnus of their own to whisper to.
#[test]
fn what_each_conversation_has_beside_its_entries() {
    assert_eq!(CAPS, Caps::default(), "the personal chat claims what only several people have");
    type Channel<'a> = channel::Channel<'a, crate::state::State>;
    let member = Channel::caps_for(Role::Member);
    assert!(member.several && member.reactions && member.pins && member.threads && member.whisper && member.typing);
    assert!(!Channel::caps_for(Role::Guest).whisper, "a guest may whisper to a Magnus of their own");
    let wire = serde_json::to_value(member).unwrap();
    for key in ["several", "reactions", "pins", "threads", "whisper", "typing"] {
        assert!(wire[key].is_boolean(), "caps has no {key}: {wire}");
    }
}

/// A person's thread is theirs alone: to anyone else, the server's owner and
/// an admin among them, it is not there, not its entries, not a branch of
/// it, not one exported.
#[tokio::test(flavor = "multi_thread")]
async fn a_personal_thread_gives_nothing_to_anyone_but_its_owner() {
    let s = crate::admin_app::tests::server_with("", |_| axum::Router::new()).await;
    let db = crate::log::connect(&s.k.cfg.database.url).await.unwrap();
    db.batch_execute(&crate::log::content_walls_sql("postgres")).await.unwrap();
    crate::chat_thread::golden_tests::fixture(&s.k).await;
    let mine = Personal::open(&s.k, "mo", "th1").await.ok().unwrap();
    assert!(mine.find("m:t1").is_some() && !mine.entries().is_empty());
    for other in ["sam", "ana"] {
        for thread in ["th1", "th2", "old-branch"] {
            let theirs = Personal::open(&s.k, other, thread).await.map(|_| ());
            assert_eq!(theirs.unwrap_err().status(), StatusCode::NOT_FOUND, "{other} has mo's {thread}");
        }
        let token = s.token(other).await;
        for path in ["/v1/chat/th1", "/v1/chat/th1/entries/m:t1/export"] {
            let (code, body) = s.call(&token, reqwest::Method::GET, path, None).await;
            assert_eq!(code, 404, "{path} gave {other} {body}");
        }
    }
    s.done().await;
}
