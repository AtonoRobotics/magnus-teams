//! How long each step of a wake or a hibernation took, for the log and for
//! Magnus, which logs a wake's whole time with these parts.

use serde::Serialize;
use std::collections::BTreeMap;
use std::time::Instant;

#[derive(Serialize, Debug, Clone, Default)]
#[serde(transparent)]
pub struct Steps(BTreeMap<&'static str, u64>);

impl Steps {
    pub fn total_ms(&self) -> u64 {
        self.0.values().sum()
    }

    /// "open 38012 ms, launch 1100 ms": for a log line.
    pub fn words(&self) -> String {
        self.0.iter().map(|(step, ms)| format!("{step} {ms} ms")).collect::<Vec<_>>().join(", ")
    }
}

pub struct Took {
    since: Instant,
    steps: Steps,
}

impl Took {
    pub fn start() -> Took {
        Took { since: Instant::now(), steps: Steps::default() }
    }

    /// The time since the last mark, as `step`.
    pub fn mark(&mut self, step: &'static str) {
        let ms = u64::try_from(self.since.elapsed().as_millis()).unwrap_or(u64::MAX);
        *self.steps.0.entry(step).or_default() += ms;
        self.since = Instant::now();
    }

    pub fn steps(&self) -> Steps {
        self.steps.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_step_is_the_time_since_the_last() {
        let mut t = Took::start();
        t.mark("a");
        t.mark("b");
        t.mark("a");
        let s = t.steps();
        assert_eq!(s.0.len(), 2, "a step marked twice is one step");
        assert!(s.words().contains("a ") && s.words().contains("b "));
        assert_eq!(s.total_ms(), s.0.values().sum::<u64>());
    }
}
