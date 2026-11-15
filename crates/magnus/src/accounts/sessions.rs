//! Sessions: opening one, finding the live one a token belongs to, and
//! ending them, one, a device's or a person's.

use super::{key, rules, session_of, token_hash, Accounts, Session, Status};
use anyhow::{Context, Result};
use chrono::Utc;
use rand::RngCore;

impl Accounts {
    /// End every session of `person`; the ids of those ended.
    pub async fn end_all(&self, person: &str) -> Result<Vec<String>> {
        let person = key(person);
        let mut g = self.client().await?;
        let c = g.as_mut().context("accounts connection")?;
        let mut ended = vec![];
        for r in c.query("DELETE FROM public.account_sessions WHERE person = $1 RETURNING id", &[&person]).await? {
            ended.push(r.get("id"));
        }
        self.refresh_local(c).await?;
        Ok(ended)
    }

    /// The people with a live session.
    pub async fn signed_in(&self) -> std::collections::BTreeSet<String> {
        let mut g = match self.client().await {
            Ok(g) => g,
            Err(_) => return std::collections::BTreeSet::new(),
        };
        let c = match g.as_mut() {
            Some(c) => c,
            None => return std::collections::BTreeSet::new(),
        };
        let _ = self.fresh_local(c).await;
        c.query("SELECT DISTINCT person FROM public.account_sessions WHERE expires > now()", &[]).await.map(|rows| rows.into_iter().map(|r| r.get(0)).collect()).unwrap_or_default()
    }

    /// Open a session for a person whose identity is already established.
    /// `device` is the paired computer, when this sign-in presented one.
    /// It lasts the rules' longest session: the lifetimes an admin sets
    /// govern every member session the appliance issues, however the
    /// person signed in (#508).
    pub async fn open_session(&self, person: &str, client: &str, device: Option<&str>) -> Result<(Session, String)> {
        let person = key(person);
        let now = Utc::now();
        let mut raw = [0u8; 32];
        rand::rngs::OsRng.fill_bytes(&mut raw);
        let token = format!("mgs_{}", hex::encode(raw));
        let mut g = self.client().await?;
        let c = g.as_mut().context("accounts connection")?;
        let policy = self.fresh_policy(c).await;
        let session = Session {
            id: crate::kernel::new_id("sess"),
            person,
            created: now,
            expires: rules::session_expires(now, &policy),
            client: client.to_string(),
            device: device.map(str::to_string),
        };
        c.execute(
            "INSERT INTO public.account_sessions (hash, id, person, created, expires, client, device, seen) VALUES ($1, $2, $3, $4, $5, $6, $7, $4)",
            &[&token_hash(&token), &session.id, &session.person, &session.created, &session.expires, &session.client, &session.device],
        )
        .await?;
        self.refresh_local(c).await?;
        Ok((session, token))
    }

    /// The live session a token belongs to: unexpired, lately used, its
    /// row still there, and its person still active. Read from the store
    /// on every call, so a removal or disabling from any process revokes
    /// here at once, and a re-added id never inherits ended sessions
    /// (#469). A live session's last-seen stamp is refreshed, at most
    /// once a minute, so idle sessions end without a write on every
    /// request. None only when there is no such session; a store that
    /// cannot answer is an error, never a signed-out person (decision
    /// `2026-11-14-a-database-fault-is-not-a-sign-out.md`).
    pub async fn session(&self, token: &str) -> Result<Option<Session>> {
        let mut g = self.client().await.context("reach the account store")?;
        let c = g.as_mut().context("accounts connection")?;
        let _ = self.fresh_local(c).await;
        let policy = self.ttl_policy(c).await;
        let r = c
            .query_opt(
                "SELECT s.id, s.person, s.created, s.expires, s.client, s.device, COALESCE(s.seen, s.created) AS seen, t.status, now() AS now FROM public.account_sessions s
                 LEFT JOIN public.account_statuses t ON t.person = s.person
                 WHERE s.hash = $1",
                &[&token_hash(token)],
            )
            .await
            .context("read the session")?;
        let Some(r) = r else { return Ok(None) };
        // Judged on the store's own clock, read with the row.
        let life = rules::SessionRow { expires: r.get("expires"), seen: r.get("seen"), status: r.get::<_, Option<String>>("status").map(|s| Status::parse(&s)), now: r.get("now") };
        if !rules::session_live(&life, &policy) {
            return Ok(None);
        }
        let session = session_of(&r);
        // Best-effort: a stamp that cannot be written does not end a live
        // session, it only leaves the older one.
        if rules::needs_restamp(&life) {
            let _ = c.execute("UPDATE public.account_sessions SET seen = now() WHERE hash = $1", &[&token_hash(token)]).await;
        }
        Ok(Some(session))
    }

    /// A person's live sessions, newest first, each with whether it is the
    /// one `token` belongs to.
    pub async fn sessions_of(&self, person: &str, token: &str) -> Vec<(Session, bool)> {
        let person = key(person);
        let mut g = match self.client().await {
            Ok(g) => g,
            Err(_) => return vec![],
        };
        let c = match g.as_mut() {
            Some(c) => c,
            None => return vec![],
        };
        let _ = self.fresh_local(c).await;
        let mine = token_hash(token);
        c.query("SELECT hash, id, person, created, expires, client, device FROM public.account_sessions WHERE person = $1 AND expires > now() ORDER BY created DESC", &[&person])
            .await
            .map(|rows| rows.into_iter().map(|r| (session_of(&r), r.get::<_, String>("hash") == mine)).collect())
            .unwrap_or_default()
    }

    /// End one of a person's sessions by its id: that device is signed out.
    pub async fn end(&self, person: &str, id: &str) -> Result<Option<Session>> {
        let person = key(person);
        let mut g = self.client().await?;
        let c = g.as_mut().context("accounts connection")?;
        let gone = c
            .query_opt("DELETE FROM public.account_sessions WHERE person = $1 AND id = $2 RETURNING id, person, created, expires, client, device", &[&person, &id])
            .await?
            .map(|r| session_of(&r));
        self.refresh_local(c).await?;
        Ok(gone)
    }

    /// Remember which paired computer a session belongs to. Pairing does
    /// this for the sign-in that asked for the certificate, which is on the
    /// LAN and so was not bound on its own.
    pub async fn bind_session_device(&self, token: &str, device: &str) -> Result<()> {
        let mut g = self.client().await?;
        let c = g.as_mut().context("accounts connection")?;
        c.execute("UPDATE public.account_sessions SET device = $1 WHERE hash = $2", &[&device, &token_hash(token)]).await?;
        self.refresh_local(c).await?;
        Ok(())
    }

    /// End every session bound to a device, and return their ids. Revoking
    /// the device's certificate does this, so a stolen computer's saved
    /// sign-in stops working with the certificate.
    pub async fn end_device(&self, device: &str) -> Result<Vec<Session>> {
        let mut g = self.client().await?;
        let c = g.as_mut().context("accounts connection")?;
        let ended: Vec<Session> = c
            .query("DELETE FROM public.account_sessions WHERE device = $1 RETURNING id, person, created, expires, client, device", &[&device])
            .await?
            .into_iter()
            .map(|r| session_of(&r))
            .collect();
        self.refresh_local(c).await?;
        Ok(ended)
    }

    pub async fn sign_out(&self, token: &str) -> Result<Option<Session>> {
        let mut g = self.client().await?;
        let c = g.as_mut().context("accounts connection")?;
        let gone = c
            .query_opt("DELETE FROM public.account_sessions WHERE hash = $1 RETURNING id, person, created, expires, client, device", &[&token_hash(token)])
            .await?
            .map(|r| session_of(&r));
        self.refresh_local(c).await?;
        Ok(gone)
    }
}
