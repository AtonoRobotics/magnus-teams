# Aurora, compact: scrollbars, one model chip, voice in reach

## Choice

Step F of the Aurora composition (`2026-11-21-aurora-composition.md`).

- **Scrollbars** in Aurora are thin and take the theme's colours. They are
  unseen until what they scroll is pointed at or holds the focus, then show
  as a faint thumb that firms when it is held. With forced colours, the
  system's scrollbars. Other styles keep the system's.
  (`desktop/ui/aurora-compact.css`)
- **The model and the reasoning level are one chip** in the composer,
  "claude-sonnet · Auto". It opens a small menu above it with the two
  choices; Esc shuts it and gives the focus back to the chip, as does a
  click elsewhere. The two choices stay the composer form's own selects, so
  what is sent is read as before. Other styles show the two menus.
  (`desktop/ui/aurora-compact.js`)
- **The mic uses the server's voice input when the server has it** (the
  `[speech]` config: whisper.cpp). What is said is recorded and the server
  writes it into the message for the person to read before sending; it
  never leaves the server. This works in any web view, including the desktop
  app's, which on most systems has no speech recognition of its own; that is
  why the mic was not shown. Without voice input on the server, the
  browser's own recognition is used where there is one; with neither, there
  is no mic. A recording stops by itself after two minutes. This is every
  style's composer, not only Aurora's.
- **Read aloud** stays on each answer's controls, shown when the answer is
  pointed at or has the focus, as the owner's rule asks ("When it doesn't
  need user focus it disappears").
- **Density.** The sidebar's lines were made as tall as their words in step
  C; the composer loses one of its two menus here.

## Reason

The owner: "I would add in the mic and read aloud if it's not already
present", "make the scroll bar match the theme styling and not be so
obtrusive", "Nothing should take up more space than it needs to make the
element look good." The server's voice input comes first because the
product keeps voice on the server (`crates/magnus/src/speech.rs`: "so voice
never leaves the appliance"); a browser's recognition may send it to the
browser's maker.

The production server has no voice input installed, so its desktop app
still shows no mic until it is; installing it is a host change for the
owner.

## Verification

`desktop/tests/aurora-compact.mjs` (the `appearance` scratch test): thin
scrollbars, unseen until pointed at, then themed, and the system's in
another style; one chip saying both choices and named for them, its menu
shut, opened above it, a new choice said and still the form's, shut by Esc
with the focus back and by a click elsewhere, two menus in another style and
one chip again on return; the mic recording through the server even where
the browser has recognition, the words written into the message, the
browser's own without it, and none with neither.
