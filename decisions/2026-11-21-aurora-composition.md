# Aurora adapts the mockup's composition to the app

## Choice

The website mockup is a reference for Aurora's elements, style and layout,
not a page to clone. Where the app's use differs from a web demo, the app and
the person come first. In Aurora and Obsidian 2 Aurora:

- **One frame.** The conversation and the canvas are two panes of one frame,
  styled in Aurora's own colours and glass. The canvas takes the room its
  content needs rather than a fixed split. With the canvas closed, the
  conversation keeps a comfortable reading width.
- **The input stays at the bottom**, always, for consistency. In a new,
  empty conversation the avatar and the prompt sit centred, as the mockup's
  opening does. After the first message the prompt settles at the bottom.
  The model and reasoning choices fold into one small chip.
- **Context boxes sit just above the input.** The reading order is history,
  latest answer, what needs you, then where you type. Boxes come and go with
  what is important (2026-11-20-aurora-stage.md), and their room grows and
  folds (2026-11-21-aurora-sizes.md).
- **The thread is the conversation's words.** Status drawn in the thread
  today becomes context boxes: the updates banner, the schedule card, the
  request's task group and the decision card. The thread keeps a one-line
  reference where each one happened.
- **Navigation is a rail.** It holds the avatar, New conversation, Search,
  Conversations and Settings. The full sidebar slides in over the stage on a
  click or a shortcut, never on hover. It closes when a conversation is
  picked, on Esc or on a click outside it, and can be pinned open. What needs
  the person arrives on the stage, so the sidebar only navigates.
- **The canvas is built around the decision:** its title and Close, the
  result, and the main action at its foot. Progress, steps and details sit
  behind one disclosure.
- **Magnus is an avatar, not an image.** Any avatar meets one small contract:
  mount it, set its state, take it down. Each style names its avatar, so a
  theme or the person can replace the orb. It follows the status line
  (ready, thinking, planning, working, waiting on you, done, failed) in colour
  and shape. It responds to the pointer, and clicking it shows what Magnus is
  doing. The status stays readable text. Reduced motion keeps colour and
  shape without movement, and the avatar pauses when the window is hidden.
- **Density.** Nothing takes more room than it needs to look right: list
  rows, the sidebar's lines, padding and gaps are sized to their content.
  Scrollbars are thin and in the theme's colours, and show when used.
- **Voice.** Voice input and read aloud, which the app has, are kept and
  reachable in the composer and on each answer. A purely conversational,
  voice-first mode is the next upgrade, and this layout leaves room for it
  through the avatar.

It is built in steps, each shown in a filmstrip of the real app before it
ships:

- A: the frame, boxes above the input, notices at sign-in;
- B: the thread as words;
- C: the rail;
- D: the avatar;
- E: the canvas;
- F: density, scrollbars and voice.

## Reason

The owner, after seeing the real app beside the mockup: "My vision for this
is not to copy exactly the mockup but adapt it from a web demo of the app to
the actual app. The layout should serve the app and the user first", "The
input box should remain at the bottom", "Nothing should take up more space
than it needs to make the element look good", and the orb should become "an
interactive avatar … treated as a replaceable element". The owner agreed to
the proposal above on 2026-11-21.
