# A person deleted takes the approvals given to them (2026-11-06)

- Erasing a person removes every approval given for their actions (any scope) in the state reducer, so it holds live and after the log is replayed: approvals match by person ID, and a new account later bound to that ID is someone else (#191; test `a_person_deleted_takes_the_approvals_given_to_them_but_not_those_they_gave`).
- Approvals the deleted person gave to others stay: each was valid when given and is about the other person's work, and the gate already checks at use that a standing or window approval's approver may still give it, which after erasure reads that person ID's rights from the organisation's config alone (same test).
