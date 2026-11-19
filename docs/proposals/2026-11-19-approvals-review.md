# Approvals and security review: protect without ceremony

The owner's standard (2026-11-19): protect the person, their data and
Magnus as autonomously as possible, with as little asked of the person as
possible, without giving up safety that is useful and necessary. An ask is
worth a person's attention only when they know something the system does
not: whether this is what they meant, or whether this money or this
irreversible act is wanted. An ask that the person cannot judge better than
the system, or that fires because a check is down, is ceremony.

This reviews every place that asks the person or stops work (inventory:
`crates/magnus/src/boundary.rs` pipeline, `asks.rs`, `auto_review.rs`,
`safety.rs`, `desktop.rs`, `consent.rs`, `tool_gate.rs`, `payments/`,
`computer/egress/`) and sorts it into keep, fix and remove.

## The model the owner set

| What | The person sees |
|---|---|
| Work that serves what they asked | nothing: asking for the task is the approval |
| High consequence, irreversible, beyond the request | Allow / Allow for this task |
| High consequence, reversible, beyond the request | Allow / Allow for this task / Allow always |
| A payment | Allow, each time |

Done in `decisions/2026-11-19-allow-covers-the-task.md`: Allow for this task
covers every tool of the task, not one; Auto Review no longer asks only
because an act is hard to undo; the card offers the three answers; Allow
always works for reversible actions that name no recipient.

## Keep: these protect and cost little

- **Payments ask every time.** Money is the clearest case a person must own.
- **Hard denials** (disclosing a credential, reaching someone else's
  account, blind resend of an unknown outcome): denied, never asked.
- **Kill switch, prohibited tools, identity-provider groups, Cedar denials,
  the person's own block and ask-first rules.** No ask; they decide.
- **Inbound injection screen, login-code stripping, read-back after
  irreversible acts, idempotency, leases.** Silent; they protect without
  involving anyone.
- **The Rule of Two for sends to a recipient** (case 3 with a named
  address). Exfiltration after reading outside text is the main real
  threat to an agent, and the exemptions (named by the person, mailed
  before, in the directory, remembered) already keep it rare.
- **Tool-definition pinning and screening.** Stops a changed tool from
  running unscreened.

## Fix: real protection, delivered as ceremony

1. **An outage becomes an ask.** Auto Review or the outgoing classifier
   unreachable, a review document over 256 KB (`auto_review.rs` 208-219),
   or the safety monitor unreachable (`safety.rs` 181), each put a card in
   front of the person. They have no more to judge than the system had, so
   they rubber-stamp it. **Proposed:** an outage is not a decision. The run
   waits and retries with backoff, with a notice ("Safety review is
   unavailable; this resumes on its own"). It is still fail-closed and asks
   no one. An action too large to review is reviewed in parts, or denied
   with its reason to the agent, which can split it.
2. **The outgoing classifier's outage alone forces an ask.** Its finding is
   only advice to Auto Review. **Proposed:** Auto Review runs without it,
   told the finding is missing.
3. **The send reviewer's flag is an approval card**, though the module calls
   itself "quality, never a security control" and fails open
   (`run/reviewer.rs`). **Proposed:** its flag goes to Auto Review as input,
   not to the person as a card. Its block stays.
4. **Desktop program tools all ask, reads included** (`desktop.rs` 55-91:
   nothing is classified, so every Blender or FreeCAD tool is irreversible
   and ask-first, and Allow always is never offered). **Proposed:**
   classify each program's read tools as reads, and offer Allow always for
   a program on the person's own computer ("Allow Blender always"). Running
   code in it stays an ask the first time.
5. **Case 3 on a write that names no recipient** (a commit or comment after
   reading an issue). It protects against something real: injected text
   steering private data into a public place. But the card names no
   destination, so the person cannot judge it, and it can never be allowed
   always. **Proposed (universal, needs an invariant 4 change):** ask only
   when the outgoing content carries private data, as the outgoing
   classifier finds, rather than whenever the run is tainted. A commit of
   the code the person asked for passes; a commit carrying their mail does
   not. Within a task, the new Allow for this task already stops repeats.
6. **A changed tool definition with no screening model is held until the
   app is reconnected** (`tool_gate.rs` 229-231). Reconnecting accepts the
   change without anyone seeing it: ceremony. **Proposed:** a card showing
   what changed in the definition, with Accept or Disconnect; or screen it
   with the default model.
7. **A Link purchase can ask up to three times** (Auto Review on
   `purchase_request`, again on `purchase_fill`, and Link's own approval).
   **Proposed:** Link's approval is the payment ask; the fill of an approved
   request is covered by it.

## Remove or reduce: cost without matching protection

8. **Live tool listing before every connector call** (`check_pin`): a
   network round trip per action, and a denial if the listing fails. The
   pin protects against a definition changed between listing and use;
   **proposed:** re-list at most once a minute per app, and on a pin
   mismatch. No person sees this, but it slows every task.
9. **Credential egress review keyed by path** (`auto_review.rs` 168): every
   new path is a model call, and an outage denies. **Proposed:** key by host
   and method.
10. **`delegate = false`** makes every Auto Review allow a person card. It is
    the owner's switch and defaults to on; it should say in the admin page
    that it doubles every review.

## Make what already exists visible

- **Grants** ("Handle things like this") are the person's standing
  authority for a category of work, and the right answer to "stop asking me
  about this kind of task". They are reachable only from a proposal card.
  **Proposed:** offer "Handle things like this" on any approval card where
  Allow always is not offered, so an irreversible kind of work the person
  trusts can be granted within a budget.
- **What Magnus may do on its own** (Settings) should list Allow always
  rules beside grants, with one-tap removal, so allowing always is easy to
  take back and the person trusts it.

## What needs the owner

- Items 1, 2 and 5 change what invariant 4's tests expect (an outage and a
  tainted unnamed write ask today). They need a `git apply` patch to
  `tests/invariants/` from the owner.
- Item 5 is a judgement on how much the Rule of Two should rely on the
  outgoing classifier. The recommendation is yes, with Auto Review behind it.
- The rest follow from the owner's standard and can be done as decisions.
