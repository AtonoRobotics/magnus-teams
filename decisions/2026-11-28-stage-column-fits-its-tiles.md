# The stage beside a conversation fits what it holds

The owner, on test.419 in Nova: "The right side bar is larger than it should be for a simple task." The screenshot showed one sub-agent at work. The stage beside the conversation took 44% of its width and the whole height. It held a "While Magnus works" tip card, and a Live Workspace tile for that sub-agent, which the thread was already showing inline with its Stop button. Below the two cards the column was empty.

**Choice.**

- **Width.** A conversation at work with something on the stage is `minmax(360px, 1fr) clamp(280px, 30%, 360px)`, not 56% / 44%. A tile needs about 300px. The conversation keeps the rest: at 1440px the thread grows by about 200px.
- **Height.** The side column is as tall as its tiles (`align-self: start`, scrolling past the row's height). It has no tinted background or left rule running down the window, since the tiles are cards of their own.
- **No repeats.** The stage read its sub-agents from the thread's own inline card (`threadSubAgents`), so it could only repeat that card. With no task running in the conversation, it drew a tile of the sub-agents alone. Under a running task, it drew one row per sub-agent. Both are gone, with their renderer and style. A running task's tile still says how many sub-agents work for it ("· 1 sub-agent"). With a sub-agent alone, as in the owner's case, there is no stage, and the conversation has the whole width.
- **No tip card.** The "While Magnus works" carousel (decisions/2026-11-26-nova-theme-and-ux-overhaul.md) showed the same three product tips on every run, first in the column. It reports no work and asks for no decision, so it took the place of what the person came to see. It is removed, with its styles. The standing rule is that notices are only for finished work or a needed decision.

**Checks.** `desktop/tests/bento-stage-check.mjs` (fast) fails on the old width, height, sub-agent tile, sub-agent rows and tip card. The scratch test `stage_column` holds a sub-agent and a task at work on a real server and measures both conversations in Chromium at 1440 by 900, in Nova dark and light.
