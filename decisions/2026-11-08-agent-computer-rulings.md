# Agent computer: the owner's rulings before the build (2026-11-08)

The owner ruled on the conflicts found reviewing the Magnus Agent Computer spec, and the spec now carries them. The rulings on computer taint were withdrawn at spec revision 69: see `2026-11-08-agent-computer-declared-destinations.md`.

- Logged-in browsing runs in a separate browser VM per identity, with no agent shell, driven through a broker on the server. Reason: the agent has root in its computer, so a browser there could not keep a profile or a typed password from it. The computer's own browser holds no logins.
- Auto Review cannot release private data to a wider audience after untrusted input. That asks unless a rule or grant covers it, never on Auto Review's judgment alone. Reason: a reviewer that reads attacker-influenced content can be misled like the working model. Invariant 4 tests it.
- Logins live only in browser VMs, so untrusted reading never needs a separate computer. Web reading uses a throwaway browser: a browser VM with no profile, through the proxy. Inbound email is read through connectors, and its text reaches the agent as quoted data that taints the run. An attachment opens in the per-run sandbox with no network, user volume or profiles, and only what it extracts comes back, as quoted data, unless the user saves the file. Accounts that move money ask for every payment. Reason: it keeps the reader away from the logins without a second persistent computer per person.
- Test 29 covers a computer's screen and files only. The admin's data access under spec-decisions ruling 14 stands.
- The purge window is 30 days. Snapshots and backups are kept no longer. Reason: a snapshot is a block image, so a deleted file stays in older snapshots until they expire, and the GDPR gives a month to act on an erasure.
- Operator visibility: developers see the audit log and observability by default, and a screen or files only in a session the user grants. Until a mode blocks operator access technically, this is a rule the product enforces. Every granted session is logged where the user can see it, and alpha users are told.
- Also settled in the spec: an install or payment ask that expires leaves the task waiting with the ask; reads in connected folders pass without review; one credential service with a key per person; approvals on a workspace resource go to the approvers a workspace rule names.

What this changes here:

- A computer is a new, persistent thing beside the per-run sandbox, which stays as it is for runs and checks, with no network. `sandbox_has_no_network_device_and_reaches_nothing` keeps its meaning for those VMs.
- `docs/proposals/agent-workspace.md` (a disposable VM per session) and spec-decisions question 19 (a VM per run) are superseded for computers.
- The new invariant tests are drafted in `docs/proposals/agent-computer-invariants.md` and go into `tests/invariants/` only with the owner's approval, each with the build step that makes it pass.
