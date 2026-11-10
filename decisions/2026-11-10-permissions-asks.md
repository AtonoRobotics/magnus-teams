# Only the four asks of spec rev 140, from one place, with a card the server builds

Spec rev 140 ("Permissions and tasks") says only four things reach a person as an ask, and the person sees only a server-written card. The owner asked for a rebuild to it that deletes the old paths instead of layering on them. This records what changed, what was deleted and why.

**One module asks.** `crates/magnus/src/asks.rs` is the only code that makes an approval item (`raise`), and the only one that decides whether the person's own answer already covers an action (`person_cover`). It names the four asks as `asks::Reason`:

1. **Outside what was asked** (`outside_request`). Auto Review judges a boundary action. When it asks, or cannot answer, or allows without the owner's delegation, this is the ask. A send reviewer's flag is the same case. Enforced in `Kernel::settle`.
2. **A payment** (`payment`), every time. `payment_ask` in the boundary's policy stage. No rule answers it and Allow covers one action.
3. **Private data to a new destination after untrusted input** (`new_destination`), the Rule of Two floor, unless a rule covers it. Recipients the person named pass, and so does a destination the person allowed once before (`asks::remembered`). A web lookup's host is its destination. It is never Auto Review's call alone (invariant 4): the floor runs before Auto Review and Auto Review never sees it. Allow covers one action, and there is no Allow always.
4. **The person's own ask-first rule** (`ask_first`): `rule_effects::person_rules` and, for the person's own computer, `local::local_gate`.

A unit test (`asks_tests::no_ask_outside_the_four_can_be_produced`) maps every rule string the kernel asks with to one of the four, and fails if any other file in the kernel makes an approval item or names one of the deleted asks.

**Deleted.** Each of these asked the person something rev 140 does not:
- the contract-change request to the person (a frozen contract now refuses the change and tells the agent to finish against its criteria or fail);
- the per-task new-source ask of the computer's sources tool (it grants the host to the task);
- the audience ask (`kernel:audience`) for private data to a wider audience without untrusted input: Auto Review judges it as any other boundary action;
- the web-lookups-after-private-data ask (`kernel:web_lookups`): a lookup asks only as case 3, after untrusted input too;
- the local-change ask a policy Ask raised on the person's computer (`kernel:local_change`): a policy's Ask is no longer an ask, only its Deny stands;
- the unknown-outcome ask after a write that may have reached an app that cannot deduplicate. It is not sent again, the agent is told to check, and the person gets a notice on the task;
- the approval-for-an-hour ("window") and standing scopes, the workspace's named approvers, Auto Review's routing table (`route`, `clear_ask`, `review_permitted`), the old approval and reviewer stages of the boundary, and the unattended-expiry setting.

The Never-allowed denials, payments, the floor, taint and the reviewer's block are unchanged. Reads and everything inside the computer pass without review (`asks::is_boundary`).

**The card.** The inbox renders an approval with `ask`: one sentence the server wrote from the action's fields (`asks::sentence`), the destinations, the offered answers, the scope Allow gives, and the conversation it belongs in. The details keep the exact content (one tap away in the app) and lose the rule, the reason, the taint and Auto Review's or the reviewer's words (`asks::without_reasoning`). The agent never writes or sees it: the model is told only the outcome.
- **Allow** covers the rest of the task for the same kind of ask and the same destinations (`scope: task`); for a payment and case 3, one action (`scope: once`). The approval event gets an optional `destinations` field, defaulting to none, so earlier approvals read as before (an event kind is never edited).
- **Allow always** is offered on case 1 only, to the person whose work it is, when the action has a destination. It saves a narrow rule through `rules::compile`, accepted at once, which counts as the person's answer ahead of Auto Review next time. A rule never answers a payment or an ask-first rule.
- **Don't allow** stops that action. The agent is told to carry on with what does not depend on it (`asks::declined`).
- In a team thread only the asker can answer.

**Unwatched work.** On a schedule or in the background, an ask waits `UNWATCHED_ASK_WAIT_MINUTES` (30). Then a message stays a draft and the item is closed; anything else keeps waiting for the person while the run is told to carry on with what does not depend on it, and is told again if the person allows it later.

**What only developers see.** The rule, the reason, the taint and the reviewer's words are on `/v1/dev/runs/{id}` (each ask's `reasoning`, each action's decisions with reason and rule) and drawn by `dev.js`, not on the inbox, the task view's decisions or toasts. Scratch tests that read them now read them as a developer (`tests/scratch/src/dev_view.rs`).

**Invariants.** `inv4_taint::private_data_sent_to_a_wider_audience_asks` encodes the deleted audience ask. It passes today only because the scripted model cannot answer Auto Review. `decisions/2026-11-10-permissions-asks-invariants.patch` replaces it with the same post made after untrusted input, which must ask as case 3 with neither a task-wide Allow nor Allow always. It needs the owner's `git apply`. `inv3_event_log::crash_against_an_app_without_idempotency_asks_instead_of_resending` still passes unchanged: the person's notice is the open inbox item it waits for, and nothing is sent again.
