# Magnus is an avatar, not an image

## Choice

Step D of the Aurora composition (`2026-11-21-aurora-composition.md`).

- **One small contract.** An avatar is anything that can be mounted into an
  element at a size, told each state, and taken down:
  `AuroraAvatar.define(name, (host, { size }) => ({ set(state), destroy(), look?(x, y) }))`
  (`desktop/ui/aurora-avatar.js`). Each style names its avatar in theme.js
  (`Theme.avatar`): Aurora and Obsidian 2 Aurora name the orb. A theme puts
  another avatar in its place by defining one and naming it; the orb is then
  taken down wherever it was shown and the new one mounted and told the
  state. A person's own choice of avatar is left for later: nothing in the
  contract stands in its way.
- **Where it stands for Magnus:** the top of the navigation (the rail and the
  sidebar), and the middle of a new conversation, above the greeting. The
  orb aurora.js drew in those two places is gone; its other orbs (the
  narrow window's bar) are unchanged.
- **The states** follow the line above the composer and the answer's
  reasoning pill: ready, listening, thinking, planning (a plan laid out,
  nothing started), working, waiting on you, the person in control, done and
  failed. Done shows for 4 s after a run is seen ending well, failed for 8 s
  after one is seen ending in failure; then ready. A run the person stopped
  is not a failure, a run already ended when first seen (an earlier answer,
  another conversation) shows no end, and a new run ends the last one's.
- **The orb** (`desktop/ui/aurora-orb.js`) shows each state in colour and in
  shape, so neither alone carries it: ripples for listening, a light
  swirling inside for thinking, three points going round a dashed ring for
  planning, an arc running round its rim for working, a steady halo for
  waiting, a hollow ring when the person has the controls, a tick for done
  and a bar for failed. It turns from one look to the next over 600 ms and
  leans toward the pointer.
- **It is a button.** Its name says the state in words ("Magnus is waiting on
  you. Show what Magnus is doing"), and the line above the composer still
  says it as text. Clicking it opens the decision waiting longest on the
  canvas if one waits, else the Activity page (Aurora shows Activity as a
  page, not on the canvas).
- **Motion.** In full motion it moves; with reduced motion or none it is
  drawn once for each change of state, still. It draws nothing while the
  window is hidden.
- **A new conversation opens at its top**, where the avatar and the greeting
  are, once each time it is opened. The composition plan said the avatar and
  the prompt sit centred in an empty conversation; the owner's "The input
  box should remain at the bottom. That should be consistent for usability"
  is the firmer rule, so the input stays at the bottom and the avatar and
  greeting are centred above it.

`aurora-reasoning.js` marks its pill with how the run ended (`data-outcome`)
and whether its plan is laid out with nothing started (`data-planned`), for
the avatar to read.

## Reason

The owner: "The orb status I would like to make an interactive avatar not
just an animated image. It should represent magnus and change color or shape
based on the status line. For example working, thinking, planning. The orb
itself may be changed by themes so it should be treated as a replaceable
element that another avatar could replace." The next upgrade is a
voice-first conversation, which will lean on the avatar as the thing the
person talks to; the contract keeps that open.

## Verification

`desktop/tests/aurora-avatar.mjs` (the `appearance` scratch test): the
mounts and the button's name, each state and its words, done and failed
shown and then ready, a stopped or earlier run not shown as an end, colour
by state, the click, the pointer, motion in full, still with reduced motion
and redrawn for a new state, nothing drawn while hidden, another avatar
taking the orb's place, and another style with none.
`desktop/tests/aurora-rail.mjs` checks, in the real app, the avatar at the
rail's top and in a new conversation, which opens at its top.
