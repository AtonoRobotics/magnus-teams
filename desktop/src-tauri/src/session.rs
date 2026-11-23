//! Everything on this computer that belongs to the person signed in: their
//! session token (in the store), who they are, the local agent working in
//! their folders, the name the bridge gives this computer for them, and the
//! live streams of their tasks. Sign-out, or a session the server ended,
//! calls `end`, which ends all of it at once; the next person starts with
//! none of it.
use crate::live::Streams;
use crate::local::{Agent, Connection};
use crate::store::Store;
use std::sync::{Mutex, MutexGuard, PoisonError};

#[derive(Default)]
pub struct Session {
    /// The local agent and the connection it serves.
    local: Mutex<Option<(Agent, Connection)>>,
    pub live: Streams,
    /// Wakes the bridge: to report again now, or because the person changed.
    pub wake: tokio::sync::Notify,
}

/// What is kept for one person at one server is kept under this, so another
/// person on this computer, or the same person at another server, never
/// finds it.
pub fn key(server: &str, person: &str) -> String {
    use sha2::Digest;
    let mut h = sha2::Sha256::new();
    h.update(server.as_bytes());
    h.update([0]);
    h.update(person.as_bytes());
    hex::encode(h.finalize())[..32].to_string()
}

impl Session {
    pub fn local(&self) -> MutexGuard<'_, Option<(Agent, Connection)>> {
        self.local.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The key of whoever is signed in now, if anyone is known to be.
    pub fn key_of(store: &Store) -> Option<String> {
        let url = store.config().appliance?;
        let person = store.person(&url)?;
        Some(key(&url, &person))
    }

    /// `person` signed in at `url`. Anything still here for someone else ends.
    pub fn begin(&self, store: &Store, url: &str, person: &str) -> Result<(), String> {
        if store.person(url).as_deref() != Some(person) {
            self.end_here();
        }
        store.set_person(url, person)?;
        self.wake.notify_one();
        Ok(())
    }

    /// The person's session at `url` is over: its token, who they were, and
    /// everything that ran for them.
    pub fn end(&self, store: &Store, url: &str) {
        store.clear_token(url);
        store.clear_person(url);
        self.end_here();
    }

    /// Stop what runs for the person: the local agent (its process is
    /// killed as it goes), the live streams, and the bridge's poll.
    fn end_here(&self) {
        *self.local() = None;
        for job in self.live.end() {
            job.abort();
        }
        self.wake.notify_one();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_person_at_each_server_has_their_own_key() {
        let a = key("https://magnus.local:7443", "ana");
        assert_eq!(a, key("https://magnus.local:7443", "ana"), "the same person's key changed");
        assert_ne!(a, key("https://magnus.local:7443", "ben"), "two people share what this computer keeps");
        assert_ne!(a, key("https://other.local:7443", "ana"), "two servers share what this computer keeps");
        assert_ne!(key("https://a", "bc"), key("https://ab", "c"), "the server and the person run together");
    }

    /// Sign-out stops the local agent: its process is gone, so the next
    /// person's Magnus cannot work in the last one's folders.
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn the_end_stops_the_local_agent_and_the_streams() {
        let s = Session::default();
        let mut command = std::process::Command::new("sleep");
        command.arg("30");
        let Ok(agent) = crate::local::launch(command) else { panic!("it did not start") };
        let process = format!("/proc/{}", agent.id());
        *s.local() = Some((agent, Connection::for_test()));
        s.live.join("t1", "chat", |_| {});
        let job = tokio::spawn(std::future::pending::<()>());
        assert!(s.live.started("t1", job.abort_handle()));
        s.end_here();
        assert!(s.local().is_none());
        assert!(!std::path::Path::new(&process).exists(), "the local agent outlived the sign-out");
        assert!(job.await.is_err_and(|e| e.is_cancelled()), "a live stream outlived the sign-out");
    }
}
