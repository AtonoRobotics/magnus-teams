//! Single-use account tokens in the store: invitations, password-recovery
//! links and one-time machine codes.

use super::{key, phc, rules, token_hash, valid_person_id, Accounts, ResetRequest, Role, INVITE_DAYS, RESET_HOURS};
use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use rand::RngCore;

impl Accounts {
    /// A one-time code for the owner's backup password, valid ten minutes.
    /// Only someone with a shell on the appliance's machine (as its user)
    /// can make one, so a relay or tunnel that delivers remote connections
    /// from loopback cannot carry the backup.
    pub async fn new_machine_code(&self) -> Result<String> {
        use rand::Rng;
        let mut g = self.client().await?;
        let c = g.as_mut().context("accounts connection")?;
        let now = Utc::now();
        c.execute("DELETE FROM public.account_machine_codes WHERE until <= $1", &[&now]).await?;
        let code: String = (0..10).map(|_| char::from(b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789"[rand::rngs::OsRng.gen_range(0..32)])).collect();
        c.execute("INSERT INTO public.account_machine_codes (hash, until) VALUES ($1, $2)", &[&token_hash(&code), &(now + Duration::minutes(10))]).await?;
        self.refresh_local(c).await?;
        Ok(code)
    }

    /// Invite a person: a single-use token, good for 7 days, is stored by
    /// its hash and returned to pass on. The person is local or in the
    /// config; someone with neither a password nor a record is marked
    /// invited, anyone else keeps their status, so an invite never
    /// revokes working access. Callers pass a real person.
    pub async fn invite(&self, person: &str, email: Option<&str>) -> Result<String> {
        let person = key(person);
        let mut g = self.client().await?;
        let c = g.as_mut().context("accounts connection")?;
        let tx = c.transaction().await?;
        tx.batch_execute("SELECT pg_advisory_xact_lock(hashtext('magnus-accounts'))").await?;
        let now = Utc::now();
        tx.execute("DELETE FROM public.account_invites WHERE expires <= $1", &[&now]).await?;
        let mut raw = [0u8; 16];
        rand::rngs::OsRng.fill_bytes(&mut raw);
        let token = hex::encode(raw);
        tx.execute("DELETE FROM public.account_invites WHERE person = $1", &[&person]).await?;
        tx.execute(
            "INSERT INTO public.account_invites (hash, person, email, expires) VALUES ($1, $2, $3, $4)",
            &[&token_hash(&token), &person, &email, &(now + Duration::days(INVITE_DAYS))],
        )
        .await?;
        let known = tx.query_opt("SELECT 1 FROM public.account_statuses WHERE person = $1", &[&person]).await?.is_some()
            || tx.query_opt("SELECT 1 FROM public.account_passwords WHERE person = $1", &[&person]).await?.is_some();
        if !known {
            tx.execute("INSERT INTO public.account_statuses (person, status) VALUES ($1, 'invited') ON CONFLICT (person) DO NOTHING", &[&person]).await?;
        }
        tx.commit().await?;
        self.refresh_local(c).await?;
        Ok(token)
    }

    /// Invite someone new in one transaction: the person, their invited
    /// status, their role and a single-use token. One call, so an
    /// unfinished invitation — a role committed before its invited
    /// status — never counts as an enabled admin (#527). Refuses a held
    /// id; the caller refuses config ids, which this store cannot see.
    pub async fn invite_person(&self, id: &str, name: &str, email: Option<&str>, role: Role) -> Result<String> {
        let id = key(id);
        anyhow::ensure!(valid_person_id(&id), "a person id is 1 to 64 letters, digits, dots, dashes and underscores");
        let mut g = self.client().await?;
        let c = g.as_mut().context("accounts connection")?;
        let tx = c.transaction().await?;
        tx.batch_execute("SELECT pg_advisory_xact_lock(hashtext('magnus-accounts'))").await?;
        let n = tx.execute("INSERT INTO public.account_people (person, name, email) VALUES ($1, $2, $3) ON CONFLICT (person) DO NOTHING", &[&id, &name, &email]).await?;
        anyhow::ensure!(n == 1, "{id} is already a person here");
        tx.execute("INSERT INTO public.account_statuses (person, status) VALUES ($1, 'invited') ON CONFLICT (person) DO NOTHING", &[&id]).await?;
        tx.execute("INSERT INTO public.account_roles (person, role) VALUES ($1, $2) ON CONFLICT (person) DO UPDATE SET role = EXCLUDED.role", &[&id, &role.as_str()]).await?;
        let now = Utc::now();
        tx.execute("DELETE FROM public.account_invites WHERE expires <= $1", &[&now]).await?;
        let mut raw = [0u8; 16];
        rand::rngs::OsRng.fill_bytes(&mut raw);
        let token = hex::encode(raw);
        tx.execute(
            "INSERT INTO public.account_invites (hash, person, email, expires) VALUES ($1, $2, $3, $4)",
            &[&token_hash(&token), &id, &email, &(now + Duration::days(INVITE_DAYS))],
        )
        .await?;
        tx.commit().await?;
        self.refresh_local(c).await?;
        Ok(token)
    }

    /// Accept an invitation: the token with the new password activates the
    /// person. The person it activated, or None for a bad, used or expired
    /// token (all answered alike).
    pub async fn accept(&self, token: &str, password: &str) -> Result<Option<String>> {
        self.accept_at(token, password, Utc::now()).await
    }

    /// `accept` at `at`, for tests.
    pub async fn accept_at(&self, token: &str, password: &str, at: DateTime<Utc>) -> Result<Option<String>> {
        // The password is judged before the token is touched, so a short one
        // does not burn the invitation.
        phc::long_enough(password, self.cfg.min_password_length)?;
        let mut g = self.client().await?;
        let c = g.as_mut().context("accounts connection")?;
        let tx = c.transaction().await?;
        tx.batch_execute("SELECT pg_advisory_xact_lock(hashtext('magnus-accounts'))").await?;
        tx.execute("DELETE FROM public.account_invites WHERE expires <= $1", &[&at]).await?;
        // A disabling stands against an outstanding invitation: the token
        // stays live for a re-enabling, but accepting refuses alike and
        // consumes nothing (#449).
        let held: Option<String> = tx.query_opt("SELECT person FROM public.account_invites WHERE hash = $1", &[&token_hash(token.trim())]).await?.map(|r| r.get(0));
        let held = match &held {
            Some(person) => Some(Self::status_tx(&tx, person).await?),
            None => None,
        };
        if !rules::invite_consumes(held) {
            tx.commit().await?;
            return Ok(None);
        }
        // Consumed by the delete itself, so racing accepts admit exactly
        // one (#450).
        let consumed: Option<String> = tx.query_opt("DELETE FROM public.account_invites WHERE hash = $1 RETURNING person", &[&token_hash(token.trim())]).await?.map(|r| r.get(0));
        let Some(person) = consumed else {
            tx.commit().await?;
            return Ok(None);
        };
        let phc = phc::hash(password)?;
        // Permanent: accepting after a temporary password was set must
        // not leave the account choosing forever (#527).
        tx.execute(
            "INSERT INTO public.account_passwords (person, phc, temporary) VALUES ($1, $2, false) ON CONFLICT (person) DO UPDATE SET phc = EXCLUDED.phc, temporary = false",
            &[&person, &phc],
        )
        .await?;
        tx.execute("INSERT INTO public.account_statuses (person, status) VALUES ($1, 'active') ON CONFLICT (person) DO UPDATE SET status = 'active'", &[&person]).await?;
        tx.commit().await?;
        self.refresh_local(c).await?;
        Ok(Some(person))
    }

    /// Start a password recovery for the holder of `email`: a one-hour
    /// token, or nothing to send (unknown address, not an active person,
    /// or asked within the minute — the answer never says which, so
    /// addresses cannot be probed).
    pub async fn request_reset(&self, email: &str) -> Result<Option<ResetRequest>> {
        self.request_reset_at(email, Utc::now()).await
    }

    /// `request_reset` at `at`, for tests.
    pub async fn request_reset_at(&self, email: &str, at: DateTime<Utc>) -> Result<Option<ResetRequest>> {
        let email = email.trim().to_lowercase();
        if email.is_empty() {
            return Ok(None);
        }
        let mut g = self.client().await?;
        let c = g.as_mut().context("accounts connection")?;
        let tx = c.transaction().await?;
        tx.batch_execute("SELECT pg_advisory_xact_lock(hashtext('magnus-accounts'))").await?;
        tx.execute("DELETE FROM public.account_reset_tokens WHERE expires <= $1", &[&at]).await?;
        // The first holder of the address, deterministically: people keep
        // their own addresses, and two sharing one is a misconfiguration.
        let person: Option<String> = tx.query_opt("SELECT person FROM public.account_people WHERE lower(email) = $1 ORDER BY person LIMIT 1", &[&email]).await?.map(|r| r.get(0));
        let Some(person) = person else {
            tx.commit().await?;
            return Ok(None);
        };
        let status = Self::status_tx(&tx, &person).await?;
        let requested: Option<DateTime<Utc>> = tx.query_opt("SELECT requested FROM public.account_reset_tokens WHERE person = $1", &[&person]).await?.map(|r| r.get(0));
        match rules::reset_request(Some(&rules::ResetHolder { status, requested }), at) {
            rules::ResetAnswer::Nothing => {
                tx.commit().await?;
                return Ok(None);
            }
            rules::ResetAnswer::Silent => {
                tx.commit().await?;
                return Ok(Some(ResetRequest { person, token: String::new(), send: false }));
            }
            rules::ResetAnswer::Issue => {}
        }
        let mut raw = [0u8; 16];
        rand::rngs::OsRng.fill_bytes(&mut raw);
        let token = hex::encode(raw);
        tx.execute(
            "INSERT INTO public.account_reset_tokens (person, hash, expires, requested) VALUES ($1, $2, $3, $4) ON CONFLICT (person) DO UPDATE SET hash = EXCLUDED.hash, expires = EXCLUDED.expires, requested = EXCLUDED.requested",
            &[&person, &token_hash(&token), &(at + Duration::hours(RESET_HOURS)), &at],
        )
        .await?;
        tx.commit().await?;
        Ok(Some(ResetRequest { person, token, send: true }))
    }

    /// Finish a password recovery: the token with the new password sets it
    /// permanently, clears any authenticator (the email is the recovery
    /// factor), and ends every session. Single-use: the token dies here,
    /// and racing confirms admit exactly one. Answers the person, or
    /// nothing for a bad, spent or expired token.
    pub async fn confirm_reset(&self, token: &str, password: &str) -> Result<Option<(String, Vec<String>)>> {
        self.confirm_reset_at(token, password, Utc::now()).await
    }

    /// `confirm_reset` at `at`, for tests.
    pub async fn confirm_reset_at(&self, token: &str, password: &str, at: DateTime<Utc>) -> Result<Option<(String, Vec<String>)>> {
        // The password is judged before the token is touched, so a short
        // one does not burn the recovery.
        phc::long_enough(password, self.cfg.min_password_length)?;
        let mut g = self.client().await?;
        let c = g.as_mut().context("accounts connection")?;
        let tx = c.transaction().await?;
        tx.batch_execute("SELECT pg_advisory_xact_lock(hashtext('magnus-accounts'))").await?;
        tx.execute("DELETE FROM public.account_reset_tokens WHERE expires <= $1", &[&at]).await?;
        let held: Option<String> = tx.query_opt("SELECT person FROM public.account_reset_tokens WHERE hash = $1", &[&token_hash(token.trim())]).await?.map(|r| r.get(0));
        let holder = match &held {
            Some(person) => {
                let known: bool = tx.query_opt("SELECT 1 FROM public.account_people WHERE person = $1", &[person]).await?.is_some();
                Some(rules::TokenHolder { known, status: Self::status_tx(&tx, person).await? })
            }
            None => None,
        };
        match rules::reset_confirm(holder.as_ref()) {
            rules::ConfirmAnswer::Consume => {}
            rules::ConfirmAnswer::DropToken => {
                tx.execute("DELETE FROM public.account_reset_tokens WHERE hash = $1", &[&token_hash(token.trim())]).await?;
                tx.commit().await?;
                return Ok(None);
            }
            rules::ConfirmAnswer::Refuse => {
                tx.commit().await?;
                return Ok(None);
            }
        }
        let consumed: Option<String> =
            tx.query_opt("DELETE FROM public.account_reset_tokens WHERE hash = $1 RETURNING person", &[&token_hash(token.trim())]).await?.map(|r| r.get(0));
        let Some(person) = consumed else {
            tx.commit().await?;
            return Ok(None);
        };
        let phc = phc::hash(password)?;
        tx.execute(
            "INSERT INTO public.account_passwords (person, phc, temporary) VALUES ($1, $2, false) ON CONFLICT (person) DO UPDATE SET phc = EXCLUDED.phc, temporary = false",
            &[&person, &phc],
        )
        .await?;
        tx.execute("DELETE FROM public.account_totp WHERE person = $1", &[&person]).await?;
        tx.execute("DELETE FROM public.account_totp_steps WHERE person = $1", &[&person]).await?;
        let mut ended = vec![];
        for r in tx.query("DELETE FROM public.account_sessions WHERE person = $1 RETURNING id", &[&person]).await? {
            ended.push(r.get("id"));
        }
        tx.commit().await?;
        self.refresh_local(c).await?;
        Ok(Some((person, ended)))
    }

    /// Whether a recovery token is live: present and unexpired. Peeks
    /// without consuming, for the recovery page.
    pub async fn reset_live(&self, token: &str) -> bool {
        let Ok(mut g) = self.client().await else { return false };
        let Some(c) = g.as_mut() else { return false };
        let now = Utc::now();
        let _ = c.execute("DELETE FROM public.account_reset_tokens WHERE expires <= $1", &[&now]).await;
        c.query_opt("SELECT person FROM public.account_reset_tokens WHERE hash = $1", &[&token_hash(token.trim())]).await.ok().flatten().is_some()
    }

    /// Take a one-time code printed on the appliance's machine
    /// (`magnus sign-in-code`): true once, if it is current.
    pub async fn take_machine_code(&self, code: &str) -> Result<bool> {
        let mut g = self.client().await?;
        let c = g.as_mut().context("accounts connection")?;
        let now = Utc::now();
        c.execute("DELETE FROM public.account_machine_codes WHERE until <= $1", &[&now]).await?;
        let n = c.execute("DELETE FROM public.account_machine_codes WHERE hash = $1 AND until > $2", &[&token_hash(code.trim()), &now]).await?;
        self.refresh_local(c).await?;
        Ok(n == 1)
    }
}
