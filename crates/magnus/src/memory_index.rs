//! What a memory may be, how two facts are the same, and how recall ranks
//! what matched: the write caps, the refusals and deduplication of
//! `memory_write`, and the score `memory_search` sorts by (`memory_hits`).
//! Memory is `Ev::Memory` events with their content rows; recall searches
//! it where it is kept (`recall::Recall::search`).

use std::collections::HashSet;

/// Named constants with explicit units (AGENTS.md Code Standard).
pub const MAX_MEMORY_WRITES_PER_RUN: usize = 12;
/// A memory is one short sentence about the person. The longest the
/// person-taught facts in production ran was under 400 characters; the
/// longer entries were run checkpoints and status reports.
pub const MAX_MEMORY_FACT_CHARS: usize = 300;
pub const DEDUP_TOKEN_OVERLAP_THRESHOLD: f32 = 0.85;
/// The rank offset of reciprocal-rank scoring: a match's text rank `r`
/// scores `1 / (RRF_K + r + 1)`.
pub const RRF_K: f64 = 60.0;
pub const TIER2_WEIGHT: f64 = 0.65;
pub const RECENCY_HALF_LIFE_DAYS: f64 = 30.0;
pub const SECONDS_PER_DAY: f64 = 86_400.0;

/// A matched memory as recall scores it.
#[derive(Clone, Debug)]
pub struct RankedCandidate {
    /// Its place in the text-rank order, best first.
    pub fts_rank: usize,
    pub untrusted: bool,
    pub age_days: f64,
}

/// The score recall sorts matches by: the text rank's reciprocal, decayed
/// with age (half-life `RECENCY_HALF_LIFE_DAYS`, never below half), and
/// down-weighted for untrusted facts (`TIER2_WEIGHT`).
pub fn hybrid_score(c: &RankedCandidate) -> f64 {
    let base = 1.0 / (RRF_K + (c.fts_rank as f64) + 1.0);
    let decay = 0.5_f64.powf((c.age_days.max(0.0)) / RECENCY_HALF_LIFE_DAYS);
    let recency_factor = 0.5 + 0.5 * decay;
    let trust_factor = if c.untrusted { TIER2_WEIGHT } else { 1.0 };
    base * recency_factor * trust_factor
}

/// Jaccard overlap of the words of two texts, for near-duplicate facts.
pub fn token_overlap_similarity(a: &str, b: &str) -> f32 {
    let tokens = |s: &str| -> HashSet<String> { s.split(|c: char| !c.is_alphanumeric()).filter(|w| w.len() >= 2).map(|w| w.to_ascii_lowercase()).collect() };
    let sa = tokens(a);
    let sb = tokens(b);
    if sa.is_empty() || sb.is_empty() {
        return 0.0;
    }
    let inter = sa.intersection(&sb).count() as f32;
    let union = sa.union(&sb).count() as f32;
    inter / union
}

/// Whether `fact` repeats `existing`: the same words whatever the case,
/// spacing or punctuation, at any length, or a near duplicate when long
/// enough for word overlap to mean anything.
pub fn same_fact(fact: &str, existing: &str) -> bool {
    let words = |s: &str| s.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_lowercase).collect::<Vec<_>>();
    let (a, b) = (words(fact), words(existing));
    !a.is_empty() && (a == b || (a.len() >= 4 && token_overlap_similarity(fact, existing) >= DEDUP_TOKEN_OVERLAP_THRESHOLD))
}

/// Why `fact` may not be remembered, if it may not: memory keeps short
/// facts the person taught, so not an empty one, not a paragraph (a run's
/// checkpoint or report), and nothing from a run the person said nothing
/// in, which is the system's dialog with itself (a review, a heartbeat, a
/// schedule's run).
pub fn fact_refusal(fact: &str, person_spoke: bool) -> Option<String> {
    if fact.is_empty() {
        return Some("memory_write needs a fact.".into());
    }
    let n = fact.chars().count();
    if n > MAX_MEMORY_FACT_CHARS {
        return Some(format!("Not remembered: a memory is one short sentence about the person (at most {MAX_MEMORY_FACT_CHARS} characters), and this is {n}. Run notes, checkpoints and reports are not memories."));
    }
    if !person_spoke {
        return Some(
            "Not remembered: memory keeps what the person told Magnus, and the person said nothing in this run (a review, a schedule's run or a check Magnus started itself)."
                .into(),
        );
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_facts_outrank_equal_untrusted_facts_and_newer_outrank_older() {
        let clean = RankedCandidate { fts_rank: 0, untrusted: false, age_days: 1.0 };
        let untrusted = RankedCandidate { untrusted: true, ..clean.clone() };
        assert!(hybrid_score(&clean) > hybrid_score(&untrusted));
        let old = RankedCandidate { age_days: 90.0, ..clean.clone() };
        assert!(hybrid_score(&clean) > hybrid_score(&old));
        assert!(hybrid_score(&old) > hybrid_score(&clean) / 2.0, "age halves a score at most");
    }

    #[test]
    fn near_duplicates_are_found_by_word_overlap() {
        assert!(same_fact("User prefers concise bullet summaries in status emails", "User prefers concise bullet summaries in all status emails"), "one word more");
        assert!(!same_fact("User prefers concise summaries", "Office moved to Austin Texas"));
    }

    #[test]
    fn the_same_fact_is_one_memory_at_any_length() {
        assert!(same_fact("Calls me Mags.", "calls me mags"), "short facts repeat too");
        assert!(same_fact("The person prefers to call the assistant Mags.", "The person prefers to call the assistant Mags"));
        assert!(!same_fact("Calls me Mags.", "Calls me Sam."));
        assert!(!same_fact("", ""), "nothing is not a fact");
    }

    #[test]
    fn memory_refuses_paragraphs_and_the_systems_own_runs() {
        assert_eq!(fact_refusal("The person is allergic to peanuts.", true), None);
        let checkpoint = "RUN 1 CHECKPOINT phase=awaiting_continuity_check ".repeat(8);
        assert!(fact_refusal(&checkpoint, true).is_some_and(|r| r.contains("short sentence")), "a run's checkpoint is not a memory");
        assert!(fact_refusal("The person's latest review found a promotion.", false).is_some_and(|r| r.contains("said nothing")), "a review's report is not a memory");
        assert!(fact_refusal("", true).is_some());
    }
}
