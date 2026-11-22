# Agentic security without approval fatigue

A clean-slate design, written without regard to how Magnus works today. The
goal is to remove the person from the loop wherever that can be done safely,
and keep them in it only where their judgement is necessary. The last
section maps it to the current code.

## 1. The standard

Protect the person, their data, their money and Magnus as autonomously as
possible. An approval is kept only when all three hold:

1. the action cannot be undone, or its harm lands before it can be;
2. the person's request does not already say it should happen; and
3. no fixed rule can tell, from what is known, that it is safe.

Everything else runs. Safety comes from what the system can enforce by
construction (isolation, capabilities, data checks, undo), not from asking
the person to read a card they cannot judge better than the system can.

## 2. What it defends against

| Threat | Example | Primary defence |
|---|---|---|
| Prompt injection | A page tells the agent to mail the CFO's numbers out | Untrusted text cannot grant authority; data checks at egress |
| Model error or overreach | The agent emails the wrong person, deletes the wrong file | Scope from the request; undo; monitor |
| Compromised runtime | Code in the agent's computer turns hostile | No secrets inside; one egress gate outside |
| Credential theft | A token in a log or a prompt | The agent never holds credentials |
| Exfiltration | Private data in a URL, a post, a file upload | Value-level data check on every request |
| Irreversible harm | Money sent, data destroyed, access granted | The short list of necessary approvals |
| Persistence | An injected "rule" saved to memory | Memory keeps its source; untrusted memory never instructs |

## 3. Architecture: six parts

### 3.1 The Gate (one way out)

Every effect on the world goes through one gate outside the agent: connector
calls, web and browser requests, mail, payments, and all network traffic from
the agent's computers. The gate holds the credentials and injects them into
approved requests; the agent sees stand-ins and never holds a secret. The
gate decides by fixed rules, logs everything, and has the kill switch.
Nothing else can reach the world.

### 3.2 Scope from the request (the request is the approval)

When the person asks for something, the gate compiles a **task scope** from
their own words (never from tool results): the apps and kinds of action the
request implies, the people, sites, repositories and channels it names, a
spend limit (zero unless stated), and how long the task may run. A model may
propose the scope, but the kernel keeps only what literally appears in the
person's words or follows from their standing rules.

Anything inside the scope runs without asking. That is what "asking for a
task is the approval for what serves it" means in practice. The person's
standing rules ("Allow always") widen every task's scope; "Allow for this
task" widens one.

### 3.3 Data labels and value-level checks (instead of run-wide taint)

Every piece of data carries two labels: where it came from (the person, the
person's private apps, an outside source) and who may see it. The gate does
not ask "has this run ever read something untrusted?" It asks of each
request: **does this request carry private data to someone who may not see
it?**

- Private values (figures, codes, ids, addresses, and on request whole
  documents by fingerprint) that the conversation has read are checked in
  every outgoing request, in plain, encoded and transformed forms.
- A request that carries none goes anywhere, with no card.
- A request that carries some goes only to a destination allowed to see it
  (the person, their own accounts, people they named, people the data
  already came from); otherwise it is held as a draft, never sent.

Outside content is still marked, but the mark limits only one thing: outside
text can never add to the scope. It cannot name a new recipient, raise a
spend limit or create a rule, however it is worded.

### 3.4 Reversibility first

What can be undone does not need approving; it needs undoing. So the system
makes as much as it can reversible:

- outgoing mail and posts to people outside the scope become **drafts**;
  mail inside it is sent with a short undo window;
- deletions go to a trash with a retention period;
- file changes are versioned, and each computer is snapshotted before a task;
- record edits keep the previous value.

The person gets a **digest** per task: what was done, what was sent and to
whom, what was held, each with Undo. Review happens afterwards and in bulk,
not as interrupts.

### 3.5 Monitors that can stop, never release

- **Intent check (a model):** does this action serve the request? It may
  stop an action or raise a question, but it can never release something a
  fixed rule held, because a reviewer that reads attacker text can be misled
  like the agent.
- **Inbound screen:** content that tries to give the agent instructions is
  withheld from it, with a note.
- **Budgets:** per task, limits on spend, on sends, on outgoing data volume
  and on requests to new hosts. Hitting one pauses the task with a notice;
  it is not an approval prompt.
- **Outage:** if a monitor cannot answer, the action waits and retries; the
  person is not asked to stand in for it.

### 3.6 Isolation

- Research and browsing run, wherever possible, in a worker that holds no
  private data, so it can browse with no limit at all.
- Code runs in a disposable computer with no credentials, whose traffic
  leaves only through the gate.
- Logged-in browsing runs in a separate browser per identity, which the
  agent drives but cannot read the cookies of.
- Memory records the source of everything written to it; what came from
  outside is shown to the agent as data, never as an instruction.

## 4. The approvals that remain

These are the only approvals, and each is asked once, as a system card in the
app (never in the chat), with Allow / Allow for this task / Allow always
where the action allows it:

1. **Money leaves.** A payment, purchase or subscription, unless a standing
   rule covers it (a payee and a limit). Asked every time it is not covered.
2. **Private data goes to someone new, irreversibly.** Sending or publishing
   private data to a person or place the request did not name and that does
   not already have it, when it cannot be held as a draft.
3. **Something is destroyed for good.** A deletion with no trash or backup.
4. **Who has access changes.** Sharing, permissions, new credentials or app
   connections, new standing rules, a new network destination for the
   computer.
5. **Acting for the person in a binding way.** Signing, accepting terms,
   committing them to something on their behalf.
6. **The intent check cannot tell.** Rare by design; the card says what is
   unclear.

If the request itself names the action specifically ("pay invoice 1042 to
Acme, €500", "send this to jane@acme.com"), items 1, 2 and 5 are already
approved by the request, within its stated limits.

Never asked: reading anything, searching, browsing, running code in the
computer, writing inside the scope, anything reversible, anything held as a
draft, an outage, a refusal the agent can work around.

## 5. Decision table at the gate

| The request | Decision |
|---|---|
| Read (any app, any site, any search) carrying no private value | Allow |
| Read whose request carries a private value to a party not allowed it | Refuse; the agent is told to leave the data out |
| Write inside the task scope | Allow (mail with an undo window) |
| Write outside the scope, reversible or stageable | Hold as a draft; it appears in the digest |
| Write outside the scope, neither reversible nor stageable | Approval 2 or 3 |
| Payment | Allow if the request or a standing rule covers it; otherwise approval 1 |
| Permission, credential, rule or destination change | Allow if the request names it; otherwise approval 4 |
| Any action, while a budget is exceeded | Pause the task with a notice |
| Any action the intent check flags | Stop, or approval 6 |
| Computer traffic | Same table: reads allowed unless carrying private values; writes by scope |

Each row has its own test, and the prompt-injection suite (an attacker model
that obeys every instruction it reads) must find no breach on every change.
A change to a row needs a recorded decision naming it.

## 6. What the person sees

- Requests start working at once; nothing waits unless one of the six
  approvals is needed.
- A digest per task with Undo, and a drafts tray with Send, Send to this
  person from now on, and Discard.
- A sent log: every request that left, where it went and which rule allowed
  it.
- One kill switch.

## 7. Residual risk, stated plainly

- An injected agent could leak private data in a form the value check does
  not recognise (spelled out in words, split across many requests). The
  inbound screen, the intent check, the egress budgets and the injection
  suite reduce it; they do not make it zero.
- Undo has a window, and what someone has already read cannot be unread;
  that is why mail outside the scope is drafted, not sent with undo.
- The scope comes from the person's words, so what they paste counts as
  theirs.

## 8. Mapping to Magnus today

| Part | Exists | Missing |
|---|---|---|
| Gate | Boundary pipeline; computer egress proxy; stand-in credentials for the computer; kill switch | One module for all routes; a sent log for the kernel's side |
| Scope from the request | Envelopes (named destinations); Allow for this task and Allow always | Spend limit and verbs in the scope; "the request names it" for payments and permissions |
| Value-level checks | `private_values.rs` for web reads (2026-11-22) | Apply to writes and computer traffic; document fingerprints |
| Reversibility | Drafts (staging); scheduled-send style undo is absent | Undo window for in-scope mail; trash; per-task snapshots; the digest with Undo |
| Monitors | Auto Review, inbound screen, outage retries, query budgets | Egress budgets per task; outage waits everywhere |
| Isolation | OpenWeb worker, computers, browser VMs, labelled memory | Route web reading to the worker by default |

The work that removes the most approvals is: value checks on writes (so the
Rule of Two stops asking), the digest with Undo (so reversible actions stop
asking), and payments and permissions covered by the request's own words.
