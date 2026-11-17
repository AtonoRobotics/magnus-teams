# Content-Rich Playbook Hero and Explore Gallery

## Context
Testers repeatedly reported not understanding what to do with Magnus or how to use it. The empty-state new conversation view (`#/chat/new`) showed only a greeting and four single-line text buttons, with no explanation of Magnus's connected apps, Cloud Computer, background schedules, or approval gates—and once a user had any conversation, `home()` opened the latest thread instead of a discovery surface.

## Decision
1. Added `desktop/ui/discover.js` (registered in `desktop/ui/index.html` and `crates/magnus/src/ui/assets.rs`) with a curated playbook catalog (`PLAYBOOKS`) across 5 categories (`Daily Ops`, `Research`, `Monitor`, `Engineering`, `Teams & Files`).
2. Upgraded the personal chat empty state (`Discover.heroHtml()`, wired through `personal-chat.js` and `chat-view.js`) into a Prism / Google AI Studio-style landing surface: personalized greeting, capability quick-action pills (`Connect apps`, `Find automations`, `How Magnus works`, `All playbooks`), category filter tabs, 2-column rich playbook cards (mode badge, description, tool tags, and 1-click prompt insertion), and a 3-pillar "How Magnus works" strip.
3. Added a top-level **Explore** (`#/explore`) sidebar link right below **New conversation** (`app.js`, `layout.css`) and a full searchable **Explore** gallery (`Discover.show(main)` in `pages.css`) showing live connected app status, expandable workflow steps and prompts, and 1-click **"Use in new chat"** launch.
4. Added unit tests in `desktop/tests/discover-check.mjs` and wired them into `.github/workflows/fast.yml`.

## Reason
Showing concrete, multi-step workflows alongside live app connections, Cloud Computer capabilities, and approval gates—both on arrival in a new conversation and via a persistent Explore destination in the sidebar—gives newcomers immediate clarity on how Magnus differs from a plain chatbot and lets them launch real workflows in one click.
