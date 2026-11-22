# Magnus's ideas go to Explore, and open no conversation

## Choice

The first time someone signed in with nothing here yet, and the first time
they connected an app, Magnus opened a conversation of its own ("What I
could take over in Gmail") with two or three suggestion cards. The owner
saw these conversations as work making threads again, against
`2026-11-16-work-makes-no-thread.md`: "Put them in Explore, no new
threads".

Now:
- **An opener opens no conversation.** It runs as Magnus's own background
  work, the way a review does (`openers.rs`, actor `review`), under the
  review's instructions and its own text (`instructions/opener.md`,
  `opener-app.md`): look, then suggest. Each opener still starts once
  ever, never on a reconnection, and not while the kill switch is on. A
  conversation an opener opened before this stays as it is.
- **Its suggestions are ideas in Explore's For you.** A `suggest` call
  from a run that is not a chat turn was already a `suggestion` item. Such
  items now show at the top of Explore, under For you, each with Start
  (or Schedule) and Dismiss. They are no decision waiting: they are not
  in Approvals, make no box above the input, and are not in the
  overview's other items (`explore-ideas.js`, `app.js`). This covers a
  review's suggestions too: they are the same thing, Magnus's own idea of
  what it could take over. An empty Tasks or Schedules offers the ideas,
  linking to Explore.
- **One notification.** When an opener or a review ends having left ideas,
  the person gets one notice, "Magnus has 2 ideas for you in Explore"
  (`Kernel::tell_ideas`, reason `ideas`), shown once as a notification
  that opens Explore. A run that left none sends nothing.
- The cold-start opener looks at the person's apps, the company's
  knowledge, their schedules and memory. At a cold start they have no
  tasks, and a review is not offered the chat's task list.
- Suggestions made in a conversation stay cards in that conversation.

## Reason

Work belongs to the conversation that asked for it, and Magnus asking
nothing of anyone opens none. An idea is not urgent: it waits where the
person looks for what Magnus can do, and they are told once, by the
standing rule that notices are for finished work or a needed decision.

## Verification

- `tests/scratch/tests/cold_start_proposals.rs`, rewritten:
  - a first sign-in opens no conversation, and the opener runs under the
    review's instructions;
  - its two ideas cite what it read, and are only that person's;
  - there is one notice, "Magnus has 2 ideas for you in Explore";
  - starting an idea starts its task, and dismissing the other leaves none;
  - a second sign-in and a restart run nothing and tell nothing;
  - connecting an app reads it and leaves ideas citing that read, and
    reconnecting runs nothing;
  - nothing runs under the kill switch.

  Two tests of the old conversation are dropped: a deleted opener
  conversation, and reactions to the opener's message. That behaviour is
  removed.
- `desktop/tests/explore-ideas-check.mjs` (fast tier) checks:
  - For you lists the waiting ideas, newest first;
  - an idea is no decision waiting;
  - the notice links to Explore and is the one notification;
  - the empty lists offer the ideas;
  - Explore draws For you and redraws once an idea is answered.

Neither passes on the code before this change.
