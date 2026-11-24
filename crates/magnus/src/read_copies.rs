//! Where an item's text was read into a run, so removing the item reaches
//! it there too (decisions/2026-11-23-removing-an-item-reaches-the-runs-that-read-it.md).
//!
//! A memory search, a knowledge search, a project file read, a playbook
//! search and a read of a team knowledge base return the items' full text
//! into the run's tool result. Each of
//! those results says which items it returned, in its data's `read_items`
//! (`read_data`); a knowledge search said it before in `entries`, and a
//! project file read in `file`. Removing a memory, a knowledge entry or a
//! file finds the results that returned it (`State::reads_of`) and names
//! them in its `Removed`, so their content leaves the log and the backups
//! as the item's own does. What a run made from what it read stays: only
//! the item's own text goes. A memory search from before results said what
//! they returned is found by the fact's text in the results of the runs
//! that searched (`State::unlabelled_memory_reads`).
//!
//! When a `Removed` is applied, the results it names lose their text in
//! the state too (`State::drop_read_results`), live as after a replay.

use crate::state::State;
use serde_json::{json, Value};
use std::collections::BTreeSet;

/// The key of a reading tool's data that lists the items it returned.
pub const READ_ITEMS: &str = "read_items";

/// A reading tool's data: `more`, with the ids of the items it returned.
pub fn read_data(ids: impl IntoIterator<Item = String>, more: Value) -> Value {
    let mut d = match more {
        Value::Object(m) => Value::Object(m),
        _ => json!({}),
    };
    d[READ_ITEMS] = json!(ids.into_iter().collect::<Vec<_>>());
    d
}

/// The items a result returned, by what its data says.
fn items_read(data: &Value) -> impl Iterator<Item = &str> {
    let listed = [READ_ITEMS, "entries"].into_iter().flat_map(move |k| data[k].as_array().into_iter().flatten().filter_map(Value::as_str));
    listed.chain(data["file"].as_str())
}

impl State {
    /// The tool results that returned this item's text into a run.
    pub fn reads_of(&self, item: &str) -> Vec<i64> {
        let results = self.runs.values().flat_map(|r| r.results.values().chain(r.intents.values().filter_map(|i| i.result.as_ref())));
        let seqs: BTreeSet<i64> = results.filter(|x| x.data.as_ref().is_some_and(|d| items_read(d).any(|i| i == item))).map(|x| x.seq).collect();
        seqs.into_iter().collect()
    }

    /// For a memory of `person`'s: each run of theirs that searched their
    /// memory before results said what they returned, to find the fact's
    /// text (`text`, as the content store writes it) in its tool results.
    pub fn unlabelled_memory_reads(&self, person: &str, text: &str) -> Vec<(&'static str, String, String)> {
        let unlabelled = |r: &crate::state::Run| {
            r.intents.values().any(|i| i.tool == "memory_search" && i.result.as_ref().is_some_and(|x| x.data.as_ref().is_none_or(|d| d.get(READ_ITEMS).is_none())))
        };
        self.runs.values().filter(|r| r.person == person && unlabelled(r)).map(|r| ("tool_result", r.id.clone(), text.to_string())).collect()
    }

    /// Results whose content was removed hold nothing in the state either.
    pub(crate) fn drop_read_results(&mut self, seqs: &[i64]) {
        let gone: BTreeSet<i64> = seqs.iter().copied().collect();
        for r in self.runs.values_mut() {
            for x in r.results.values_mut().chain(r.intents.values_mut().filter_map(|i| i.result.as_mut())) {
                if gone.contains(&x.seq) {
                    x.text.clear();
                    x.data = None;
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "read_copies_tests.rs"]
mod tests;
