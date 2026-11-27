# A new conversation's launch tiles are one tap

## Decision
On Nova's New Chat, a launchpad prompt or the Live Workspace tile sends its
work when clicked. The conversation then opens on that work. Before this, a
click only put the prompt in the composer.

The Editorial Playbooks and Apps & Skills tiles stay links to Explore and
Apps.

## Reason
On 2026-11-26 the owner called the Live Workspace tile on new threads "a dead
link". The text appeared in the box, but nothing else on the screen changed.

The spec (§4.3) calls these "clickable one-tap workflow prompts". In the
mockup, each one calls `launchStage`, which starts the work and goes to the
stage.

Sending only starts the work. Anything that leaves the workspace still waits
for an approval, as with any typed message.

The real-app screen test (`desktop/tests/nova-screens.mjs`) clicks the tile
and checks that a conversation opens with the prompt sent.
