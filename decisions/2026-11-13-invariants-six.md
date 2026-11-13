# Six invariant tests brought to the rules the owner already set (owner approved, 2026-11-13)

**Choice.** `2026-11-13-invariants-six.patch` changes six invariant tests that asserted rules from before later decisions. Each still guards the same promise: nothing happens without the right person's say. The owner approved all six on 2026-11-13. It is a patch, not applied to the tree, so the owner applies it with `git apply decisions/2026-11-13-invariants-six.patch`.

| Test | Change | Follows |
|---|---|---|
| inv8_layers `a_user_layer_cannot_widen_the_organization_layer` | A permit written to `user/bob` is refused when written; mail still does not go out | 2026-11-11 layers: below `org` a layer only tightens (`policy_layers.rs`) |
| inv8_layers `an_organization_forbid_wins_over_any_lower_permit` | The lower permit is refused when written; still no update and no approval offered | Same |
| inv6 `people_set_policy_only_within_their_role` | A person's own layer needs no role, so carol leaves the refused list. Bob and carol each have a permit to their own layer refused and a forbid saved; the other refusals stay | Same |
| inv8_layers `only_holders_of_the_approver_role_can_approve`, renamed `an_ask_is_answered_by_its_requester_alone` | Bob (approver role) and alice (owner) are refused and nothing happens; the requester's own Allow works | 2026-11-12 asks go to their requester alone; spec rev 145 |
| inv8_layers `a_local_command_gets_the_network_only_as_a_reviewed_send` | Case 3's Allow covers the task (`allow_scope` `task`); it still asks before anything leaves | 2026-11-13 case-three Allow; inv4 already retargeted |
| payments `a_saved_card_is_approved_at_every_fill_and_works_only_at_its_seller` | Reads the card's own words ("saved card", "USD 35.05") instead of the reasoning fields kept off the person's surface; adds that its Allow covers this fill only; the seller checks are unchanged | Spec rev 140/148: an ask's reasoning stays off the user's surface |

**Not touched.** A person's Allow always rules (`asks::always_rule`, `rules.rs`) are compiled into the rules store, not written to a policy layer. Nothing here routes them through the layer write check.

**Proof.** On 2026-11-13 the patch was applied inside the copied tree on Strix Beta, at main `fe18577` (`ci/on-strix-beta.sh inv-six`). The whole invariant suite then ran there, with `--test-threads=2` and `MAGNUS_CHROMIUM` set.

The six changed tests all pass. That includes the saved-card checks after the changed lines, which had never run before: an approval covers one use only, and the card works only at its seller.

Of 123 tests, 116 pass. The 7 that fail are not touched by the patch:
- Four are stale against spec rev 142 ("one computer, always there") and the 2026-11-11 computer decisions, and need their own approved changes:
  - inv1 `killing_a_computer_stops_it_revokes_its_grants_and_keeps_the_user_volume`
  - inv4 `an_admin_cannot_delete_a_persons_volume`
  - inv8_layers `operator_routes_reach_a_computer_only_in_a_session_its_user_granted`
  - inv8_team `the_team_computer_holds_only_what_every_member_may_see`
- inv3 `destroying_a_computer_loses_no_user_work` drops its guest connection after a cold boot. It was diagnosed on a full disk, so it is to be judged again.
- inv3 `earlier_versions_of_a_deleted_file_leave_the_volume_when_it_goes_idle` returns a 500 at line 393. It passed on `f2bf39b`, so it is newly failing on main.
- `desktop_screens` gets past its browser setup now, and fails at line 88.
