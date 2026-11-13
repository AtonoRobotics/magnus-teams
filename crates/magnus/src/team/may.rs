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
use super::routes::Reply;
use super::Role;
use crate::api::err;
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
/// a 404 that does not say whether it exists, or a 403 saying `why`.
pub(super) fn check(k: &Kernel, ws: &str, channel: &str, id: &str, person: &str, allowed: impl Fn(&May) -> bool, why: &str) -> Result<TeamItem, Box<Reply>> {
    let now = chrono::Utc::now();
    let Some(e) = k.read(|s| MemberView::of(s, ws, person).and_then(|v| entry(&v, channel, id, now))) else {
        return Err(Box::new(err(StatusCode::NOT_FOUND, "no such message")));
    };
    if !e.may.as_ref().is_some_and(allowed) {
        return Err(Box::new(err(StatusCode::FORBIDDEN, why)));
    }
    Ok(e)
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
