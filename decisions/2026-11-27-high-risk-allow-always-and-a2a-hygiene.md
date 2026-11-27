# 2026-11-27 — High-Risk "Allow Always" Safety & Outside-Agent Key Hygiene (Tier 4 Phase 1)

## Choice
1. **High-Risk Approval Safety (`asks.js`, `app.js`)**:
   - Tagged `allow_always` buttons on `irreversible` and `financial` asks with `data-risk="${tier}"`.
   - Added an explicit danger confirmation modal in `respond()` before saving a standing `approve:always` rule on an `irreversible` or `financial` action.
   - Clarified inside `Edit before allowing` that edits apply to the single action only, and downgraded `approve:always` to `approve:once` whenever `body.edited_input` is present so edited parameters never silently create a standing rule.
2. **Outside-Agent (A2A) Key Hygiene (`outside-agents.js`)**:
   - Added 1-click **Copy key** and **Clear** buttons beside the one-time `inbound_key` banner.
   - Registered `onLeave(() => { OutsideAgents.key = null; })` so the one-time key is wiped from memory as soon as the user navigates away.

## Reason
- Prevents accidental standing rules on destructive/financial actions and ensures one-time A2A inbound keys cannot linger in memory after setup.
