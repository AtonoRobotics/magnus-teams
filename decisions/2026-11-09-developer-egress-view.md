# The developer view shows each computer's egress live, and pauses or starts it

The spec's developer view is "full observability of agents, the computer and its policy decisions, with the ability to pause or start", and egress is "audited in its own table, keyed by computer". The table was written but nothing read it.

- `GET /v1/dev/computers` lists every agent computer with its owner, status, who has control and its allowed destinations. `GET /v1/dev/computers/{id}/egress?after=` reads the audit table for one computer after the last id seen, oldest first, at most 500 rows a read. The view polls it every two seconds and shows each request as allowed, blocked or result, newest first, with a filter. Polling the id cursor is the same pattern as the run log, so no new streaming channel.
- Developers see this by default: it is the audit log, which the 2026-11-08 operator-visibility decision gives developers. The screen and files stay behind a session the user grants.
- A query string can carry a secret, so a developer sees only the path for a computer that is not their own. An identity's browser is already recorded with the path only.
- `POST /v1/dev/computers/{id}/pause` and `/start` behind the developer role run the same code as the computer page's Pause now and Start (`kill_this`, `start_this`): a pause is the kill switch, every grant revoked and the VM frozen in one step. A developer may only pause a running computer, never destroy or reset one; deleting stays with the person it belongs to.
- There is no separate pause of egress alone. The spec names pause and start of the computer, and a computer whose network is cut while the agent keeps working would leave its run failing in confusing ways; the kill switch already stops all egress with the VM.
