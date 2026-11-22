# Aurora's boxes come and go, and each can be closed

## Choice

The boxes above the input in Aurora and Obsidian 2 Aurora used to stay for
as long as their condition held. Live Workspace stayed while any task in the
conversation ran. Up Next stayed while a routine was due within a day, Files
for an hour after a new file, and Recent Work for an hour after work
finished. There was no way to close one. The owner: "The desktop app bento
boxes are still lined up across the bottom. There is no way to close them.
If a task is running they appear. Some tasks run persistently so the boxes
would stay."

Now (`desktop/ui/aurora-boxes.js`, used by `aurora-stage.js`):
- **A box that wants the person stays** until it is dealt with: a decision,
  work waiting on them, the computer asking for them, an app needing
  attention, or a teammate's message. A decision just answered also stays
  while it says the answer.
- **Any other box slides in when it appears or changes, and leaves after
  15 seconds.** This covers work running or just finished, Up Next, new
  files, the computer working, Recent Work and what happened while the
  person was away. Work that runs for hours shows its box when it starts,
  when it needs the person and when it finishes, but not the whole time it
  runs. A new status line is not a change. The avatar and the conversation
  still say what Magnus is doing.
- **The pointer or the focus on the boxes keeps them.** Once the pointer
  leaves, they leave 15 seconds later.
- **Every box has a Close (×).** With a mouse it shows when the pointer is
  on the box; on a touch screen it always shows, with a 44 px target.
  Closing puts the box away and opens nothing. A closed box stays away
  until it changes: another task, another decision, or the same one in
  another state. Closing a decision lets the next one waiting take the box.
  The decision itself stays in Approvals, which the dock still counts.
  This browser remembers the latest 200 closed boxes.
- Other styles are unchanged.

## Reason

The owner's rule for the interface: "What is important slides into view.
When it doesn't need user focus it disappears." Work that is running does
not need the person's focus, so its box should not sit there for as long as
the work runs. What does need them stays until they deal with it, or until
they close it.

## Verification

- `desktop/tests/aurora-boxes-check.mjs` (fast tier) checks the rules:
  - what stays, and what leaves on time;
  - a new status line against a real change;
  - the pointer keeping a box;
  - Close remembered, and capped at 200;
  - a browser that stores nothing.
- `desktop/tests/aurora-stage.mjs` (the `appearance` scratch test), in a
  real browser:
  - work that keeps running shows its box, which leaves on its own after
    15 seconds;
  - a new status line brings nothing back;
  - work needing the person and a decision stay;
  - Close shows on hover, and closing opens nothing;
  - a closed decision lets the next one in;
  - a box under the pointer stays, and leaves after the pointer goes.

Neither passes on the code before this change.
