//! The server's own directory rows: adding a person, and the recovery
//! address. People named only in the config have no row until one is
//! written here, and without an address a recovery email never finds them.

use super::{key, valid_person_id, Accounts};
use anyhow::{Context, Result};

impl Accounts {
    /// Add a person to the server's own directory. Refuses a second
    /// person with the id, and an id outside 1 to 64 letters, digits,
    /// dots, dashes and underscores.
    pub async fn add_person(&self, id: &str, name: &str, email: Option<&str>) -> Result<()> {
        let id = key(id);
        anyhow::ensure!(valid_person_id(&id), "a person id is 1 to 64 letters, digits, dots, dashes and underscores");
        let mut g = self.client().await?;
        let c = g.as_mut().context("accounts connection")?;
        let n = c.execute("INSERT INTO public.account_people (person, name, email) VALUES ($1, $2, $3) ON CONFLICT (person) DO NOTHING", &[&id, &name, &email]).await?;
        anyhow::ensure!(n == 1, "{id} is already a person here");
        self.refresh_local(c).await?;
        Ok(())
    }

    /// Set (or with `None`, clear) the address recovery emails go to.
    /// Someone named only in the config gets a directory row under `name`;
    /// an existing row keeps its name.
    pub async fn set_email(&self, id: &str, name: &str, email: Option<&str>) -> Result<()> {
        let id = key(id);
        anyhow::ensure!(valid_person_id(&id), "a person id is 1 to 64 letters, digits, dots, dashes and underscores");
        let email = email.map(|e| e.trim().to_lowercase());
        let mut g = self.client().await?;
        let c = g.as_mut().context("accounts connection")?;
        c.execute("INSERT INTO public.account_people (person, name, email) VALUES ($1, $2, $3) ON CONFLICT (person) DO UPDATE SET email = EXCLUDED.email", &[&id, &name, &email])
            .await?;
        self.refresh_local(c).await
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{cfg, db};
    use super::super::Accounts;

    /// Someone with a password but no directory row (as the config's owner
    /// is) cannot be found by a recovery request until an address is set;
    /// then they can, a new address replaces the old, and clearing it stops
    /// recovery again.
    #[tokio::test(flavor = "multi_thread")]
    async fn an_address_makes_a_config_person_recoverable() {
        let (_db, url) = db().await;
        let accounts = Accounts::open(&url, cfg()).await.unwrap();
        accounts.set_password("samuel", "the-first-password").await.unwrap();
        assert!(accounts.request_reset("owner@example.com").await.unwrap().is_none(), "found before any address was set");
        accounts.set_email("samuel", "Samuel", Some("Owner@Example.com ")).await.unwrap();
        assert_eq!(accounts.local_person("samuel").and_then(|p| p.email).as_deref(), Some("owner@example.com"));
        let r = accounts.request_reset("owner@example.com").await.unwrap().expect("the owner is now recoverable");
        assert_eq!(r.person, "samuel");
        accounts.set_email("samuel", "ignored", Some("new@example.com")).await.unwrap();
        assert_eq!(accounts.local_person("samuel").map(|p| p.name).as_deref(), Some("Samuel"), "an existing row lost its name");
        assert!(accounts.request_reset("owner@example.com").await.unwrap().is_none(), "the old address still recovers");
        accounts.set_email("samuel", "Samuel", None).await.unwrap();
        assert!(accounts.request_reset("new@example.com").await.unwrap().is_none(), "a cleared address still recovers");
    }
}
