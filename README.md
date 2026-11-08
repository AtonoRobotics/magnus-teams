# magnus-teams

A team workspace where people and agents (Claude, Codex, Grok, Muse,
Magnus, …) work on the same projects without copy-pasting context
between tools. A deterministic coordination kernel (claims, expiry,
conflicts) underneath; five messages on top (claim, plan, touch,
heartbeat, handoff); any orchestrator is only a participant.

Build documents: [docs/principles.md](docs/principles.md) ·
[docs/product.md](docs/product.md) · [docs/architecture.md](docs/architecture.md) ·
[docs/build-plan.md](docs/build-plan.md). Principles win on conflict.

## Phase 0 (this code)

- `magnus_teams/kernel.py` — deterministic claim/heartbeat/expiry/
  conflict projection over an append-only JSONL event record.
- `magnus_teams/cli.py` — the five messages as a harness CLI.
  `python -m magnus_teams.cli claim --issue 42 --actor codex --files a.py`
  Add `--github owner/repo` to mirror the message as an issue comment.
- `magnus_teams/server.py` — watcher/status server for the witness
  host: `GET /` (who is doing what), `GET /api/status`, `POST /events`.

## Run

Locally: `python -m magnus_teams.server` then open http://localhost:8080

On the witness host: `docker compose up -d --build`
The server binds 0.0.0.0:8080 (LAN + Tailscale, not loopback-only).
Headless witness host (strix-alpha): LAN http://192.168.0.50:8080 ·
Tailscale http://100.92.130.94:8080 · wired http://10.42.0.1:8080
(UFW allows 8080 only from 192.168.0.0/24, 10.42.0.0/24, 100.64.0.0/10.)

Tests: `python -m unittest discover -s tests -v`

First live trial: Phase 1 in docs/build-plan.md (two agents, two
issues from the Magnus repo queue, run at once).
