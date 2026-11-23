//! A person turned off, one way however it happens (decision
//! `2026-11-23-one-person-is-turned-off.md`): by an admin here, on the
//! server's command line, or at the identity provider. Turning off holds
//! their new work, ends their sessions, pauses their schedules and revokes
//! their paired devices' certificates; signing in refuses them while their
//! account is off (`api::sign_in`).

use crate::events::Ev;
use crate::kernel::Kernel;
use std::sync::Arc;
use std::time::Duration;

/// How often people's sign-in sessions are checked with the identity
/// provider, and turned-off accounts with their hold.
const SESSIONS_EVERY: Duration = Duration::from_secs(300);

impl Kernel {
    /// Hold the work of accounts turned off outside the kernel (the
    /// command line writes the store alone), and end the sessions and pause
    /// the schedules of people the identity provider has turned off or no
    /// longer knows: once now, then every five minutes.
    pub fn watch_sessions(self: &Arc<Self>) {
        let k = self.clone();
        tokio::spawn(async move {
            let mut t = tokio::time::interval(SESSIONS_EVERY);
            loop {
                t.tick().await;
                k.hold_disabled_accounts().await;
                k.end_turned_off().await;
            }
        });
    }

    /// Turn off, with everything turning off does, everyone whose account is
    /// off in the store but whose work is not held. How many.
    pub async fn hold_disabled_accounts(&self) -> usize {
        let disabled = match self.accounts.disabled().await {
            Ok(d) => d,
            Err(e) => {
                tracing::warn!("cannot read which accounts are turned off: {e:#}");
                return 0;
            }
        };
        let mut n = 0;
        for person in disabled.into_iter().filter(|p| !self.read(|s| s.identity.is_turned_off(p))) {
            self.turn_off(&person, &format!("{person}'s account was turned off on the server"), "An administrator has turned your account off").await;
            n += 1;
        }
        n
    }

    /// Everyone signed in or with a schedule is asked about; a provider that
    /// cannot be asked changes nothing (owner's ruling 12).
    pub async fn end_turned_off(&self) -> usize {
        let Some(id) = self.identity() else { return 0 };
        let mut people = self.accounts.signed_in().await;
        people.extend(self.read(|s| s.arrangements.schedules().values().filter(|x| x.next_due().is_some()).map(|x| x.person.clone()).collect::<Vec<_>>()));
        let mut ended = 0;
        for person in people {
            let sub = self.read(|s| s.identity.subject(&person).map(str::to_string));
            match id.active(&person, sub.as_deref()).await {
                Ok(true) => {}
                Ok(false) => {
                    let label = id.label();
                    ended += self.turn_off(&person, &format!("{label} has turned {person} off or no longer knows them"), &format!("{label} has turned your account off")).await;
                }
                Err(e) => tracing::debug!("cannot ask whether {person} is still on: {e:#}"),
            }
        }
        ended
    }

    /// A person is turned off (at the provider, or by an admin): their new
    /// work is held, their sessions here end, their paired devices are
    /// revoked, and their schedules stop acting for them until they are
    /// turned back on and resume them. `why` is for the log, `told` for the
    /// person. How many sessions ended.
    pub async fn turn_off(&self, person: &str, why: &str, told: &str) -> usize {
        let mut ended = 0;
        // Their apps answer no calls for them from now; the sign-ins stay in
        // the vault, unused, for when they are back (connectors spec, Isolation).
        if !self.read(|s| s.identity.is_turned_off(person)) {
            self.tell(vec![("kernel", Ev::PersonTurnedOff { person: person.to_string(), why: why.to_string() })]).await;
        }
        match self.accounts.end_all(person).await {
            Ok(sessions) => {
                if !sessions.is_empty() {
                    tracing::warn!("{why}: their {} sessions here are ended", sessions.len());
                }
                ended = sessions.len();
                let evs: Vec<(&str, Ev)> = sessions.into_iter().map(|session| ("kernel", Ev::SignedOut { person: person.to_string(), session })).collect();
                if !evs.is_empty() {
                    self.tell(evs).await;
                }
            }
            Err(e) => tracing::error!("ending {person}'s sessions: {e:#}"),
        }
        self.revoke_devices_of("kernel", person).await;
        // Every schedule of theirs not over, paused or not: one held on a
        // waiting run, or suspended by an admin, stays paused for them once
        // that lifts, until they resume it.
        let schedules: Vec<crate::state::Schedule> =
            self.read(|s| s.arrangements.schedules().values().filter(|x| x.person == person && x.ended.is_none() && !x.paused_for_person()).cloned().collect());
        for sc in schedules {
            let title = self.schedule_title_for_notice(&sc).await;
            let notice = self.notice_to(
                None,
                None,
                person.to_string(),
                format!("A schedule paused: {title}"),
                serde_json::json!({ "reason": "schedule_paused", "evidence": { "schedule": sc.id, "why": told } }),
            );
            self.tell(vec![("kernel", Ev::SchedulePaused { id: sc.id.clone(), reason: told.to_string(), task: None, admin: false, source: None, turn: None }), ("kernel", notice)])
                .await;
        }
        ended
    }

    /// Revoke every paired device of `person`, as `actor`: dead here at
    /// once, so the device gate refuses them, and deleted at Cloudflare when
    /// it can be reached. How many.
    pub async fn revoke_devices_of(&self, actor: &str, person: &str) -> usize {
        let revoked = self.devices.revoke_person(self.device_issuer().as_ref(), person).await;
        let n = revoked.len();
        for d in revoked {
            if let Ok(ended) = self.accounts.end_device(&d.id).await {
                let evs: Vec<(&str, Ev)> = ended.into_iter().map(|s| (actor, Ev::SignedOut { person: s.person, session: s.id })).collect();
                if !evs.is_empty() {
                    self.tell(evs).await;
                }
            }
            self.tell(vec![(actor, Ev::DeviceRevoked { person: person.to_string(), device: d.id, by: actor.to_string() })]).await;
        }
        n
    }

    /// Cloudflare's certificate issuer, when this server pairs devices and
    /// its token can be read.
    fn device_issuer(&self) -> Option<crate::devices::Cloudflare> {
        let cfg = self.cfg.client_certs.clone()?;
        if self.vault.is_sealed() {
            return None;
        }
        let token = self.vault.get(&cfg.credential).ok().flatten()?.expose().to_string();
        Some(crate::devices::Cloudflare { api: crate::devices::CF_API.into(), zone: cfg.zone_id, token, days: cfg.days })
    }
}

#[cfg(test)]
#[path = "turned_off_tests.rs"]
mod tests;
