//! What a member sees, from the ledger and each request's outcome alone: no
//! server, no database. sam owns the workspace, mo is a member, ana a guest.
//! Each rule is held here once, for every route that shows it, including
//! the places the routes' own copies used to disagree.
use super::chat::Chat;
use super::items::{said, timeline};
use super::member_view::{MemberView, Outcome, Said};
use super::notices::of_workspace;
use super::search::find;
use super::{Change, Ledger, Message, Role};
use crate::state::Status;
use chrono::{Duration, Utc};
use std::collections::BTreeMap;

type At = chrono::DateTime<chrono::Utc>;

struct World {
    ledger: Ledger,
    runs: BTreeMap<String, Outcome>,
    t0: At,
}

impl World {
    fn new() -> World {
        let mut ledger = Ledger::default();
        ledger.apply(&Change::Created { workspace: "w".into(), name: "Team".into(), owner: "sam".into() });
        ledger.apply(&Change::Member { workspace: "w".into(), person: "mo".into(), role: Role::Member });
        ledger.apply(&Change::Member { workspace: "w".into(), person: "ana".into(), role: Role::Guest });
        ledger.apply(&Change::Channel { workspace: "w".into(), channel: "c".into(), name: "general".into() });
        World { ledger, runs: BTreeMap::new(), t0: Utc::now() - Duration::hours(1) }
    }

    fn at(&self, secs: i64) -> At {
        self.t0 + Duration::seconds(secs)
    }

    fn chat(&mut self, chat: Chat) {
        self.ledger.apply(&Change::Chat { workspace: "w".into(), chat });
    }

    fn say(&mut self, channel: &str, id: &str, by: &str, text: &str, thread: Option<&str>, secs: i64) {
        let message = Message { id: id.into(), sender: by.into(), text: text.into(), thread: thread.map(String::from), at: self.at(secs) };
        self.ledger.apply(&Change::Message { workspace: "w".into(), channel: channel.into(), message });
    }

    /// `requester` asked in `channel` with message `asked`; it ended `secs` in.
    fn answered(&mut self, channel: &str, task: &str, requester: &str, asked: Option<&str>, text: &str, private: bool, secs: i64) {
        let run = Change::Run {
            workspace: "w".into(),
            channel: channel.into(),
            thread: None,
            task: task.into(),
            requester: requester.into(),
            history: String::new(),
            sub: false,
            outside: false,
        };
        self.ledger.apply(&run);
        if let Some(m) = asked {
            self.chat(Chat::Asked { channel: channel.into(), message: m.into(), task: task.into() });
        }
        let outcome =
            Outcome { status: Status::Completed, summary: Some(text.into()), reason: None, private, started: self.at(secs - 1), ended: Some(self.at(secs)), steps: 1, doing: None };
        self.runs.insert(task.into(), outcome);
    }

    fn view<'a>(&'a self, person: &'a str) -> MemberView<'a, BTreeMap<String, Outcome>> {
        MemberView::new(&self.ledger, self.ledger.workspace("w").unwrap(), person, &self.runs)
    }

    fn unsend(&mut self, channel: &str, id: &str, sender: &str, secs: i64) {
        let at = self.at(secs);
        self.chat(Chat::Unsent { channel: channel.into(), id: id.into(), sender: sender.into(), thread: None, at });
    }
}

fn ids<'a>(m: impl IntoIterator<Item = &'a Message>) -> Vec<String> {
    m.into_iter().map(|m| m.id.clone()).collect()
}

fn texts(items: &[super::items::TeamItem]) -> Vec<String> {
    items.iter().map(|i| i.text.clone()).collect()
}

/// A direct conversation is its people's, a project's channel its members':
/// to anyone else there is nothing in it, not its messages, work or answers.
#[test]
fn a_channel_is_seen_by_its_people_only() {
    let mut x = World::new();
    x.ledger.apply(&Change::Channel { workspace: "w".into(), channel: "d".into(), name: String::new() });
    x.chat(Chat::Direct { channel: "d".into(), people: vec!["sam".into(), "mo".into()], agent: false });
    x.say("d", "m1", "sam", "between us", None, 1);
    x.answered("d", "t1", "sam", Some("m1"), "the answer", false, 2);
    let def = super::knowledge::Project { id: "p".into(), name: "P".into(), owner: "sam".into(), ..Default::default() };
    x.ledger.apply(&Change::Project { workspace: "w".into(), id: "p".into(), def });
    x.ledger.apply(&Change::Channel { workspace: "w".into(), channel: "pc".into(), name: "deal".into() });
    x.ledger.apply(&Change::ChannelProject { workspace: "w".into(), channel: "pc".into(), project: Some("p".into()) });
    x.say("pc", "m2", "sam", "the price is 4m", None, 3);
    let now = x.at(10);
    assert_eq!(ids(x.view("mo").messages("d")), ["m1"]);
    for (who, channel) in [("ana", "d"), ("mo", "pc"), ("ana", "pc")] {
        let v = x.view(who);
        assert!(!v.sees(channel), "{who} sees {channel}");
        assert!(v.messages(channel).is_empty() && v.places(channel).is_empty() && v.answers(channel, now).is_empty() && v.work(channel, now).is_empty(), "{who} reads {channel}");
        assert!(timeline(&v, channel, now).is_empty() && said(&v, channel, now).is_empty());
        assert!(v.message(channel, "m1", now).is_none() && v.message(channel, "agent:t1", now).is_none());
        assert!(!v.channels().any(|(id, _)| id == channel), "{who} is shown {channel} in their list");
    }
    assert_eq!(x.view("ana").channels().map(|(id, _)| id.as_str()).collect::<Vec<_>>(), ["c"]);
}

/// What a person deleted a conversation for themselves up to is gone for
/// them, answers included; the others keep it.
#[test]
fn a_conversation_cleared_for_oneself_is_gone_for_them_alone() {
    let mut x = World::new();
    x.say("c", "m1", "sam", "old news", None, 1);
    x.answered("c", "t1", "mo", None, "an old answer", false, 2);
    let mark = x.at(3);
    x.chat(Chat::Cleared { channel: "c".into(), person: "mo".into(), at: mark });
    x.say("c", "m2", "sam", "fresh", None, 4);
    let now = x.at(10);
    let mo = x.view("mo");
    assert_eq!(ids(mo.messages("c")), ["m2"]);
    assert!(mo.answers("c", now).is_empty() && mo.message("c", "agent:t1", now).is_none() && mo.message("c", "m1", now).is_none());
    assert_eq!(texts(&timeline(&mo, "c", now)), ["fresh"]);
    assert_eq!(texts(&said(&mo, "c", now)), ["fresh"]);
    let sam = x.view("sam");
    assert_eq!(texts(&said(&sam, "c", now)), ["old news", "an old answer", "fresh"]);
}

/// A message or answer deleted for everyone is gone everywhere: from the
/// list, from a single look-up, and from what was said. Only its place stays,
/// in the channel view, while replies hang from it.
#[test]
fn what_was_deleted_for_everyone_is_gone_for_everyone() {
    let mut x = World::new();
    x.say("c", "m1", "mo", "the door code is 4471", None, 1);
    x.say("c", "r1", "sam", "noted", Some("m1"), 2);
    x.say("c", "m2", "mo", "lonely", None, 3);
    x.answered("c", "t1", "mo", None, "The door code is 4471.", false, 4);
    x.unsend("c", "m1", "mo", 1);
    x.unsend("c", "m2", "mo", 3);
    x.unsend("c", "agent:t1", "agent:mo", 4);
    let now = x.at(10);
    for who in ["sam", "mo", "ana"] {
        let v = x.view(who);
        assert_eq!(ids(v.messages("c")), ["r1"], "{who}");
        assert!(v.answers("c", now).is_empty(), "{who} still reads a deleted answer");
        for id in ["m1", "m2", "agent:t1"] {
            assert!(v.message("c", id, now).is_none(), "{who} can still act on deleted {id}");
        }
        let shown = timeline(&v, "c", now);
        let m1 = shown.iter().find(|i| i.id == "m1").expect("a deleted message with replies leaves its place");
        assert_eq!((m1.deleted, m1.replies, m1.text.as_str()), (Some(true), Some(1), ""));
        assert!(!shown.iter().any(|i| i.id == "m2" || i.id == "agent:t1"), "{who}: a deleted message with no replies left something");
        assert_eq!(texts(&said(&v, "c", now)), ["noted"], "{who}");
    }
}

/// The legacy messages route's list once showed an answer deleted for
/// everyone; the channel view hid it. Both now read the same view.
#[test]
fn what_was_said_hides_a_deleted_answer_as_the_view_does() {
    let mut x = World::new();
    x.answered("c", "t1", "mo", None, "The door code is 4471.", false, 4);
    let now = x.at(10);
    assert_eq!(texts(&said(&x.view("sam"), "c", now)), ["The door code is 4471."]);
    x.unsend("c", "agent:t1", "agent:mo", 4);
    assert!(said(&x.view("sam"), "c", now).is_empty(), "a deleted answer is still listed");
}

/// An answer built from data only its requester may see is theirs: the
/// others are told only that it answered them, everywhere it shows.
#[test]
fn a_private_answer_reads_in_full_to_its_requester_alone() {
    let mut x = World::new();
    x.say("c", "m1", "mo", "@Magnus what did my inbox say", None, 1);
    x.answered("c", "t1", "mo", Some("m1"), "your inbox says the deal closes Friday", true, 2);
    let now = x.at(10);
    let Some(Said::Answer(mine)) = x.view("mo").message("c", "agent:t1", now) else { panic!("mo cannot find their answer") };
    assert!(mine.private && mine.text.contains("Friday"));
    for who in ["sam", "ana"] {
        let v = x.view(who);
        let a = v.message("c", "agent:t1", now).unwrap_or_else(|| panic!("{who} is not told it answered"));
        assert!(a.private() && !a.text().contains("Friday") && a.text().contains("mo directly"), "{who}: {}", a.text());
        assert!(!format!("{:?}", timeline(&v, "c", now)).contains("Friday"), "{who} reads it in the view");
        assert!(!format!("{:?}", said(&v, "c", now)).contains("Friday"), "{who} reads it in what was said");
    }
}

/// Notices once told a person of an answer from before they deleted the
/// conversation for themselves, and of one deleted for everyone; messages
/// they were not told of. Now both follow the view.
#[test]
fn notices_follow_what_the_person_sees() {
    let mut x = World::new();
    let after = x.at(0);
    x.say("c", "m1", "sam", "before", None, 1);
    x.answered("c", "t1", "mo", None, "an answer before", false, 2);
    x.chat(Chat::Cleared { channel: "c".into(), person: "mo".into(), at: x.at(3) });
    x.say("c", "m2", "sam", "after", None, 4);
    x.answered("c", "t2", "mo", None, "an answer after", false, 5);
    x.answered("c", "t3", "mo", None, "an answer deleted", false, 6);
    x.unsend("c", "agent:t3", "agent:mo", 6);
    let told: Vec<String> = of_workspace(&x.view("mo"), "Mo", after, x.at(10)).into_iter().map(|n| n.preview).collect();
    assert_eq!(told, ["after", "an answer after"]);
    let sam: Vec<String> = of_workspace(&x.view("sam"), "Sam", after, x.at(10)).into_iter().map(|n| n.preview).collect();
    assert!(sam.is_empty(), "sam was told of his own messages or of mo's answers: {sam:?}");
}

/// Search finds only what the person has: nothing deleted for everyone,
/// nothing from before they deleted the conversation for themselves.
#[test]
fn search_finds_only_what_the_person_has() {
    let mut x = World::new();
    x.say("c", "m1", "sam", "launch plan draft", None, 1);
    x.say("c", "m2", "sam", "launch moved", None, 2);
    x.say("c", "m3", "sam", "launch party", None, 5);
    x.unsend("c", "m2", "sam", 2);
    x.chat(Chat::Cleared { channel: "c".into(), person: "mo".into(), at: x.at(3) });
    let found = |who: &str| -> Vec<String> {
        let hits = find(&x.view(who), "launch");
        hits["messages"].as_array().unwrap().iter().map(|m| m["id"].as_str().unwrap().to_string()).collect()
    };
    assert_eq!(found("mo"), ["m3"]);
    let mut sam = found("sam");
    sam.sort();
    assert_eq!(sam, ["m1", "m3"]);
}

/// A work card sits on the message that asked; one the agent began itself
/// stands alone; a card follows its message out when it is cleared.
#[test]
fn work_sits_on_the_message_that_asked() {
    let mut x = World::new();
    x.say("c", "m1", "mo", "@Magnus pull together the feedback", None, 1);
    x.answered("c", "t1", "mo", Some("m1"), "Here it is.", false, 2);
    x.answered("c", "t2", "mo", None, "Following up.", false, 3);
    for t in ["t1", "t2"] {
        if let Some(o) = x.runs.get_mut(t) {
            (o.status, o.summary, o.ended) = (Status::Running, None, None);
        }
    }
    let now = x.at(10);
    let shown = timeline(&x.view("sam"), "c", now);
    let m1 = shown.iter().find(|i| i.id == "m1").unwrap();
    assert_eq!(m1.work.as_ref().map(|w| (w.task_id.as_str(), w.title.as_str(), w.status)), Some(("t1", "@Magnus pull together the feedback", Status::Running)));
    let alone = shown.iter().find(|i| i.id == "work:t2").expect("work the agent began itself is not shown");
    assert_eq!((alone.sender.as_deref(), alone.requester.as_deref()), (Some("agent"), Some("mo")));
}

/// As the personal chat's turn gives way to its answer, a short request's
/// card is gone once it is done: the answer stands in its place. Long work
/// keeps its row, as a task a personal message started keeps its card; so
/// does work stopped with no answer, as a stopped personal turn keeps its line.
#[test]
fn a_short_requests_card_gives_way_to_its_answer() {
    let mut x = World::new();
    x.say("c", "m1", "mo", "@Magnus plan the launch", None, 1);
    x.answered("c", "t1", "mo", Some("m1"), "Here is the plan.", false, 2);
    x.say("c", "m2", "mo", "@Magnus research the market", None, 3);
    x.answered("c", "t2", "mo", Some("m2"), "Here is the research.", false, 4000);
    if let Some(o) = x.runs.get_mut("t2") {
        (o.started, o.steps) = (x.t0, 200);
    }
    x.say("c", "m3", "mo", "@Magnus book the hall", None, 5);
    x.answered("c", "t3", "mo", Some("m3"), "", false, 6);
    if let Some(o) = x.runs.get_mut("t3") {
        (o.status, o.summary) = (Status::Cancelled, None);
    }
    let now = x.at(5000);
    let shown = timeline(&x.view("mo"), "c", now);
    let card = |id: &str| shown.iter().find(|i| i.id == id).and_then(|i| i.work.as_ref()).map(|w| w.status);
    assert_eq!(card("m1"), None, "a done short request still shows its card under the answer");
    assert!(shown.iter().any(|i| i.id == "agent:t1" && i.text == "Here is the plan."), "the answer is not there");
    assert_eq!(card("m2"), Some(Status::Completed), "long work lost its row");
    assert_eq!(card("m3"), Some(Status::Cancelled), "work that was stopped lost its line");
}
