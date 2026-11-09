# #529 delete stays retryable (2026-11-08)

`admin_delete` now disables (guarded) first, erases, then removes the
local directory entry: a failed erasure leaves a listed, disabled
account whose deletion retries, instead of a vanished person with
orphaned content. Response shape unchanged.

Covered by `deleting_a_local_person_erases_then_removes_them` (new):
invite, accept, sign in, schedule, delete — gone from the list, counts
(1 schedule, 1 session), nothing signs in, `user_deleted` audited. The
first local-person delete exercised end to end; the config delete and
the last-admin refusal still pass unchanged. The mid-erase failure
itself has no injection hook, so the ordering is by construction.

Verified: `admin_users` 6/6, `admin_people` 1/1, lib 150/150. Every
suite runs in CI.
