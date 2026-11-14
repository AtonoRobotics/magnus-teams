//! Changing a password with the current one, as a signed-in person does in
//! Settings.

use super::{check::counter_of, key, token_hash, Accounts, SignInError, Via};
use anyhow::{Context, Result};
use argon2::password_hash::{rand_core::OsRng, PasswordHasher, SaltString};
use argon2::Argon2;

impl Accounts {
    /// Change a signed-in person's password. The current one is checked as
    /// a sign-in checks it, and a wrong one counts on the counter for `via`,
    /// the same counter sign-in uses from there: guessing here locks as guessing at
    /// sign-in does. The new one replaces it, every other session of theirs
    /// ends (the one `keep` belongs to stays), and a recovery link mailed
    /// before dies. A new password too short is refused before the current
    /// one is checked, so it costs no attempt. Answers the ids of the
    /// sessions ended.
    pub async fn change_password(&self, person: &str, current: &str, new: &str, via: Via, keep: &str) -> Result<std::result::Result<Vec<String>, SignInError>> {
        anyhow::ensure!(new.chars().count() >= self.cfg.min_password_length, "a password needs at least {} characters", self.cfg.min_password_length);
        match self.sign_in_counted(person, current, &counter_of(via, &key(person))).await? {
            Ok(()) => {}
            // A temporary password only chooses the first one, and nobody
            // signed in holds one; answered as wrong, uncounted.
            Err(SignInError::MustChoose) => return Ok(Err(SignInError::Rejected)),
            Err(e) => return Ok(Err(e)),
        }
        let person = key(person);
        let salt = SaltString::generate(&mut OsRng);
        let phc = Argon2::default().hash_password(new.as_bytes(), &salt).map_err(|e| anyhow::anyhow!("hash: {e}"))?.to_string();
        let mut g = self.client().await?;
        let c = g.as_mut().context("accounts connection")?;
        let tx = c.transaction().await?;
        tx.batch_execute("SELECT pg_advisory_xact_lock(hashtext('magnus-accounts'))").await?;
        let n = tx.execute("UPDATE public.account_passwords SET phc = $2, temporary = false WHERE person = $1", &[&person, &phc]).await?;
        anyhow::ensure!(n == 1, "{person} holds no password here");
        tx.execute("DELETE FROM public.account_reset_tokens WHERE person = $1", &[&person]).await?;
        let mut ended = vec![];
        for r in tx.query("DELETE FROM public.account_sessions WHERE person = $1 AND hash <> $2 RETURNING id", &[&person, &token_hash(keep)]).await? {
            ended.push(r.get("id"));
        }
        tx.commit().await?;
        self.refresh_local(c).await?;
        Ok(Ok(ended))
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{cfg, db};
    use super::super::{Accounts, SignInError};

    const OLD: &str = "the-old-password";
    const NEW: &str = "the-new-password";

    async fn ada() -> (super::super::tests::TestDb, Accounts) {
        let (d, url) = db().await;
        let accounts = Accounts::open(&url, cfg()).await.unwrap();
        accounts.add_person("ada", "Ada", Some("ada@example.com")).await.unwrap();
        accounts.set_password("ada", OLD).await.unwrap();
        (d, accounts)
    }

    /// The new password takes, the old one stops, the session that asked
    /// stays and every other one ends, and a recovery link mailed before
    /// no longer works.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_change_keeps_this_session_and_ends_the_others() {
        let (_db, accounts) = ada().await;
        let (_, here) = accounts.open_session("ada", "app", None).await.unwrap();
        let (gone, there) = accounts.open_session("ada", "browser", None).await.unwrap();
        let reset = accounts.request_reset("ada@example.com").await.unwrap().expect("a recovery link for ada");
        let Ok(ended) = accounts.change_password("ada", OLD, NEW, super::super::Via::Network, &here).await.unwrap() else { panic!("the right current password was refused") };
        assert_eq!(ended, vec![gone.id], "the other session did not end");
        assert!(accounts.session(&here).await.is_some(), "the session that changed the password ended");
        assert!(accounts.session(&there).await.is_none(), "another session outlived the change");
        assert!(accounts.sign_in("ada", NEW).await.unwrap().is_ok(), "the new password does not sign in");
        assert!(matches!(accounts.sign_in("ada", OLD).await.unwrap(), Err(SignInError::Rejected)), "the old password still signs in");
        assert!(!accounts.reset_live(&reset.token).await, "a recovery link from before the change still works");
    }

    /// A wrong current password changes nothing and counts on the sign-in
    /// counter: enough of them lock it, and then sign-in from there is
    /// locked too.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_wrong_current_password_is_refused_and_counted() {
        let (_db, accounts) = ada().await;
        let (_, here) = accounts.open_session("ada", "app", None).await.unwrap();
        let counter = Accounts::network_counter("ada");
        for _ in 1..cfg().max_failures {
            assert!(matches!(accounts.change_password("ada", "not-the-password", NEW, super::super::Via::Network, &here).await.unwrap(), Err(SignInError::Rejected)));
            assert!(accounts.locked(&counter).is_none(), "locked before the limit");
        }
        assert!(matches!(accounts.change_password("ada", "not-the-password", NEW, super::super::Via::Network, &here).await.unwrap(), Err(SignInError::Rejected)));
        assert!(accounts.locked(&counter).is_some(), "wrong current passwords were not counted");
        assert!(
            matches!(accounts.change_password("ada", OLD, NEW, super::super::Via::Network, &here).await.unwrap(), Err(SignInError::Locked(_))),
            "a locked counter still changed the password"
        );
        assert!(matches!(accounts.sign_in("ada", OLD).await.unwrap(), Err(SignInError::Locked(_))), "sign-in was not locked by the same counter");
        assert!(accounts.session(&here).await.is_some(), "a refused change ended the session");
    }

    /// A new password under the length is refused before the current one is
    /// checked: nothing changes, nothing ends, and no attempt is counted.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_short_new_password_is_refused_uncounted() {
        let (_db, accounts) = ada().await;
        let (_, here) = accounts.open_session("ada", "app", None).await.unwrap();
        let (_, there) = accounts.open_session("ada", "browser", None).await.unwrap();
        let counter = Accounts::network_counter("ada");
        for _ in 0..cfg().max_failures {
            let Err(e) = accounts.change_password("ada", "not-the-password", "short", super::super::Via::Network, &here).await else { panic!("a short new password was taken") };
            assert!(e.to_string().contains("at least"), "{e:#}");
        }
        assert!(accounts.locked(&counter).is_none(), "a short new password counted as a wrong one");
        assert!(accounts.session(&there).await.is_some(), "a refused change ended another session");
        assert!(accounts.sign_in("ada", OLD).await.unwrap().is_ok(), "a refused change replaced the password");
    }
}
