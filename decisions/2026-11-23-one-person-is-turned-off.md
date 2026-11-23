# One "person is turned off"

**Choice.**
- Turning someone off is one operation, `Kernel::turn_off_account` (`account_change.rs`), which the Admin section's PATCH with `status: "disabled"` and POST /disable both call. It sets the account's status (their sessions end on the record) and then `Kernel::turn_off` (now in `turned_off.rs`), which holds their new work, ends any session left, revokes their paired devices and pauses their schedules. The identity provider's turn-off goes through the same `turn_off`.
- `turn_off` revokes every live device certificate of the person: dead in the registry at once, so the device gate refuses it, then deleted at Cloudflare when this server pairs devices and its token can be read (a failure there is logged; the origin still refuses). Erasing a person revokes their devices too.
- The command line writes the account store alone. The kernel's five-minute watch (`hold_disabled_accounts`) turns off, the same way, anyone recorded as off whose work is not held.
- Signing in refuses an account recorded as off before anything is recorded: no subject bound, no rights from the provider, no session, no `SignedIn` (which is what clears the hold). This holds for the password, the provider and the invitation paths, which all end in `issue`; the provider's path checks before it records anything.
- Turning someone back on sets their status; the hold clears when they next sign in, as before, and their paused schedules stay paused until they resume them. No new event kind is needed.
- The hold's words no longer say "at the organisation's sign-in", since an admin here turns people off too.

**Reason.** "Off" was kept in two places that drifted: the PATCH wrote only the status, so schedules, routines, proactive reviews and mailbox watches kept running and spending as the person; a provider sign-in cleared the hold for someone turned off here; and a removed employee's paired laptop still passed the device gate. One way to turn off, and a sign-in that refuses an off account first, keep the two places in step.
