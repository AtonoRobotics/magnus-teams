//! Whose a knowledge entry or a playbook lesson is: one rule, read by the
//! log as it writes (`State::content_owner`), by `migrate` for rows written
//! before (`owners_sql`), and by an erasure (`Kernel::erased_knowledge`).
//!
//! An entry only one person may read is theirs: some clause of its readers
//! names that person alone, and no other clause names another person alone
//! (`decisions/2026-11-11-a-persons-knowledge-and-rules-stay-theirs.md`).
//! Its readers are those it has now: an entry shared with a group or with
//! everyone is no longer one person's. Any other entry is the
//! organisation's. A lesson is its person's: the playbook is kept per person.

use crate::kernel::Kernel;
use crate::log::Holding;
use crate::state::State;
use anyhow::Result;
use std::collections::HashSet;

/// The person an entry with these readers belongs to, if any.
pub fn entry_owner(readers: &[Vec<String>]) -> Option<&str> {
    let mut alone = readers.iter().filter_map(|clause| match clause.as_slice() {
        [one] => one.strip_prefix("person:"),
        _ => None,
    });
    let first = alone.next()?;
    alone.all(|p| p == first).then_some(first)
}

/// `entry_owner` in SQL, of the readers (a JSON array of clauses) `readers` evaluates to.
fn owner_of(readers: &str) -> String {
    format!(
        "(SELECT CASE WHEN count(DISTINCT c->>0) = 1 THEN substr(min(c->>0), 8) END
          FROM jsonb_array_elements(CASE WHEN jsonb_typeof({readers}) = 'array' THEN {readers} ELSE '[]'::jsonb END) AS c
          WHERE jsonb_typeof(c) = 'array' AND jsonb_array_length(c) = 1 AND c->>0 LIKE 'person:%')"
    )
}

/// Make each entry's content row say whose it is now, by its latest
/// readers (its last share, else those it was filed with). Run by
/// `migrate`, after the rows written before it said.
pub fn owners_sql() -> String {
    let now = "COALESCE((SELECT s.body->'readers' FROM public.events s WHERE s.kind = 'knowledge_shared' AND s.body->>'id' = e.body->>'id' ORDER BY s.seq DESC LIMIT 1), e.body->'readers')";
    format!(
        "UPDATE public.content c SET person = o.person FROM (
             SELECT e.seq, {} AS person FROM public.events e WHERE e.kind = 'knowledge_added'
         ) o WHERE c.seq = o.seq AND c.person IS DISTINCT FROM o.person;",
        owner_of(now)
    )
}

impl State {
    /// The events holding knowledge entries, in the library and in the
    /// trash: those that are `person`'s, and those that are not.
    fn entries_of(&self, person: &str) -> (Vec<i64>, HashSet<i64>) {
        let entries = self.library.knowledge().values().chain(self.trash.items().values().filter_map(|t| t.knowledge()));
        let (mut theirs, mut others) = (vec![], HashSet::new());
        for e in entries {
            match entry_owner(&e.readers) == Some(person) {
                true => theirs.push(e.seq),
                false => {
                    others.insert(e.seq);
                }
            }
        }
        (theirs, others)
    }
}

impl Kernel {
    /// An erasure's content, by the one rule: their entries and their
    /// lessons go, and an entry of theirs since shared stays, whatever the
    /// store's own column says of it (a row written before `migrate` set it).
    pub(crate) async fn erased_knowledge(&self, person: &str, seqs: &mut Vec<i64>) -> Result<()> {
        let (theirs, others) = self.read(|s| s.entries_of(person));
        seqs.retain(|q| !others.contains(q));
        seqs.extend(theirs);
        let lessons = Holding { kinds: vec!["playbook"], field: "person", id: person.into(), ..Default::default() };
        seqs.extend(self.log.content_seqs(&lessons).await?);
        Ok(())
    }
}

#[cfg(test)]
#[path = "knowledge_owner_tests.rs"]
mod tests;
