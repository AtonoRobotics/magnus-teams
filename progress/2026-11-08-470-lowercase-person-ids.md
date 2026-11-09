# #470 lowercase person ids (2026-11-08)

- Added `key()` canonicalization at all 23 person/counter-taking `Accounts` methods plus the JSON import (keys and person fields).
- Verified live box needs no migration: `account_*` tables not yet deployed (only sessions.json with `samuel` exists).
- Tests: new canonical-lifecycle unit test fails before/passes after; import fixture extended; lib 145/145, clippy clean, local_people 1/1, backup 4/4.
