//! What a task may spend comes from the server, never from the model
//! (spec rev 133, Chat and tasks): "A task's budget defaults to the
//! server's per-task limit. A model's cost estimate is shown to the
//! person, never used as the cap." and "Reviews and other background
//! wakes run on the per-task budget."
//!
//! Every place a task starts asks here: a proposal or suggestion the
//! person approved, a confirmed schedule's runs, a task under a grant, a
//! proactive review or a watch poll. What the model estimated or asked for
//! travels on the card for the person to read and is ignored here, so a
//! card written before this change, carrying the model's figure as its
//! budget, starts with the server's limit too. Auto Review and the other
//! reviewer calls of a run are charged to that run (`reviewer_call`), so
//! they share this budget and are limited by it alone.

use crate::config::Budget;

/// What a task starts with: the server's per-task limit, whatever the
/// model estimated or asked for.
pub(crate) fn for_task(server: &Budget) -> Budget {
    server.clone()
}

/// What a task started from an approved card or inbox proposal starts
/// with. `_stored` is whatever budget the card carries: a card written
/// before rev 133 carries the model's estimate or ask there, so it is
/// never read as the limit.
pub(crate) fn approved(server: &Budget, _stored: &serde_json::Value) -> Budget {
    for_task(server)
}

/// A confirmed schedule's run. A schedule confirmed before rev 133 may
/// carry the budget its card had, which was the model's ask; it is ignored.
pub(crate) fn scheduled(server: &Budget, _stored: Option<&Budget>) -> Budget {
    for_task(server)
}

/// A task under a grant: the server's per-task limit, its dollars no more
/// than what is left of the grant this period. The grant's figure is the
/// person's own, not the model's.
pub(crate) fn under_grant(server: &Budget, left_usd: f64) -> Budget {
    let mut b = for_task(server);
    b.max_usd = b.max_usd.min(left_usd);
    b
}

/// A background wake (a proactive review, a watch poll): the per-task
/// budget, not a smaller one of its own.
pub(crate) fn for_wake(server: &Budget) -> Budget {
    for_task(server)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server() -> Budget {
        Budget { max_model_calls: 300, max_actions: 500, max_usd: 50.0 }
    }

    fn same(a: &Budget, b: &Budget) -> bool {
        (a.max_model_calls, a.max_actions, a.max_usd) == (b.max_model_calls, b.max_actions, b.max_usd)
    }

    #[test]
    fn a_task_starts_with_the_servers_limit() {
        assert!(same(&for_task(&server()), &server()));
    }

    /// A card or schedule written before rev 133 carries the model's
    /// estimate as its budget; approving it starts the server's.
    #[test]
    fn an_approved_card_starts_with_the_servers_limit_not_the_one_it_carries() {
        let stored = serde_json::json!({ "max_model_calls": 300, "max_actions": 500, "max_usd": 0.75 });
        assert!(same(&approved(&server(), &stored), &server()), "the model's figure capped the task");
        assert!(same(&approved(&server(), &serde_json::Value::Null), &server()));
    }

    /// The teams server's failed schedule run of 2026-11-10: the schedule
    /// was proposed with the model's own estimate of 3 model calls and
    /// $0.50 as its budget. Its runs, and a new card's, take the server's.
    #[test]
    fn a_schedule_proposed_with_a_too_small_estimate_runs_on_the_servers_budget() {
        let stored = Budget { max_model_calls: 3, max_actions: 1, max_usd: 0.5 };
        assert!(same(&scheduled(&server(), Some(&stored)), &server()), "the stored estimate capped the run");
        assert!(same(&scheduled(&server(), None), &server()));
        let card = serde_json::json!({ "max_model_calls": 3, "max_actions": 1, "max_usd": 0.5 });
        assert!(same(&approved(&server(), &card), &server()), "the card's estimate capped the schedule");
    }

    #[test]
    fn a_grant_lowers_only_the_dollars_and_only_to_what_is_left() {
        let b = under_grant(&server(), 2.5);
        assert_eq!((b.max_model_calls, b.max_actions, b.max_usd), (300, 500, 2.5));
        assert!(same(&under_grant(&server(), 80.0), &server()), "a grant raised the server's limit");
    }

    /// No chat tool lets the model set a task's or a schedule's budget.
    #[test]
    fn no_chat_tool_takes_a_budget_from_the_model() {
        for t in crate::tools::chat_specs() {
            assert!(t.input_schema["properties"].get("budget").is_none(), "{} takes a budget from the model", t.name);
        }
    }

    #[test]
    fn a_wake_runs_on_the_per_task_budget() {
        assert!(same(&for_wake(&server()), &server()));
    }
}
