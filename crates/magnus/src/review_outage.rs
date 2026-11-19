//! An outage is not a decision (decisions/2026-11-19-an-outage-asks-no-one.md).
//! When a check the boundary relies on (Auto Review, the outgoing
//! classifier, the safety monitor) cannot answer, it is tried again a few
//! times while that is quick (`REVIEW_RETRY_WINDOW`); if it still cannot, the action is not done and the agent is told
//! to try it again shortly. Nobody is asked: a person shown a card because a
//! service is down has nothing more to judge than the system had.

use std::future::Future;
use std::time::Duration;

/// How many times a check is tried before it counts as unavailable.
pub const REVIEW_ATTEMPTS: u32 = 3;
/// The wait before the second try; each later try waits twice as long.
pub const REVIEW_FIRST_BACKOFF: Duration = Duration::from_millis(500);
/// No new try starts once this long has passed: a check that fails fast is
/// tried again, one that hangs to its timeout is not, so an outage costs
/// the run at most one timeout.
pub const REVIEW_RETRY_WINDOW: Duration = Duration::from_secs(10);

/// Run `check` until it answers, at most `REVIEW_ATTEMPTS` times.
pub async fn with_retries<T, F, Fut>(check: F) -> Option<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Option<T>>,
{
    retry(check, REVIEW_ATTEMPTS, REVIEW_FIRST_BACKOFF, REVIEW_RETRY_WINDOW).await
}

async fn retry<T, F, Fut>(mut check: F, attempts: u32, first_backoff: Duration, window: Duration) -> Option<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Option<T>>,
{
    let started = std::time::Instant::now();
    let mut wait = first_backoff;
    for attempt in 0..attempts {
        if let Some(answer) = check().await {
            return Some(answer);
        }
        if attempt + 1 >= attempts || started.elapsed() >= window {
            break;
        }
        tokio::time::sleep(wait).await;
        wait = wait.saturating_mul(2);
    }
    None
}

/// What the agent is told when `what` could not answer: not done, ask no
/// one, try again shortly.
pub fn unavailable_text(what: &str) -> String {
    format!("{what} is unavailable right now, so this was not done and no one was asked. Try it again in a minute, and carry on meanwhile with work that does not need it")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    const NO_WAIT: Duration = Duration::from_millis(0);
    const WIDE: Duration = Duration::from_secs(60);

    #[tokio::test]
    async fn a_check_that_answers_on_a_later_try_is_used() {
        let tries = AtomicU32::new(0);
        let got = retry(
            || {
                let n = tries.fetch_add(1, Ordering::SeqCst);
                async move { (n == 1).then_some("allow") }
            },
            3,
            NO_WAIT,
            WIDE,
        )
        .await;
        assert_eq!((got, tries.load(Ordering::SeqCst)), (Some("allow"), 2));
    }

    #[tokio::test]
    async fn a_check_that_never_answers_is_unavailable_after_its_tries() {
        let tries = AtomicU32::new(0);
        let got: Option<()> = retry(
            || {
                tries.fetch_add(1, Ordering::SeqCst);
                async { None }
            },
            3,
            NO_WAIT,
            WIDE,
        )
        .await;
        assert_eq!((got, tries.load(Ordering::SeqCst)), (None, 3));
    }

    /// A check that took the whole window is not tried again.
    #[tokio::test]
    async fn a_check_that_hangs_is_tried_once() {
        let tries = AtomicU32::new(0);
        let got: Option<()> = retry(
            || {
                tries.fetch_add(1, Ordering::SeqCst);
                async { None }
            },
            3,
            NO_WAIT,
            Duration::ZERO,
        )
        .await;
        assert_eq!((got, tries.load(Ordering::SeqCst)), (None, 1));
    }

    #[test]
    fn the_agent_is_told_nothing_was_done_and_no_one_was_asked() {
        let t = unavailable_text("Auto Review");
        assert!(t.starts_with("Auto Review is unavailable") && t.contains("was not done") && t.contains("no one was asked"), "{t}");
    }
}
