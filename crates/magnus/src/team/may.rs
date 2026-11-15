//! What a member may do with a channel's entry, decided once from what they
//! see (`MemberView`) and their role: the app draws its message controls from
//! it (`may` on each entry), and each team message route refuses what it
//! does not allow. The personal chat's entries carry the same `May`
//! (decisions/2026-11-13-one-message-actions.md).
//!
//! The team's own rules, kept here because a team needs them:
//! - an answer holding data only its requester may see is neither shared nor
//!   saved for the team;
//! - a guest saves nothing, makes no task, pins nothing and has the agent
//!   do nothing;
//! - only who asked asks again, or gives feedback on the answer;
//! - who wrote a message edits it; who wrote it, or whom the agent wrote for,
//!   deletes it, and so do the workspace's owners and admins outside a direct
//!   conversation ("The teams admin can moderate and delete it only from
//!   teams", the owner, 2026-11-11).
use super::conversation::AGENT_ID;
use super::items::{self, TeamItem};
use super::member_view::{MemberView, Runs, Said};
use super::Role;
use crate::api_error::ApiResult;
use crate::chat_entry::May;
use crate::kernel::Kernel;
use axum::http::StatusCode;

type At = chrono::DateTime<chrono::Utc>;

/// What this member may do with this entry; nothing for a deleted message's
/// place or for the agent's work.
pub fn may<R: Runs>(v: &MemberView<R>, channel: &str, e: &TeamItem) -> Option<May> {
    if !matches!(e.kind, "message" | "answer") {
        return None;
    }
    let me = v.person();
    let w = v.workspace();
    let role = w.members.get(me).copied().unwrap_or(Role::Guest);
    let guest = role == Role::Guest;
    let moderates = role >= Role::Admin && !w.chat.direct.contains_key(channel);
    let agent = e.sender.as_deref() == Some(AGENT_ID);
    let mine = e.sender.as_deref() == Some(me);
    let for_me = agent && e.requester.as_deref() == Some(me);
    let asked = for_me && e.task.is_some();
    let private = e.private == Some(true);
    Some(May {
        edit: mine && !agent,
        retry: asked && !guest,
        feedback: asked,
        share: !private,
        save: !guest && !private,
        task: !guest,
        pin: !guest,
        delete: (mine && !agent) || for_me || moderates,
    })
}

/// The entry `id` if this member sees it and `allowed` lets them act on it:
/// a 404 that does not say whether it exists, or a 403 saying `why`. The
/// channel as a conversation decides both (`crate::conversation`).
pub(super) fn check(k: &Kernel, ws: &str, channel: &str, id: &str, person: &str, allowed: impl Fn(&May) -> bool, why: &str) -> ApiResult<TeamItem> {
    use crate::conversation::{channel as conv, Conversation};
    conv::read(k, ws, person, channel, &conv::Names::new(), |c| c.act(id, allowed, why))
}

/// One entry as this member sees it, with what they may do with it.
pub fn entry<R: Runs>(v: &MemberView<R>, channel: &str, id: &str, now: At) -> Option<TeamItem> {
    let mut e = match v.message(channel, id, now)? {
        Said::Message(m) => items::message(v.workspace(), m),
        Said::Answer(a) => items::answer(&a),
    };
    e.may = may(v, channel, &e);
    Some(e)
}

/// What a member asks to do in a workspace, beside acting on an entry: the
/// team's rules for each, decided here once.
pub enum Act<'a> {
    /// Post a message, `with_agent` when it asks the agent or whispers to it,
    /// or start a conversation with the agent: a guest reads and chats, and
    /// has the agent do nothing.
    Post { with_agent: bool },
    /// Take `person` out of the workspace: anyone leaves, an admin removes.
    Remove { person: &'a str },
    /// Delete a file `by` added: they may, and so may an admin.
    DeleteFile { by: &'a str },
    /// Tie a channel to a project its `owner` keeps, or untie it (None):
    /// the project's owner or an admin ties, an admin unties.
    LinkChannel { owner: Option<&'a str> },
}

/// Whether a member with `role`, `me`, may do `a`: why not when not.
pub fn act(role: Role, me: &str, a: Act) -> Result<(), &'static str> {
    let admin = role >= Role::Admin;
    let (ok, why) = match a {
        Act::Post { with_agent } => (!with_agent || role > Role::Guest, "a guest can read and chat, and cannot have the agent act"),
        Act::Remove { person } => (person == me || admin, "that takes the Admin role here"),
        Act::DeleteFile { by } => (by == me || admin, "only who added it, or an admin, may delete it"),
        Act::LinkChannel { owner: Some(owner) } => (owner == me || admin, "the project's owner or an admin ties a channel to it"),
        Act::LinkChannel { owner: None } => (admin, "an admin unties a channel"),
    };
    if ok {
        Ok(())
    } else {
        Err(why)
    }
}

/// `act`, as a route refuses: a 403 saying why not.
pub fn allow(role: Role, me: &str, a: Act) -> crate::api_error::ApiResult<()> {
    act(role, me, a).map_err(|why| crate::api_error::ApiError::new(StatusCode::FORBIDDEN, why))
}

#[cfg(test)]
mod act_tests {
    use super::*;

    /// Each workspace rule by role, for oneself and for another member.
    #[test]
    fn each_role_may_what_the_team_rules_say() {
        let roles = [Role::Guest, Role::Member, Role::Admin, Role::Owner];
        for role in roles {
            let admin = role >= Role::Admin;
            assert!(act(role, "me", Act::Post { with_agent: false }).is_ok(), "{role:?} could not chat");
            assert_eq!(act(role, "me", Act::Post { with_agent: true }).is_ok(), role != Role::Guest, "{role:?} with the agent");
            assert!(act(role, "me", Act::Remove { person: "me" }).is_ok(), "{role:?} could not leave");
            assert_eq!(act(role, "me", Act::Remove { person: "you" }).is_ok(), admin, "{role:?} removed another");
            assert!(act(role, "me", Act::DeleteFile { by: "me" }).is_ok());
            assert_eq!(act(role, "me", Act::DeleteFile { by: "you" }).is_ok(), admin, "{role:?} deleted another's file");
            assert!(act(role, "me", Act::LinkChannel { owner: Some("me") }).is_ok());
            assert_eq!(act(role, "me", Act::LinkChannel { owner: Some("you") }).is_ok(), admin);
            assert_eq!(act(role, "me", Act::LinkChannel { owner: None }).is_ok(), admin, "{role:?} untied a channel");
        }
        assert_eq!(act(Role::Guest, "me", Act::Post { with_agent: true }), Err("a guest can read and chat, and cannot have the agent act"));
        assert_eq!(act(Role::Member, "me", Act::Remove { person: "you" }), Err("that takes the Admin role here"));
    }
}
