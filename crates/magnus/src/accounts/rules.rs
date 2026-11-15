//! What the accounts store decides, without the store: given the rows a
//! call read inside its own transaction, what it does next. The SQL code
//! reads, asks here, and writes; the atomicity (the advisory lock, the
//! `DELETE … RETURNING` that admits exactly one racing use) stays in the
//! SQL, where it is kept.

use super::{Policy, Status, RESET_RESEND_SECONDS};
use chrono::{DateTime, Duration, Utc};

/// Seconds a live session's last-seen stamp may lag before a use moves
/// it: idle sessions end without a write on every request.
pub(super) const SESSION_RESTAMP_SECONDS: i64 = 60;

/// What a password-reset request found for its address: the holder's
/// status, and when they last asked.
pub(super) struct ResetHolder {
    pub(super) status: Option<Status>,
    pub(super) requested: Option<DateTime<Utc>>,
}

/// What a password-reset request does.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum ResetAnswer {
    /// No one to send to: an unknown address, or someone not active
    /// (invited people follow their invitation, disabled people stay
    /// out). Any token is left alone.
    Nothing,
    /// Asked within the resend window: the same answer, nothing sent.
    Silent,
    /// Mint a token and send it.
    Issue,
}

/// A reset request for `holder` at `at`.
pub(super) fn reset_request(holder: Option<&ResetHolder>, at: DateTime<Utc>) -> ResetAnswer {
    let Some(h) = holder else { return ResetAnswer::Nothing };
    if h.status.is_some_and(|s| s != Status::Active) {
        return ResetAnswer::Nothing;
    }
    if h.requested.is_some_and(|t| (at - t).num_seconds() < RESET_RESEND_SECONDS) {
        return ResetAnswer::Silent;
    }
    ResetAnswer::Issue
}

/// What a reset token's holder looks like when it is confirmed: whether
/// they are a person here (the record decides, not the status: people
/// made on the command line carry no status row, #541), and their status.
pub(super) struct TokenHolder {
    pub(super) known: bool,
    pub(super) status: Option<Status>,
}

/// What confirming a reset token does.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum ConfirmAnswer {
    /// Try to consume it: the `DELETE … RETURNING` decides, so racing
    /// confirms admit exactly one (and an unheld token consumes nothing).
    Consume,
    /// A token for nobody here: it dies, and confirms nothing.
    DropToken,
    /// Its holder is not active: confirm nothing, and keep the token.
    Refuse,
}

/// Confirming a token whose row names `holder` (None: no such token).
pub(super) fn reset_confirm(holder: Option<&TokenHolder>) -> ConfirmAnswer {
    match holder {
        Some(h) if !h.known => ConfirmAnswer::DropToken,
        Some(h) if h.status.is_some_and(|s| s != Status::Active) => ConfirmAnswer::Refuse,
        _ => ConfirmAnswer::Consume,
    }
}

/// Whether accepting an invitation goes on to consume it. A disabling
/// stands against an outstanding invitation: accepting refuses alike and
/// consumes nothing, so the token stays live for a re-enabling (#449).
/// `held` is the status of the person the token is for, when the token is
/// held at all; an unheld token goes on, and its consume finds nothing.
pub(super) fn invite_consumes(held: Option<Option<Status>>) -> bool {
    held.flatten() != Some(Status::Disabled)
}

/// When a session opened at `now` under `p` ends: the rules' longest
/// session, at least an hour.
pub(super) fn session_expires(now: DateTime<Utc>, p: &Policy) -> DateTime<Utc> {
    now + Duration::hours(p.max_hours.max(1))
}

/// A stored session as read, with the store's own clock.
pub(super) struct SessionRow {
    pub(super) expires: DateTime<Utc>,
    /// The last use, or the opening when it was never stamped.
    pub(super) seen: DateTime<Utc>,
    pub(super) status: Option<Status>,
    /// The store's `now()`, read with the row.
    pub(super) now: DateTime<Utc>,
}

/// Whether a session is live: unexpired, used within the rules' idle
/// minutes (at least one), and its person active or unrecorded.
pub(super) fn session_live(r: &SessionRow, p: &Policy) -> bool {
    r.expires > r.now && r.seen > r.now - Duration::minutes(p.idle_minutes.max(1)) && r.status.is_none_or(|s| s == Status::Active)
}

/// Whether a live session's last-seen stamp is old enough to move.
pub(super) fn needs_restamp(r: &SessionRow) -> bool {
    r.seen < r.now - Duration::seconds(SESSION_RESTAMP_SECONDS)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t0() -> DateTime<Utc> {
        DateTime::from_timestamp(1_800_000_000, 0).unwrap()
    }

    fn rules(idle_minutes: i64, max_hours: i64) -> Policy {
        Policy {
            code_required: false,
            idle_minutes,
            max_hours,
            lockout_enabled: true,
            lockout_failures: 5,
            lockout_wait_minutes: 15,
            lockout_max_wait_minutes: 15,
            lockout_permanent: false,
        }
    }

    #[test]
    fn a_reset_request_sends_only_to_an_active_holder_once_a_minute() {
        let holder = |status, requested| ResetHolder { status, requested };
        let at = t0();
        let cases = [
            ("no holder", None, ResetAnswer::Nothing),
            ("unrecorded, never asked", Some(holder(None, None)), ResetAnswer::Issue),
            ("active, never asked", Some(holder(Some(Status::Active), None)), ResetAnswer::Issue),
            ("invited", Some(holder(Some(Status::Invited), None)), ResetAnswer::Nothing),
            ("disabled, asked just now", Some(holder(Some(Status::Disabled), Some(at))), ResetAnswer::Nothing),
            ("asked just now", Some(holder(None, Some(at))), ResetAnswer::Silent),
            ("asked 59 seconds ago", Some(holder(None, Some(at - Duration::seconds(59)))), ResetAnswer::Silent),
            ("asked a minute ago", Some(holder(None, Some(at - Duration::seconds(60)))), ResetAnswer::Issue),
        ];
        for (what, h, want) in cases {
            assert_eq!(reset_request(h.as_ref(), at), want, "{what}");
        }
    }

    #[test]
    fn confirming_a_reset_drops_a_token_for_nobody_and_keeps_one_for_the_quiet() {
        let h = |known, status| TokenHolder { known, status };
        let cases = [
            ("no such token", None, ConfirmAnswer::Consume),
            ("a person here, unrecorded", Some(h(true, None)), ConfirmAnswer::Consume),
            ("a person here, active", Some(h(true, Some(Status::Active))), ConfirmAnswer::Consume),
            ("a person here, disabled", Some(h(true, Some(Status::Disabled))), ConfirmAnswer::Refuse),
            ("a person here, invited", Some(h(true, Some(Status::Invited))), ConfirmAnswer::Refuse),
            ("nobody here", Some(h(false, None)), ConfirmAnswer::DropToken),
            ("nobody here, with a status left", Some(h(false, Some(Status::Disabled))), ConfirmAnswer::DropToken),
        ];
        for (what, holder, want) in cases {
            assert_eq!(reset_confirm(holder.as_ref()), want, "{what}");
        }
    }

    #[test]
    fn a_disabled_invitee_refuses_without_consuming() {
        assert!(!invite_consumes(Some(Some(Status::Disabled))), "a disabled invitee consumed the invitation");
        assert!(invite_consumes(Some(Some(Status::Invited))));
        assert!(invite_consumes(Some(Some(Status::Active))));
        assert!(invite_consumes(Some(None)), "an unrecorded invitee did not go on");
        assert!(invite_consumes(None), "an unheld token did not reach its consume");
    }

    #[test]
    fn a_session_lasts_the_longest_session_and_at_least_an_hour() {
        assert_eq!(session_expires(t0(), &rules(30, 8)), t0() + Duration::hours(8));
        assert_eq!(session_expires(t0(), &rules(30, 0)), t0() + Duration::hours(1), "no longest session made a dead one");
    }

    #[test]
    fn a_session_is_live_until_it_expires_idles_or_its_person_stops() {
        let now = t0();
        let p = rules(30, 8);
        let row = |expires, seen, status| SessionRow { expires, seen, status, now };
        let soon = now + Duration::hours(1);
        let cases = [
            ("fresh", row(soon, now, None), true),
            ("active", row(soon, now, Some(Status::Active)), true),
            ("expired at now", row(now, now, None), false),
            ("idle 29 minutes", row(soon, now - Duration::minutes(29), None), true),
            ("idle 30 minutes", row(soon, now - Duration::minutes(30), None), false),
            ("disabled", row(soon, now, Some(Status::Disabled)), false),
            ("invited", row(soon, now, Some(Status::Invited)), false),
        ];
        for (what, r, live) in cases {
            assert_eq!(session_live(&r, &p), live, "{what}");
        }
        // No idle minutes still means one.
        assert!(!session_live(&row(soon, now - Duration::minutes(1), None), &rules(0, 8)), "zero idle minutes did not read as one");
        assert!(session_live(&row(soon, now - Duration::seconds(59), None), &rules(0, 8)));
    }

    #[test]
    fn the_last_seen_stamp_moves_at_most_once_a_minute() {
        let now = t0();
        let r = |seen| SessionRow { expires: now + Duration::hours(1), seen, status: None, now };
        assert!(!needs_restamp(&r(now)), "a just-stamped session restamped");
        assert!(!needs_restamp(&r(now - Duration::seconds(SESSION_RESTAMP_SECONDS))), "exactly a minute restamped");
        assert!(needs_restamp(&r(now - Duration::seconds(SESSION_RESTAMP_SECONDS + 1))), "an old stamp stayed");
    }
}
