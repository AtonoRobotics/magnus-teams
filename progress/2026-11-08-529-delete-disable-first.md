# #529 delete disables first (2026-11-08)

- Reordered `admin_delete` to disable → erase → remove-person, with the retryability argument in comments; fixed one stale comment.
- Added `deleting_a_local_person_erases_then_removes_them` e2e (`admin_users.rs`).
- Verified by CI (user preference: GitHub CI over local runs); local `cargo check` only.
- Unblocks: nothing behind it — next is the remaining #429 deletion (kernel provider + harness + cutover).
