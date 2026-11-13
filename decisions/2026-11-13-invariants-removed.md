# Invariant test parts that tested removed behaviour are removed

`2026-11-13-invariants-removed.patch` removes the parts of four invariant tests that checked behaviour deliberately taken out. Those parts are no longer needed. The rest of each test is unchanged. Apply this patch after `2026-11-13-invariants-six.patch`.

- **inv1 `killing_a_computer_stops_it_revokes_its_grants_and_keeps_the_user_volume`.** Removed: deleting the volume makes it vanish. Since spec rev 142 ("one computer, always there"), deleting the volume erases it to a fresh, empty one.
- **inv4 `an_admin_cannot_delete_a_persons_volume`.** Removed: an admin destroys a person's computer. Since `2026-11-11-a-persons-computer-is-theirs-alone.md`, an admin may only pause it.
- **inv8_layers `operator_routes_reach_a_computer_only_in_a_session_its_user_granted`.** Removed: bob's second computer and the check against it. Since rev 142 a person has one computer.
- **inv8_team `the_team_computer_holds_only_what_every_member_may_see`.** Removed: "no computer running" before the workspace has one. Since rev 142 the workspace always has its computer, made on first use.

**Stale, updated (owner, 2026-11-13: "accept 404 too").** inv4's remaining check expected 403 when an admin asks to delete a person's volume. The route answers 404 by design (`computer/service.rs`), so it does not say whether the computer exists. The check now accepts 403 or 404. The volume must still survive.

**Not stale.**
- inv3 `destroying_a_computer_loses_no_user_work` drops its guest connection after a cold boot. It is to be judged again now the disk has room.
- inv3 `earlier_versions_of_a_deleted_file_leave_the_volume_when_it_goes_idle` returns a 500. It is newly failing on main after `f2bf39b`.
- `desktop_screens` fails at line 88.

These are failures in the code or the environment, not in the tests.

**Proof.** On 2026-11-13, both patches were applied in order inside the copied tree on Strix Beta, at main `fe18577`, and the whole invariant suite ran there. 121 of 123 tests pass, including every test the two patches change. Both inv3 failures above pass now that the disk has room.

Two tests fail, and neither patch touches them:
- `desktop_screens` fails at line 88.
- inv1 `a_computer_reaches_only_declared_destinations_even_as_root` fails with "Broken pipe" on the harness's command into the guest. It passed in the run before, so this is the same intermittent dropped guest connection after a cold boot.
