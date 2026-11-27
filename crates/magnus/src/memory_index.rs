//! What a memory may be and when two facts are the same: the write caps,
//! the refusals and the deduplication of `memory_write` and of a fact the
//! person adds themselves.
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

/// Words that turn a fact around. "Don't" splits into "don" and "t", so
/// "t" stands for every "n't".
const NEGATIONS: &[&str] = &[
    "not", "no", "never", "nor", "none", "nothing", "nobody", "neither", "without", "cannot", "t", "dont", "doesnt", "didnt", "isnt", "arent", "wasnt", "werent", "wont", "cant",
    "shouldnt", "wouldnt", "longer",
];

/// Whether `fact` repeats `existing`: the same words whatever the case,
/// spacing or punctuation, at any length, or a near duplicate when long
/// enough for word overlap to mean anything. A fact that says no where the
/// other says yes is never a near duplicate, however many words they share:
/// "The person never wants ..." corrects "The person wants ...".
pub fn same_fact(fact: &str, existing: &str) -> bool {
    let words = |s: &str| s.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_lowercase).collect::<Vec<_>>();
    let negations = |w: &[String]| w.iter().filter(|w| NEGATIONS.contains(&w.as_str())).count();
    let (a, b) = (words(fact), words(existing));
    !a.is_empty() && (a == b || (a.len() >= 4 && negations(&a) == negations(&b) && token_overlap_similarity(fact, existing) >= DEDUP_TOKEN_OVERLAP_THRESHOLD))
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
    fn near_duplicates_are_found_by_word_overlap() {
        assert!(same_fact("User prefers concise bullet summaries in status emails", "User prefers concise bullet summaries in all status emails"), "one word more");
        assert!(!same_fact("User prefers concise summaries", "Office moved to Austin Texas"));
    }

    #[test]
    fn a_fact_that_says_no_where_the_other_says_yes_is_a_correction() {
        let wants = "The person wants a weekly project progress summary by email";
        assert!(
            token_overlap_similarity(wants, "The person never wants a weekly project progress summary by email") >= DEDUP_TOKEN_OVERLAP_THRESHOLD,
            "the words alone look the same"
        );
        for corrected in [
            "The person never wants a weekly project progress summary by email",
            "The person doesn't want a weekly project progress summary by email",
            "The person no longer wants a weekly project progress summary by email",
        ] {
            assert!(!same_fact(corrected, wants), "{corrected}");
            assert!(!same_fact(wants, corrected), "{corrected}");
        }
        let never = "The person never wants a weekly project progress summary by email";
        assert!(same_fact("The person never wants a weekly project progress summary by any email", never), "the same no, a word more, is still one fact");
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
