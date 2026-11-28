# Gates beyond the invariants come out

The owner, 2026-11-28: "magnus project is bureaucratic security theater.
It's impossible to do anything without jumping through many hoops."

Every gate a person meets was mapped and checked against the eight
invariants. The owner chose four that no invariant asks for to come out, and
kept the injection guard (invariant 4) and the per-task and wake budgets.

## Choice

1. **Mail keeps its login codes.** One-time codes, reset links and sign-in
   links reach the agent, so it can finish a sign-in or sign-up the person
   asked for. Only an app that sets `filter_login_codes` has them taken out;
   the catalog's email apps no longer set it, and a mail app is no longer
   found by its name. Replaces `2026-11-23-login-codes-out-of-all-mail.md`.
   Invariant 2 is about stored credentials (never in the model's context, the
   sandbox or the log), which this does not touch; its tests set the filter
   explicitly and still pass.
2. **A routine asks its author instead of refusing.** A routine's lane in its
   author's agent puts an ask in front of the author and waits as unwatched
   work does (`unattended`: 30 minutes, then a message stays a draft and the
   run carries on with what does not depend on it). Before, what would ask
   was refused outright. A team agent's own routines from older releases
   have no person and still ask no one. Replaces
   `2026-11-23-routine-lanes-ask-no-one.md`. Invariant test 40
   (`inv8_team.rs`) pinned the old behaviour. The owner had it updated
   (2026-11-28): the routine's ask reaches its author alone, the action waits
   for their answer, and the routine then reports to the channel.
3. **Allow always on every card but a payment.** It no longer needs the card
   to name destinations, and it is offered on the default policy's ask before
   a program runs code on the person's own computer (`desktop::ASK_RULE`).
   An allow rule that names no destination covers only actions that reach no
   destination (`rules::allow_covers`), so it never widens to a recipient and
   never answers case 3. Still without Allow always: a payment, the person's
   own ask-first rule, and their computer set to ask before every action.
4. **A granted site stays granted.** `add_computer_destination`, once Auto
   Review lets it through, records `DestinationAdded` on the person's
   computer instead of `DestinationGranted` to one task. The site is
   reachable from then on with every method, so a form there can be sent and
   the next task does not ask again. `DestinationGranted` stays a readable
   event kind for what older releases recorded. Pages the agent only opened
   stay read-only, and non-web ports stay closed.

## Why

Each of these made the person answer, or the agent fail, with no invariant
behind it: the agent could not sign in anywhere, routines silently dropped
their work, the same app action asked in every task, and the computer forgot
every site at the end of each task. What the invariants require stays: one
boundary that fails closed, credentials out of reach, untrusted content
granting no authority, payments asking every time.
