# Evidence from the record, not task contracts (spec rev 147)

The owner directed rev 147: evidence comes from the record, not from ceremony.

Removed:
- `task_contract` and the rule that one comes before any action that changes
  something. A call to it from a run that still has it answers that contracts
  are retired (`run::CONTRACT_RETIRED`), as a result, not an error.
- Contract freezing, contract-change approvals, criteria checks with JSON
  pointers, and the verifier's power to reject completion (`run/verify.rs`,
  the reviewer's completion pass). The `contract`, `contract_frozen`,
  `work_started` and `verification` events keep their readers: old logs
  still replay.
- `propose_grant`. Allow always and connect-time consent replace it. Grants
  already set keep working and can be revoked; no tool lets the agent put one
  to the person.

Built:
- Receipts: `evidence::receipts` lists what the model did that changes
  something and went through, from the boundary log, with where it went.
  Refused, failed and unknown actions are the result's "not done".
- Read-back: after each irreversible or payment action of the model's, the
  server reads the world back with the read the connector declares in
  `[read_back.<tool>]` (read tool, input pointers, expected fields), as its
  own issuer (`Issuer::ReadBack`), which the boundary lets only read. What it
  found is an `evidence` event beside the result. The agent is told whether
  it matched and which field differed, never the content. A connector with no
  read-back declared gets a "Not read back" line, so the gap shows. Gmail's
  `send_draft` reads back with `get_message`; Outlook's send returns no id, so
  it has none yet.
- Sources: `task_complete` takes `sources`, each a claim and the call ID it
  came from; a call that is not this run's or returned an error is refused,
  so the agent can fix it.
- The task card: a status line ("<plan step> · step n of N", else what it is
  doing now) and Stop; when the task ends, the result in the conversation
  shows the receipts with their read-backs, the sources and what was not done.
- `task_plan` stays, optional, progress only: `done_when` is no longer required.

Why read-back on the result rather than refusing completion: the spec removes
the verifier's power to reject completion; a mismatch is caught where it
happens, told to the agent at once, and shown as not done on the result.

Invariants 6 and 7 script `task_contract` criteria and `propose_grant`; the
patch `2026-11-12-invariants-evidence-not-contracts.patch` retargets them for
the owner to apply. The acceptance scenario (a sent email ends with a receipt
and a read-back, no contract written) lives in invariant 7 and runs as the
`evidence` acceptance suite.

Follow-on choices:
- A task's title, once from its contract, comes from `task_plan`'s optional
  `title` (a contract recorded before rev 147 still names its task).
- The verifier module is gone from the module registry; the reviewer judges
  sends only.
- A sub-agent's tool list that names a retired tool (`task_contract`,
  `propose_grant`) drops it instead of refusing the start.
- The harness text no longer describes executable checks; it says the
  sandbox is where the agent checks its own work.
- The scratch suites `contract_freeze` and `standing_authority` tested only
  what is retired and are removed; tests of criteria review and the verifier
  in other suites are removed, and the autonomy retry scenario now gets its
  authority from the organisation's policy instead of a grant.

Applied: the invariants patch landed in `0438887` (2026-11-12). Its file was removed on 2026-11-13; its content was checked to be in the tree.
