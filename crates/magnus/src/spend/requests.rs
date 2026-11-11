//! A person at their limit asks for more from inside the app; admins are
//! told in their inbox, and one approves (the person's limit rises by the
//! amount for that month alone) or declines. Both are recorded, and the
//! person is told in theirs.

use super::{enforce, month, valid_usd, Change};
use crate::events::Ev;
use crate::kernel::{new_id, Kernel};
use axum::http::StatusCode;
use chrono::Utc;
use serde_json::{json, Value};

pub type Refusal = (StatusCode, String);

/// The longest reason a person can give, in characters.
pub const MAX_REASON_CHARS: usize = 2_000;

fn failed(e: anyhow::Error) -> Refusal {
    tracing::error!("spend: {e:#}");
    (StatusCode::INTERNAL_SERVER_ERROR, "internal error".into())
}

impl Kernel {
    /// What the person sees of their month: spent, their limit as the
    /// cascade leaves it, and their request, if any.
    pub fn my_spend(&self, person: &str) -> Value {
        self.read(|s| {
            let now = Utc::now();
            let m = month::key(now);
            let (_, theirs) = month::spent_by(s, &m, person);
            let stopped = enforce::left_now(s, person, now).err();
            let request = s.spend.requests.values().filter(|r| r.person == person && r.month == m).max_by_key(|r| r.at).cloned();
            json!({
                "month": m, "spent_usd": theirs, "limit_usd": s.spend.person_limit(person, &m),
                "left_usd": enforce::left_now(s, person, now).ok().flatten(),
                "stopped": stopped.as_ref().map(enforce::details), "request": request,
            })
        })
    }

    /// The person asks for `usd` more this month.
    pub async fn ask_for_more(&self, person: &str, usd: f64, reason: &str) -> Result<Value, Refusal> {
        if !valid_usd(usd) || usd <= 0.0 {
            return Err((StatusCode::BAD_REQUEST, "ask for an amount of dollars above 0".into()));
        }
        if reason.chars().count() > MAX_REASON_CHARS {
            return Err((StatusCode::BAD_REQUEST, format!("a reason is at most {MAX_REASON_CHARS} characters")));
        }
        let m = month::key(Utc::now());
        let (has_limit, pending) = self.read(|s| (s.spend.person_limit(person, &m).is_some(), s.spend.pending(person, &m).is_some()));
        if !has_limit {
            return Err((StatusCode::CONFLICT, "you have no spend limit of your own to raise".into()));
        }
        if pending {
            return Err((StatusCode::CONFLICT, "your request for more is already waiting for an admin".into()));
        }
        let id = new_id("spendreq");
        let name = self.person(person).map(|p| if p.name.is_empty() { p.id } else { p.name }).unwrap_or_else(|| person.to_string());
        let summary = format!("{name} asks for ${usd:.2} more spend this month.");
        let details = json!({ "reason": "spend_request", "evidence": { "request": id, "person": person, "usd": usd } });
        let actor = format!("person:{person}");
        let mut evs = vec![(actor.as_str(), Ev::Spend(Change::IncreaseAsked { id: id.clone(), person: person.into(), month: m, usd, reason: reason.into() }))];
        let admins = self.admins().await;
        evs.extend(admins.iter().map(|a| ("kernel", self.notice_to(None, None, a.clone(), summary.clone(), details.clone()))));
        self.emit_all(evs).await.map_err(failed)?;
        Ok(json!({ "id": id, "usd": usd }))
    }

    /// An admin approves (with the amount asked, or `usd`) or declines.
    pub async fn decide_more(&self, actor: &str, id: &str, approve: bool, usd: Option<f64>, note: &str) -> Result<Value, Refusal> {
        if note.chars().count() > MAX_REASON_CHARS {
            return Err((StatusCode::BAD_REQUEST, format!("a note is at most {MAX_REASON_CHARS} characters")));
        }
        let _one = self.decide.lock().await;
        let Some(r) = self.read(|s| s.spend.requests.get(id).cloned()) else { return Err((StatusCode::NOT_FOUND, "no such request".into())) };
        if r.decided.is_some() {
            return Err((StatusCode::CONFLICT, "this request is already decided".into()));
        }
        let usd = if approve { usd.unwrap_or(r.usd) } else { 0.0 };
        if approve && (!valid_usd(usd) || usd <= 0.0) {
            return Err((StatusCode::BAD_REQUEST, "approve an amount of dollars above 0".into()));
        }
        let summary =
            if approve { format!("An admin approved ${usd:.2} more spend for you this month.") } else { "An admin declined your request for more spend this month.".to_string() };
        let details = json!({ "reason": "spend_decided", "evidence": { "request": id, "approved": approve, "usd": usd, "note": note } });
        let by = actor.split_once(':').map(|(_, id)| id).unwrap_or(actor).to_string();
        let evs = vec![
            (actor, Ev::Spend(Change::IncreaseDecided { id: id.into(), approved: approve, usd, by, note: note.into() })),
            (actor, self.notice_to(None, None, r.person.clone(), summary, details)),
        ];
        self.emit_all(evs).await.map_err(failed)?;
        Ok(json!({ "id": id, "approved": approve, "usd": usd }))
    }
}
