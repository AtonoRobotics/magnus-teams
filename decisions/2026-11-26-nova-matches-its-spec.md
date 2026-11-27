# Nova matches its spec and mockup

## Decision
A review on 2026-11-26 read the Nova code against `docs/nova-greenfield-spec.md`
and `docs/nova-greenfield-mockup.html`. These fixes follow from it:

- **Who sees Nova.** Saved looks move to Nova once
  (2026-11-26-saved-looks-move-to-nova-once.md). Nova's `--panel` is its own
  matte surface (#141517 dark, #F4F6FA light), as spec §2 says; before,
  `tokens()` forced every style's panel to `--main`.
- **Styles of its own.** Nova styles the reasoning disclosure, the
  composer's status line, stage-tile controls, decision boxes and the
  confirmed pill in its own sheets. It also defines its own keyframes, so
  it needs nothing from an Aurora sheet (spec §1.1).
- **New Chat.** The intro's order holds even when stage boxes show:
  companion and title, composer, then the showcase, centred. The title is
  "Where should we start?", as in the mockup.
- **Light mode.** The stage is white and the rail and sidebar stay #F4F6FA.
- **Batch approval.**
  - ⌘K's "Approve all" sends the ids as `Server.inbox.batch` takes them. It
    was refused every time.
  - A batch holds only approvals of the same kind as the first one that
    waits (the same tool).
  - A batch never holds Tier 1 approvals (money, or what cannot be undone)
    or prohibited ones. Before, irreversible approvals could be allowed in
    bulk.
- **Top bar.** In Nova the computer chip and Share sit in the ••• menu.
  The quick controls are painted with the bar, so a repaint no longer drops
  them.
- **One inbox count.**
  - The waiting count shows only on Inbox.
  - The sidebar toggle no longer carries it.
  - The rail's sidebar button counts only work that runs.
  - The rail labels are the spec's: "Inbox / Approvals", "Briefing &
    Routines", "Apps & Skills".
  - The rail is 60px and its badge is amber.
- **Briefing.**
  - The summary is written from what `/v1/briefing` sends: what finished,
    what runs, drafts staged, commitments overdue and the next routine.
    Before, it read a field the server never sends.
  - Only routines that still run are listed, each with its cadence.
  - A paused routine offers no Run now.
  - Dry run says what the routine would ask.
  - The mockup's spend card (this month's spend, from `/v1/spend`, the only
    period the server keeps) and its snooze button are added.
- **Apps.** The spotlight comes right under the page's header. The cards
  stack their head, text and foot.
- **Right panel.** In Nova the stage shell keeps the computer footer, and
  the panel has one left hairline.

The mockup's other pieces stay unbuilt, since the spec does not ask for
them. Some would make new promises: a daily spend budget, an undo of the
last step on every run, project members and project memory. The others
duplicate what exists:
- The Chat / Autonomous Task switch is one of the hero tabs that spec §4.3
  hides.
- The state demo pills are in Settings > Appearance.
- Explore's category pills already exist.

## Reason
The owner asked on 2026-11-26 for every UI problem the review found to be
fixed. Where the mockup and the spec text differ, the spec text wins, and
the Magnus Agent Computer spec wins over both.
