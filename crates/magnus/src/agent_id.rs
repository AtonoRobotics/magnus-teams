//! A person's agent is its own principal, not the person: `agent:<person>`,
//! named for the person it acts for, "Samuel's Magnus"
//! (decisions/2026-11-28-sharing-in-place.md). Wherever it posts or acts in
//! front of others, this is who it is.

/// Every agent id begins with this, then its person's id.
pub const PREFIX: &str = "agent:";
/// What a person types to ask their own agent in a shared chat: `@Magnus`.
pub const MENTION: &str = "Magnus";

/// The agent of this person.
pub fn of(person: &str) -> String {
    format!("{PREFIX}{person}")
}

/// The person an agent acts for, when `id` is an agent's.
pub fn person(id: &str) -> Option<&str> {
    id.strip_prefix(PREFIX).filter(|p| !p.is_empty())
}

/// Whether `id` is an agent's.
pub fn is_agent(id: &str) -> bool {
    person(id).is_some()
}

/// An agent's name, from its person's: "Samuel's Magnus".
pub fn name(person_name: &str) -> String {
    format!("{person_name}'s {MENTION}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_agent_is_its_own_id_named_for_its_person() {
        let id = of("sam");
        assert_ne!(id, "sam", "the agent shares its person's id");
        assert_eq!((person(&id), is_agent(&id), is_agent("sam"), is_agent(PREFIX)), (Some("sam"), true, false, false));
        assert_eq!(name("Samuel"), "Samuel's Magnus");
    }
}
