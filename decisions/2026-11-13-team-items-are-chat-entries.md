# A team channel's items speak the personal chat's entry language

**Choice.** The owner, on the server: "They should be based on the same systems unless teams requires a completely different backend. Then they should have the same UI elements." As a first step on the server, a team channel's items (`TeamItem`, `crates/magnus/src/team/items.rs`) now say what the personal chat's entries say:

- `kind`: the chat view's kind, `message` (a person's) or `answer` (the agent's words), as in `/v1/chat/{thread}`, plus `work` (the agent's work with no words yet) and `deleted` (where a deleted message with replies stood). The app no longer guesses it from the sender and the words.
- `files`: the files a message carries, by name. The app no longer reads them out of the words.
- the work row's `status` is the task's own status (`running`, `suspended`, `completed`, `failed`, `cancelled`), the words the personal chat's task rows use, not the team's own `working`, `waiting`, `done`, `stopped`. One renderer reads both.

Nothing stored changed: no event kind was added, edited or retired, and old logs replay to the same state. The fields are computed when the view is read.

**Reason.** One chat view (decisions/2026-11-13-one-chat-view.md) should read one shape. These were the places the app translated a team item into a chat entry by itself; the server now sends them in the chat's terms.

## Where the server still has two models, and what remains

The personal chat and a team channel are still stored and served by two systems:

1. **Storage.** A personal conversation is its tasks' `task_created` events with a `chat` thread, and its entries (`message`, `answer`, `turn`, `tasks`, `result`, cards) are derived from runs in `chat.rs`. A team channel is the workspace ledger's `Chat` events (messages, edits, pins, reactions, unsent) and its requests' runs, read through the member view (`team/member_view.rs`).
2. **Routes.** `/v1/chat`, `/v1/chat/{thread}`, `/v1/chat/{thread}/entries/{id}/{act}` against `/v1/workspaces/{ws}/channels/{c}/messages`, `/view`, `/messages/{id}/{act}`.
3. **Entry shape.** The personal entries are built as untyped JSON in `chat.rs`; the team's are the typed `TeamItem`. The ids, the requester and the multi-user fields (reactions, replies, pins, edited) exist only on the team's.

What teams genuinely needs of its own, and should keep whatever is unified:
- **Membership and channel visibility**: a channel is read only by its people (the member view); a personal thread has one reader.
- **A private answer shown only to its requester**: an answer that holds data only the requester may see stays theirs until they post it.
- **inv8**: team words never count as the person's own context or taint.
- **Per-workspace isolation**: nothing crosses between workspaces.

Recommended next steps, in order, each small enough to land alone:
1. Give the personal chat's entries a typed struct in Rust, and make `TeamItem` the same struct with the multi-user fields optional, so the wire type is defined once for both.
2. Serve a team request's work as the personal chat's `tasks` entry (`task_id`, `title`, `text`, `status`, `status_line`) instead of `work`, and its answer with `task_id`.
3. Route a team channel's message actions through the same handler as the personal chat's (`entries/{id}/{act}`), with the member view deciding who may act, so Share, Save, Export, Retry and Delete are one code.
4. Only then consider storing a channel's messages as the same conversation events as a personal thread, with a reader for the team's existing `Chat` events so old logs replay unchanged.

Done on 2026-11-13: steps 1 and 2 (2026-11-13-one-chat-entry.md), and step 3 as one rule rather than one route (2026-11-13-one-message-actions.md). Step 4 is not done; why, and what it would take, is in 2026-11-13-one-chat-store-not-yet.md.

Step 4 is a change of what is stored, and steps 1 to 3 are what the app needs for one experience; they were not started here because each touches the routes that the invariants hold to (inv8, team privacy) and needs its own review and acceptance run.
