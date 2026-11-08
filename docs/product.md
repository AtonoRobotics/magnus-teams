# Product Spec: Team Workspace + Desktop Agent

## 1. What this is

A team workspace where people and agents work on the same projects,
without copy-pasting context between tools. One hosted workspace per
team. Every human uses the same simple desktop (and phone) app.
Agents of any vendor join through harnesses. A Magnus digital worker
is a staff participant in the workspace, not the platform.

This replaces the Magnus appliance and its enterprise shared-context
ambition. Shared context as an ambient organisational brain is out of
scope (see principles P2).

## 2. Users

- **Individual**: wants an autonomous worker, minimal setup, Apple-simple.
- **Team (2–50)**: wants people plus several different agents on the
  same work, with no human acting as the integration layer.
- **Non-technical first**: no user should need to know Git, models,
  prompts, policy languages, or agent internals.

## 3. Top-level shape

```
┌─────────────────────────────────────────────┐
│ Team workspace (hosted, always-on server)   │
│  Rooms · tasks · artifacts · record         │
│  Magnus worker · external agents (harness)  │
└───────▲──────────────────────▲──────────────┘
        │                      │
  Desktop app (per human)  Agent computer(s)
  talk/approve/watch       persistent, watchable,
                           take-overable
```

- The worker and coordination live on the server. The app is thin:
  closing it changes nothing about running work.
- Each person also has a **private space**: a direct conversation and
  drafts with any agent. Content leaves only by **Promote** (§5).

## 4. The desktop app

### 4.1 Concepts the user learns (complete list)

1. **Room** — a project or topic. Talk, plan, and work in one place.
2. **People and agents** — named participants in a room, shown the same way.
3. **Work** — items in a room with an owner and a state: proposed,
   in progress, waiting on you, done (with evidence).
4. **Approvals** — cards in plain words: what, to whom, using what.
   Buttons: Allow, Edit, Stop.
5. **The computer** — a button: "See the screen." Watch any active
   agent computer, or take over (§6).

There is no other top-level navigation in v1. Settings holds people,
connections, and autonomy levels. Nothing else is user-facing.

### 4.2 Setup

Sign in, create or join a workspace, invite people by link. Under
5 minutes, no server, no keys. Connect an app (Gmail, Slack, GitHub…)
only when a task first needs it (just-in-time, scoped, revocable).

### 4.3 Autonomy in the UI (implements P7)

Per room or per goal, one control: Off / Suggest / Draft / Act for me.

- **Suggest**: agents propose plans and findings. Nothing executes.
- **Draft**: agents prepare artifacts and messages. A human sends.
- **Act for me**: within granted scope, agents execute and report.
  Money, external sends, and deletions always show a card first.

### 4.4 What the user never sees

Policy, taint, boundary, model names as a primary choice, prompts,
tool schemas, leases, queues, tokens. Errors say what to do next in
plain words, e.g. "The computer is asleep — tap to wake it."

## 5. Team workspace behaviour

- Conversation is the interface. Coordination is what conversation
  writes to the record (architecture §3): a claim, a proposal, a
  handoff, each visible in the room as it happens.
- **Plan before work** is shown, not enforced by nagging: an agent's
  two-line plan appears on the work item; others may object or rival
  it before execution spends anything.
- **Timeline** answers, without reading chat: who did what, on which
  item, when, checked by whom. This is the product's home screen value.
- **Private → shared** is one action: Promote. It copies the artifact
  plus its provenance. The private source stays private.
- **Multiple workspaces**: a person may belong to many. Participants
  and artifacts cross workspaces only by explicit join / promote.

## 6. The agent computer ("let the user use it")

Each workspace may run one or more persistent agent computers.

- A full desktop with browser and files. Agents work there visibly.
- **Watch**: any room member with permission can open the screen live.
- **Take over**: the human takes control; all agents on that computer
  pause instantly; every input is attributed to its actor. Release
  resumes agents from the state the human left.
- **Persistent**: files, logins, and installed tools survive between
  tasks and sleeps. It wakes on work, sleeps when idle.
- Untrusted code execution never happens here. It runs in a disposable
  sandbox beside it (salvaged Magnus sandbox, architecture §8).

## 7. The Magnus digital worker

A named staff participant, present in rooms it is assigned to.

- **Role**: defined per workspace in plain words ("watch releases and
  fix red builds"), with goals it owns for days to weeks.
- **Owns outcomes**: notices work (builds, inboxes, schedules, stale
  claims), proposes plans, claims or assigns tasks, chases handoffs,
  verifies other participants' finished work, and reports with evidence.
- **Accountable record**: its work items show goal, authority used,
  actions, evidence, and cost. A person reviews the worker periodically
  like staff: continue, adjust scope, or pause.
- **Not the planner**: it proposes like anyone (P3). It keeps the
  record honest; it does not direct other participants' work except
  tasks explicitly assigned to it.
- It coordinates external agents through the same five messages and
  the same approvals as everyone (architecture §3). No back door.

## 8. External agents

Claude, Codex, Grok, Muse, Muse, Grok Bot, and successors
join via harnesses. To the room they are participants with a name,
an owner (the person who connected them), permissions, and a computer
or sandbox. Vendor neutrality is the point: no agent, including the
Magnus worker, has a privileged protocol.

## 9. Non-goals (v1)

Marketplace of agents or connectors; organisation-wide memory or
search across teams; prescribed workflow library; per-user desktop
engines; replacing Git hosting (workspaces integrate with it);
regulated-industry compliance packs.
