//! Money held for model calls in flight (decisions/2026-11-23-one-ledger-for-money.md).
//!
//! A call's cost is known only once the provider answers, so two calls made
//! at once would each see the whole of what a limit has left, and together
//! pass it. Each paid call therefore holds its worst case, at the price the
//! adapter quotes, from the moment it is admitted until its cost is on the
//! ledger: every other call's allowance reads what is left less what is
//! held. Admission is one step under one lock: what is left is read, the
//! call is fitted to it, and the hold taken, so no two calls fit into the
//! same dollars. The holds live in memory only: they are never in the log,
//! and none outlives the call, so a replay has none to fold.
//!
//! A batch of calls a run makes at once holds its actions the same way
//! (`reserve`): the batch is admitted in one step, and the actions it was
//! given count against every other run of its tree until its own calls
//! record them (decisions/2026-11-23-a-batch-of-sub-agents-is-counted-before-it-starts.md).

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

/// How long a call that does not fit waits for calls in flight to finish
/// and free what they hold, before it is refused or its run stopped.
pub const HELD_CALL_WAIT_SECS: Duration = Duration::from_secs(300);

/// What a call in flight holds, and against which limits: the runs whose
/// budget it spends (the run and those above it), the grant it acts under,
/// the person whose month it counts in, and the team's workspace and
/// channel. The server's month counts every hold.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Hold {
    pub runs: Vec<String>,
    pub grant: Option<String>,
    pub person: Option<String>,
    pub workspace: Option<String>,
    pub channel: Option<String>,
    pub usd: f64,
    /// Actions held for a batch of the run's (`runs[0]`) calls made at
    /// once: they count against every other run whose limits the run
    /// spends, never against the run's own calls, which they are for.
    pub actions: u32,
    /// The run's own action count when they were held: each action it has
    /// recorded since used one of them (`spend::tree`).
    pub actions_at: u32,
}

/// The holds of the calls in flight.
#[derive(Default)]
pub struct Holds {
    held: Mutex<HashMap<u64, Hold>>,
    next: AtomicU64,
    released: tokio::sync::Notify,
}

/// A hold taken; dropping it frees the dollars and wakes calls waiting.
pub struct Held<'a> {
    holds: &'a Holds,
    id: u64,
}

impl Drop for Held<'_> {
    fn drop(&mut self) {
        self.holds.held.lock().unwrap_or_else(|e| e.into_inner()).remove(&self.id);
        self.holds.released.notify_waiters();
    }
}

impl Holds {
    /// The holds of the calls in flight now.
    pub fn now(&self) -> Vec<Hold> {
        self.held.lock().unwrap_or_else(|e| e.into_inner()).values().cloned().collect()
    }

    /// Admit a call: `fit` reads what is left given the holds of the calls
    /// in flight, and says what the call may do and the dollars to hold,
    /// or why it may not. A call that does not fit only because of what
    /// others hold waits for them to finish, up to [`HELD_CALL_WAIT_SECS`], and
    /// is fitted again; one that would not fit anyway is refused at once.
    pub async fn admit<T, E>(&self, scope: Hold, mut fit: impl FnMut(&[Hold]) -> Result<(T, f64), E>) -> Result<(T, Held<'_>), E> {
        let deadline = tokio::time::Instant::now() + HELD_CALL_WAIT_SECS;
        loop {
            let released = self.released.notified();
            {
                let mut held = self.held.lock().unwrap_or_else(|e| e.into_inner());
                let now: Vec<Hold> = held.values().cloned().collect();
                match fit(&now) {
                    Ok((t, usd)) => {
                        let id = self.next.fetch_add(1, Ordering::SeqCst);
                        held.insert(id, Hold { usd, ..scope });
                        return Ok((t, Held { holds: self, id }));
                    }
                    Err(why) if now.is_empty() || fit(&[]).is_err() || tokio::time::Instant::now() >= deadline => {
                        return Err(why);
                    }
                    Err(_) => {}
                }
            }
            if tokio::time::timeout_at(deadline, released).await.is_err() {
                // Fitted once more at the deadline, then refused.
                continue;
            }
        }
    }
}

impl Holds {
    /// Hold actions for a batch of calls about to be made at once: `fit`
    /// reads what is left given the holds now and gives what goes ahead
    /// and the hold to take, in one step under the lock, so no two calls of
    /// a batch, nor two batches, are fitted into the same actions. Dropping
    /// the hold frees what the batch's calls did not use.
    pub fn reserve<T>(&self, fit: impl FnOnce(&[Hold]) -> (T, Hold)) -> (T, Held<'_>) {
        let mut held = self.held.lock().unwrap_or_else(|e| e.into_inner());
        let now: Vec<Hold> = held.values().cloned().collect();
        let (t, hold) = fit(&now);
        let id = self.next.fetch_add(1, Ordering::SeqCst);
        held.insert(id, hold);
        (t, Held { holds: self, id })
    }
}

/// The dollars held against the limits `scope` names: (its run's budget,
/// its grant, its person's month, the server's month, its workspace, its
/// channel).
pub fn held_for(holds: &[Hold], scope: &Hold) -> HeldFor {
    let mut out = HeldFor::default();
    for h in holds {
        out.server += h.usd;
        if scope.runs.first().is_some_and(|r| h.runs.contains(r)) {
            out.run += h.usd;
        }
        let same = |a: &Option<String>, b: &Option<String>| a.is_some() && a == b;
        if same(&h.grant, &scope.grant) {
            out.grant += h.usd;
        }
        if same(&h.person, &scope.person) {
            out.person += h.usd;
        }
        if same(&h.workspace, &scope.workspace) {
            out.workspace += h.usd;
        }
        if same(&h.workspace, &scope.workspace) && same(&h.channel, &scope.channel) {
            out.channel += h.usd;
        }
    }
    out
}

/// Dollars held, by the limit they count against.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HeldFor {
    pub run: f64,
    pub grant: f64,
    pub person: f64,
    pub server: f64,
    pub workspace: f64,
    pub channel: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn of(person: &str, usd: f64) -> Hold {
        Hold { runs: vec![format!("r-{person}")], person: Some(person.into()), usd, ..Default::default() }
    }

    #[test]
    fn a_hold_counts_against_its_own_limits_and_the_servers() {
        let holds = [of("ada", 1.0), of("bob", 2.0)];
        let h = held_for(&holds, &of("ada", 0.0));
        assert_eq!((h.run, h.person, h.server, h.workspace), (1.0, 1.0, 3.0, 0.0));
    }

    /// A call that does not fit while another of the same person's is in
    /// flight waits for it, and is fitted again once it is done.
    #[tokio::test]
    async fn a_call_that_does_not_fit_waits_for_the_one_in_flight() {
        let holds = Holds::default();
        let Ok(((), first)) = holds.admit(of("ada", 0.0), |_| Ok::<_, ()>(((), 1.0))).await else { panic!("the first call did not fit") };
        let limit = 1.5;
        let fit = |held: &[Hold]| {
            let left = limit - held_for(held, &of("ada", 0.0)).person;
            if left >= 1.0 {
                Ok(((), 1.0))
            } else {
                Err(())
            }
        };
        let waiting = holds.admit(of("ada", 0.0), fit);
        let release = async {
            tokio::task::yield_now().await;
            drop(first);
        };
        let (admitted, ()) = tokio::join!(waiting, release);
        assert!(admitted.is_ok(), "the second call was refused though the first had finished");
    }

    /// One that does not fit with nothing in flight of its own is refused
    /// at once.
    #[tokio::test]
    async fn a_call_that_cannot_fit_is_refused_at_once() {
        let holds = Holds::default();
        let _other = holds.admit(of("bob", 0.0), |_| Ok::<_, ()>(((), 5.0))).await;
        assert!(holds.admit(of("ada", 0.0), |_| Err::<((), f64), _>(())).await.is_err());
    }
}
