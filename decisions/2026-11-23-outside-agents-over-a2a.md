# Outside agents speak A2A 1.0, both ways

**Choice.** An outside agent (a customer's, a supplier's, a vendor
platform's, a hosted coding agent) is reached over A2A 1.0, the released
version (a2a-protocol.org/latest/specification), JSON-RPC binding only, with
`A2A-Version: 1.0` on every call. The methods used are `SendMessage`,
`GetTask` and `CancelTask`. The server answers those three and refuses the
rest with A2A's `UnsupportedOperation` (-32004): streaming, subscriptions,
push notifications, `ListTasks` and the extended card. It lives in
`crates/magnus/src/a2a/`. The retired `peer_agent.rs` stays retired.
Nothing of an outside agent runs on the server.

*Connecting.* A member gives the agent card's URL in Apps, beside their
apps. The card is fetched through the egress client. It must carry a JWS
signature (RFC 7515 over the RFC 8785 canonical card with `signatures` and
default values removed, as A2A 1.0 says) that verifies against a key in the
key set its `jku` names. That key set must be on the card's own host. EdDSA,
ES256 and RS256 are accepted. An unsigned card is refused. The card's
security requirements choose the scheme: an API key in a header, HTTP bearer,
OAuth 2 client credentials, or mutual TLS. OpenID Connect and a key in a
query or cookie are refused, because Magnus cannot hold them the way it
holds an app's sign-in. The credential goes to the vault
(`outside-agent/<id>/credential`, the member's own under `secret_scope`). It
is read and attached only when a call is made (`a2a/client.rs`), as an
app's sign-in is at the boundary's credential stage. The agent is a Cedar
principal (`OutsideAgent::"<id>"`, `policy/outside.rs`) that nothing admits.
The member then admits it for themselves, or, as a Team Admin, for one
shared project, where it is a guest. Each admission is one `permit` policy.

*Outbound.* `outside_agent_send` is a boundary action: Irreversible,
untrusted label, public audience. Auto Review judges it (`is_boundary`,
`is_reviewed_send`). Its destination is the host of the endpoint the card
declared at connect (`destinations_of`), and the call goes to that endpoint
and nowhere else. A send of private data after untrusted input asks the
person unless a rule covers it (invariant 4). It is never staged, because no
draft of it can be held at the agent. Replies, artifacts and status come
back under the untrusted label, so they taint the run and reach the model
through the one output path as quoted data. A2A 1.0 does not let a client
choose a new task's id, so Magnus records the task (`TaskOpened`, with
message id `magnus-<idempotency key>`) before the send, and the agent's
task id (`TaskStarted`) as soon as it answers. When a crash leaves the
intent without a result, `reconcile` resumes with `GetTask` on the recorded
id and never sends again. With no id recorded, the outcome is unknown and
the person decides. With no task recorded, nothing left, and it is sent.
The agent's report is not evidence. A finished send reads back what it
changed with one of the person's own read tools named in `read_back`, as a
`ReadBack` record. Without one, the result says it was not read back. A run
that stops or fails cancels its open outside tasks (`CancelTask`, from
`end_run`). Disconnecting cancels them all.

*Inbound.* The server publishes one card (`/.well-known/agent-card.json`),
signed with an Ed25519 key that rotates every `CARD_KEY_ROTATION_DAYS` (30).
The current and previous keys are published at `jku`
(`/.well-known/agent-card-keys.json`). The card lists only the skills some
admission exposes: asking a person, asking a shared project. It is published
only once `[appliance] public_url` is set. A request names a person or a
shared project in its message metadata (`{"to": {"person": ..}}`). It
authenticates with the API key the card declares (`x-magnus-agent-key`): a
per-agent key issued once at connect, kept in the vault and compared by
digest. Its Cedar principal must admit the target, or the request is
refused with nothing created. The task id is a digest of the agent and its
message id, so a replay returns the task already made and two agents never
share one. Each agent may send `MAX_REQUESTS_PER_AGENT_PER_HOUR` (30)
requests. A request to a person is an inbox card with guest standing.
Taking it up starts the person's own run, labelled untrusted, with the
agent's words quoted as the objective. A request to a shared project is a
message signed `outside_agent:<id>`, which no agent answers. The members
read it, and `GetTask` returns what they post in its thread: what is shared
with the guest. The reply is a send through the gate. When the run
finishes, the reply waits in the person's Airlock (an `outside_reply` inbox
card) unless their standing release covers that agent and that exact
request. Then it still passes the checks every automatic release passes
(`Kernel::checked_send`, the door `release_automatically` uses): the
scrubber, the run's taint, the outgoing classifier and Auto Review. A reply
the person releases by hand is their own act, as a lane's is.

**Reason.** The owner's collaboration design asks for outside agents over
A2A in both directions, with every sentence above as a requirement. A2A 1.0
is the released version, and JSON-RPC is the binding every A2A agent must
offer. Each choice that is not spelled out follows the rule that settles it
nearest. A send is a boundary action, so it gets the gate, Auto Review and
invariant 4 like any other. Invariant 3 forbids a resend whose outcome is
unknown, and A2A's server-made task ids leave recording before the send and
reading after as the only way to keep it. Invariant 7 makes the agent's
"done" a claim to check. An outside agent's words are someone else's words,
so they are untrusted wherever they land, and a guest's standing is the
lowest there is.
