# Clean-only always-on context for Playbook, Commitments, and Team KB (W1.2, W1.3)

**Choice.** Adopt one uniform rule across Playbook, Commitments, and Team KB:
1. Always-on context carries **clean items only** (`!label.untrusted`, `!Weight::Outside`).
2. Untrusted items remain stored with their existing `Label` / `Weight::Outside` (zero nested-enum schema changes, preserving binary rollback) and are surfaced via pinned, clean count pointers in always-on context (pinned at `run.config.playbook` and task creation `seq` so C1 prompt caching never shifts mid-run).
3. On-demand read tools—new `playbook_search` and `commitments_search` in `crates/magnus/src/tainted_recall.rs`, plus `team_read_knowledge`—use `Taint::Returned` and return the joined `Label` of the entries shown so reading untrusted items taints only the reading run.
4. New `Ev::TaskCreated` events record their clean starting `label` explicitly alongside `#[serde(default)] explicit_label: bool`. `State::apply` trusts the recorded label when `explicit_label` is `true` and preserves legacy replay folding for historical events.

**Reason.** Closes Team KB taint laundering (where `Weight::Outside` entries entered future team runs untainted) and eliminates permanent taint DoS (where a single web-tainted playbook item or email-extracted commitment tainted every subsequent run and permanently blocked `relationship_note`), with zero approval cards or user tending.
