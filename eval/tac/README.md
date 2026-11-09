# TheAgentCompany on Magnus: stack and runner design

Status (2026-10-27): the TAC 1.0.0 server stack runs on this host. Three task images are pulled and studied. The runner below is a design and is not implemented yet. Background facts are in `docs/research/providers-and-tac.md` §6. This file adds what we verified by reading the upstream code and running the stack.

- Upstream checkout (untrusted, read-only): `/tmp/claude-1000/-home-samuel-magnus-claude/5c142d0f-e7f2-45a4-a0fd-ef83627fbb7b/scratchpad/tac/repo` (shallow clone of `TheAgentCompany/TheAgentCompany`).
- `tasks.txt`: all 175 tasks. Columns: category, services, NPC, LLM-graded, trajectory-graded.
- `stack/`: our override files for the api-server image (see §1).

---

## 1. Server stack

### 1.1 How upstream starts it (read in full before running)

`servers/setup.sh`:
1. Pulls 18 images.
2. Runs `docker run -d --name api-server --network host --restart always -v /var/run/docker.sock:/var/run/docker.sock ghcr.io/theagentcompany/servers-api-server:1.0.0`.
3. Polls `localhost:2999/api/healthcheck/{rocketchat,owncloud,gitlab,plane}`.

The api-server image (`servers/api-server/Dockerfile`) is a `docker:27.3.1` image with Flask. On boot (`api-server.py`, `SKIP_SETUP` unset) it runs `make start-all` from its own `/workspace/Makefile` (= `servers/api-server/API_MAKEFILE`):
- `docker compose -p theagentcompany down -v`, then `up -d` with the baked `/workspace/docker-compose.yml`. That starts gitlab, owncloud, owncloud-collabora, rocketchat, mongodb, redis-stack and redis-stack-npc-data-population.
- `cd /plane && make start-plane`. Plane's `setup.sh start` brings up compose project `plane-app`. `restore.sh` then wipes the `plane-app_{pgdata,redisdata,uploads}` volumes and fills them with tarballs that a `busybox` container downloads from `github.com/TheAgentCompany/the-agent-company-backup-data/releases/download/plane-20241031-0351`.
- `make reset-rocketchat`, which runs `mongorestore --drop --archive < /rocketchat/db.dump` (baked into the image).

The api-server endpoints the task containers use are `POST /api/reset-{owncloud,gitlab,plane,rocketchat}` and `GET /api/healthcheck/{owncloud,gitlab,plane,rocketchat,redis,sotopia}`:
- The owncloud and gitlab resets are synchronous: the container is removed and recreated from its image, which holds the seed data.
- The plane and rocketchat resets are asynchronous threads. The rocketchat reset also recreates redis-stack and its NPC-profile population job.

### 1.2 What we changed, and why

The upstream files are mounted over the image's copies. The originals were extracted with `docker cp` to `.../scratchpad/tac/apiimg/`.

| Change | Why |
|---|---|
| `--restart no` on api-server; `restart: "no"` on every service in both compose files (upstream uses `always`, `unless-stopped`, `on-failure`) | Nothing may come back after a reboot. |
| `stack/docker-compose.yml`: mongodb image changed from `docker.io/bitnami/mongodb:5.0` to `bitnamilegacy/mongodb:5.0` | The image's baked compose still names `bitnami/mongodb`, which Bitnami has removed from Docker Hub. Upstream `main` and `setup.sh` already switched to `bitnamilegacy`. |
| `stack/docker-compose.yml`: `pull_policy: always` changed to `missing` | The images are pinned at 1.0.0. Without this, every per-task reset would hit ghcr. |
| `stack/docker-compose.yml`: `ulimits.nofile=65536` on gitlab, rocketchat, mongodb | Docker 29/containerd 2 sets the container default soft `nofile` to 1024. With that limit, mongod aborted with "Too many open files" about 5 minutes after start. GitLab's first boot also failed (`runsv not running` during `gitlab-ctl reconfigure`), and upstream's `restart: always` would have hidden both. With the higher limit, both have stayed up. |
| `stack/plane/Makefile`: `download-setup` is a no-op | Upstream's `start-plane` re-downloads `install.sh` from GitHub and runs it as root with the Docker socket mounted. We fetched it separately, read it, and confirmed it is byte-identical to the baked `/plane/setup.sh`. We run the baked copy instead of fetching code at runtime. |
| `stack/plane/plane-app/docker-compose.yaml`: minio image changed to `tac-local/minio:RELEASE.2025-09-07T16-13-09Z` | `minio/minio:RELEASE.2024-11-07T00-52-20Z` no longer exists on Docker Hub ("repository does not exist"), and quay.io returns 401. The replacement is a local retag of the official MinIO image already on this host (`minio/minio:latest`, release 2025-09-07). Plane only uses basic S3 calls. |
| `stack/Makefile` (the api-server's `/workspace/Makefile`): `start-all` and `reset-gitlab` wait on a new `gitlab-ready` target | The image's seeded GitLab access tokens (`root-token`, used by task setup and graders) expired on 2025-11-18, and a reset restores them expired; `gitlab-ready` extends them once the database is up. It also starts GitLab once more if its first boot exits (`runsv not running`). |
| `busybox:latest` is a local retag of `busybox:1.37.0` | Plane's `restore.sh` runs plain `busybox`. The retag avoids an unpinned pull. |

The restore data (Plane tarballs, RocketChat dump) is still the upstream data. The only code that runs is code we read: `api-server.py`, `utils.py`, both Makefiles, `setup.sh`, `restore.sh`, and the compose files.

`stack/up.sh` starts (or restarts) the stack with all of these mounted, keeps Plane's runtime state in `$TAC_HOME/stack` (default `~/.magnus-tac/stack`), and waits up to 30 minutes for the four health checks. On 2026-10-29 owncloud, gitlab and plane came up healthy; rocketchat's health check returned 500.

### 1.3 Commands (exactly what ran on 2026-10-27)

```bash
# ports checked free first: 2999 3000 8091 8092 8929 8930 2424 9980 6379 (ss -ltnH)
# pulled: the 18 images in servers/setup.sh (docker pull each); minio failed -> local retag:
docker tag minio/minio:latest tac-local/minio:RELEASE.2025-09-07T16-13-09Z
docker tag busybox:1.37.0 busybox:latest

T=/home/samuel/magnus-claude/eval/tac/stack
RUN=$(mktemp -d)/plane-app && cp -r $T/plane/plane-app $RUN      # setup.sh writes plane.env + archive/ here
docker run -d --name api-server --network host --restart no \
  -v /var/run/docker.sock:/var/run/docker.sock \
  -v $T/docker-compose.yml:/workspace/docker-compose.yml:ro \
  -v $T/plane/Makefile:/plane/Makefile:ro \
  -v $RUN:/plane/plane-app \
  ghcr.io/theagentcompany/servers-api-server:1.0.0
# wait: until curl -s localhost:2999 ...; then healthcheck loop as in servers/setup.sh
for s in rocketchat owncloud gitlab plane; do curl -s localhost:2999/api/healthcheck/$s; echo; done
```

Today's running instance mounts the same files from `.../scratchpad/tac/overrides/`. Those are identical to `stack/`. All services were healthy about 100 s after the api-server started; the images were already pulled.

The ulimit fix was applied to the running stack with:

```bash
docker exec api-server sh -c 'cd /workspace && GITLAB_PORT=8929 docker compose -p theagentcompany up -d mongodb rocketchat gitlab && make reset-rocketchat'
```

Checks run against the live stack:
- RocketChat: 18 users.
- Plane: 10 projects in workspace `tac`.
- GitLab: 13 projects.
- ownCloud: `/Documents/{Admin,Financials,...}` present.
- Every container shows `RestartPolicy=no`.
- The host `/etc/hosts` is unchanged.

**Stop** (containers stay stopped after a reboot either way; add `-v` to also delete the data volumes):

```bash
docker rm -f api-server
cd /home/samuel/magnus-claude/eval/tac/stack
GITLAB_PORT=8929 docker compose -p theagentcompany -f docker-compose.yml down -v
docker compose -p plane-app -f plane/plane-app/docker-compose.yaml --env-file plane/plane-app/plane.env down     # -v to drop plane volumes
```

### 1.4 Ports the stack occupies (all were free on this host)

| Port | Service |
|---|---|
| 2999 | api-server (host network) |
| 3000 | RocketChat |
| 8091 | Plane proxy |
| 8092 | ownCloud |
| 8929 | GitLab HTTP |
| 8930 | GitLab HTTPS |
| 2424 | GitLab SSH |
| 9980 | Collabora |
| 6379 | redis-stack (NPC profiles, password `theagentcompany`) |

The Docker-published ports bind on `0.0.0.0` and may be reachable from the LAN and the tailnet, since forwarded traffic skips the INPUT chain. Port 2999 is host-network, and INPUT DROP limits it to loopback. Disk: about 45 GB of images (GitLab alone is 29.7 GB). Each task image is about 2.4 GB, but they share the `task-base-image` layers.

**Running task containers with `--network host` is required on this host.** The host's iptables INPUT policy is DROP. A container on the default bridge reaches the published ports (via DNAT), but not the host-network api-server on 2999. `reset.sh` needs 2999, and we must not change the firewall.

### 1.5 Known stack hazards (the runner must handle these)

- **Async resets race the health check.** `reset.sh` POSTs the resets and then immediately polls health. For plane and rocketchat, the first poll can pass before the old instance has been stopped. After `init.sh`, the runner waits until `docker exec api-server ps` shows no `make reset-*`, `restore.sh` or `setup.sh` process, then polls health again.
- **Nothing restarts itself now.** After each init, the runner checks that every TAC container is `running`. If one has exited, it runs `docker start <name>` once and polls health again; a second failure fails the task with reason `infra`.
- **Run tasks one at a time.** All tasks share one set of services, and each init resets them.

---

## 2. One TAC task, end to end (upstream semantics)

Sources: `docs/EVALUATION.md`, `evaluation/run_eval.py`, and `workspaces/base_image/{Dockerfile,init.sh,reset.sh,eval.py,scoring.py,common.py,config.py,npc/*}`.

1. **Task container.** OpenHands uses the task image as the sandbox base, adds its runtime, and runs it with `use_host_network=True`.
   - The agent works inside this container as root.
   - A host temporary directory is mounted at **`/outputs`**. It carries the trajectory in and the result out; the agent never uses it.
   - `/workspace` is the working directory and the place graders look for local outputs (`/workspace` appears in 92 evaluators).
2. **Init.** `SERVER_HOSTNAME=localhost LITELLM_API_KEY=… LITELLM_BASE_URL=… LITELLM_MODEL=… bash /utils/init.sh` (OpenHands sets a 900 s timeout).
   - Pings `SERVER_HOSTNAME` and appends `<ip> the-agent-company.com` to the **container's** `/etc/hosts`.
   - `reset.sh` resets only the services listed in `/utils/dependencies.yml`.
   - Runs `pre_init`, then `/npc/run_multi_npc.py` if `/npc/scenarios.json` exists, then `populate_data.py` and `post_init`.
   - **The NPCs run as background processes inside the task container** (`run_one_npc.py` per coworker; sotopia, Redis at `the-agent-company.com:6379`, RocketChat bot accounts from `/npc/npc_credential.json`). The container must stay alive for the whole agent run, and the LITELLM variables must be present at init.
3. **Pre-login.** OpenHands logs its browser into the dependent services (`evaluation/browsing.py`). TAC allows any login method.
4. **Prompt.** `"Complete the task in /instruction/task.md"`. If gitlab is a dependency, OpenHands adds `"\n\nGitlab username is 'root' and password is 'theagentcompany'"`. Limits are 100 iterations and $4.
5. **Grade.** `LITELLM_*=… DECRYPTION_KEY='theagentcompany is all you need' python_default /utils/eval.py --trajectory_path /outputs/traj_X.json --result_path /outputs/eval_X.json`.
   - The flag is `--result_path`; `EVALUATION.md` wrongly says `--output_path`.
   - `eval.py` decrypts `evaluator.py.enc` and **leaves the plaintext `/utils/evaluator.py` in the container**. Grade only after the agent has finished, and never reuse the container.
   - Graders read four things: (a) service state through REST APIs as the `theagentcompany`/`root` users, (b) files in the container (`/workspace`, `/utils/*.xlsx` answer inputs), (c) processes in the container (`localhost:5000`, `:8182`, `:5432`), and (d) the trajectory text (66 tasks do substring checks, mostly URLs).
6. **Score.** `final_score = {total, result}` after the task's bonus strategy. Per task, `S = 0.5·result/total + 0.5·⌊result/total⌋`. The benchmark score is mean(S)·100, and "resolved" is the share of tasks with S = 1.

**We ran this pipeline on this host without an agent**, using `finance-budget-variance`:
- `init.sh` (ownCloud reset) took 9 s.
- `eval.py` on the untouched state returned `{"total":4,"result":0}`, which is S = 0.
- As a harness check only (the answer was computed from `/utils`), we uploaded the correct `flagged_variances.xlsx` through WebDAV and gave a trajectory containing `/Documents/Financials`. That returned `4/4`, S = 1.0.
- `init.sh` for the PM/NPC task took 63 s (plane and rocketchat resets). It launched the Zhang Wei NPC process, but with `LITELLM_*=None`, because no environment LLM is configured yet.

---

## 3. The three tasks studied

| | `sde-copilot-arena-server-easy-add-suffix` | `pm-ask-issue-assignee-for-issue-status-and-update-in-plane` | `finance-budget-variance` |
|---|---|---|---|
| Instruction | `/instruction/task.md` in the image (source: `workspaces/tasks/<name>/task.md`) | same | same |
| Services (`dependencies.yml`) | gitlab | plane, rocketchat (+NPC "Zhang Wei", `scenarios.json`) | owncloud |
| What the agent must do | Clone `http://the-agent-company.com:8929/root/copilot-arena-server` **into `/workspace` of the task container**, `pip install -r requirements.txt` (PyPI, i.e. internet), make the `create_pair` endpoint require `suffix`, and start `uvicorn … --port 5000 &` in the background | Find the assignee of "Evaluate data throughput" in Plane project RisingWave, DM them on RocketChat **as `theagentcompany`**, wait for the NPC's LLM reply ("completed"), and set the issue to Done | Read `/Documents/Financials/{budget,actual_spending}.xlsx` from ownCloud, compute variances (>10% and >$5000), and upload `/Documents/Financials/flagged_variances.xlsx` with column `Flagged_Variances` (`Dept_Cat_YYYYMM`) |
| Files in the task container | Empty `/workspace`; `/app/logs` exists | none | none for the agent; `/utils/{budget,actual_spending}.xlsx` are grader inputs and must not reach the agent |
| Grader (from `evaluator.py`/`checkpoints.md`) | cp1 (1 pt): `/workspace/copilot-arena-server` exists. cp2 (1 pt): `GET localhost:5000/list_models` returns 200. final (2 pt): POST `/create_pair` without `suffix` returns 400. `bonus_for_completing_final` gives 4/4 if final passes | cp1 (2 pt): `theagentcompany` has a non-empty DM history with `zhang_wei`. cp2 (1 pt, only if cp1): Plane issue state group is `completed`. Plain sum | cp1 (1 pt): trajectory contains `/Documents/Financials`. cp2 (3 pt): ownCloud file exists (0), has the column (1→2), matches the recomputed ID series exactly and in order (3). `bonus_for_completing_any` |
| Hard parts for Magnus | Grading runs **inside the task container's network namespace**: the clone, the installed packages and the running server must all be there. The sandbox VM cannot help | Asynchronous NPC reply: the agent must poll or wait. Needs env LLM credentials | Binary xlsx in and out of ownCloud; the URL must show up in the trajectory |

---

## 4. Runner design: `eval/tac/run.py`, host side

The runner is a host process with Docker access. It drives the appliance only through its public HTTP API, in the same way as the invariant tests. It never edits the appliance.

### 4.1 Per-task loop

```
for task in selected(tasks.txt):
  1. docker pull ghcr.io/theagentcompany/<task>-image:1.0.0      (skip if present)
  2. docker run -d --name tac-<task> --network host --restart no \
        -v $OUT/<task>:/outputs  <image> sleep infinity
  3. docker exec -e SERVER_HOSTNAME=localhost -e LITELLM_API_KEY -e LITELLM_BASE_URL -e LITELLM_MODEL \
        tac-<task> bash /utils/init.sh                             (timeout 900 s; once more on failure, as run_eval.py does)
     + §1.5 guards: wait for the api-server's async resets, check containers, re-poll health
  4. hardening: docker exec tac-<task> sh -c 'chmod -R go-rwx /utils /npc; useradd -m agent; chown -R agent /workspace'
  5. bind the bridge (§4.3) to container tac-<task>; reset per-task bridge state
  6. text = cat /instruction/task.md (verbatim)
     + if gitlab in deps: "\n\nGitlab username is 'root' and password is 'theagentcompany'"   (same as baseline)
     POST /v1/tasks {"text": text}  as the benchmark person  -> task id
  7. poll GET /v1/tasks/{id} every 10 s until status in {completed, failed, suspended, cancelled}
     or wall clock > 60 min  -> POST /v1/tasks/{id}/cancel
     suspended: see §4.5 (approvals)
  8. trajectory: GET /v1/tasks/{id}/timeline -> $OUT/<task>/traj.json        (§4.6)
  9. docker exec -e LITELLM_* -e DECRYPTION_KEY='theagentcompany is all you need' tac-<task> \
        python_default /utils/eval.py --trajectory_path /outputs/traj.json --result_path /outputs/eval.json
 10. results.jsonl += {task, category, magnus_status, total, result, S, checkpoints, wall_s,
                       model_calls, actions, usd, approvals_asked, approvals_granted, infra_errors}
     also copy to $OUT/eval_<task>-image.json so upstream summarise_results.py works unchanged
 11. docker rm -f tac-<task>   (kills the NPCs; decrypted evaluator gone)
```

Every task starts from a fresh appliance run with no memory carried over. Either the memory tools are turned off, or the runner uses a dedicated person whose memory is cleared between tasks. Otherwise later tasks would be graded on remembered answers.

### 4.2 How the agent reaches the TAC services

The Magnus run sandbox (a Firecracker VM) has no network. Every service call leaves through the boundary to host-side tools. There are two routes, and both are configured:

**A. MCP connectors (primary).** One host process, `tac-bridge` (MCP over streamable HTTP, `127.0.0.1` only), serves these apps. Each app is declared under `[[connectors]]` in the appliance config. The credentials sit in the vault (`credential = "tac-…"`), so neither the prompt nor the model ever sees a password.

| App | Tools (risk class) | Backed by |
|---|---|---|
| `tac-owncloud` | `list(dir)`, `stat`, `share_link`, `download(path)` (read); `mkdir`, `upload(path, file)`, `move`, `copy` (reversible_write); `delete` (irreversible) | WebDAV `/remote.php/webdav`, OCS shares API, user `theagentcompany` |
| `tac-rocketchat` | `channels`, `history(room)`, `dm_history(user)`, `users`, `wait_for_reply(user, timeout≤300 s)` (read); `post(channel, text)`, `dm(user, text)` (reversible_write: they are sent as the company account) | REST `/api/v1`, user `theagentcompany` (graders read **this** user's DMs) |
| `tac-gitlab` | `projects`, `get_file`, `tree`, `issues`, `mrs`, `wiki`, `pipelines` (read); `create_issue`, `comment`, `create_mr`, `commit_files`, `update_settings`, `close_issue` (reversible_write); `delete_*` (irreversible) | REST `/api/v4` with a `root` PAT created at bridge start |
| `tac-plane` | `projects`, `issues`, `issue`, `states`, `cycles`, `members` (read); `create_issue`, `update_issue`, `add_to_cycle` (reversible_write) | REST `/api/v1/workspaces/tac`, API key `plane_api_83f8…` from `servers/README.md` |
| `tac-workspace` | `ls`, `read(path)`, `export_to_sandbox(path)` (read); `write(path, file)`, `import_from_sandbox(path)` (reversible_write); `exec(cmd, timeout, background)` (irreversible, see §4.4) | `docker exec`/`docker cp` against `tac-<task>`, user `agent`, cwd `/workspace`; paths outside `/workspace` and `/tmp` are refused |

Each bridge tool result also includes the **canonical web URL** of what it touched, for example `http://the-agent-company.com:8092/index.php/apps/files/?dir=/Documents/Financials` or `…:8929/root/<repo>/-/…`. Trajectory checkpoints look for exactly these UI URLs (`dir=/Documents/Administrative%20Specialist`, repo URLs). A bridge that hid them would lose points the baseline earns by browsing.

The bridge talks to `localhost:<port>` with `Host: the-agent-company.com:<port>` (ownCloud's trusted domains include it). The host `/etc/hosts` is not touched.

**B. Built-in browser (for UI-only steps).** Some tasks name a UI page, for example the GitLab CI editor URL in `sde-add-one-gitlab-pipeline`, and some graders only check that the URL was visited. For these the agent uses `browser_*` (the host Chromium sidecar).
- Name resolution: the sidecar launches Chromium with `--host-resolver-rules="MAP the-agent-company.com 127.0.0.1"`, a runner-only option. Without it, `the-agent-company.com` resolves to the real public website.
- `web.allow_private_networks = true` is needed only if `web_fetch` should reach the services. The connectors make that unnecessary, so leave it off.
- The browser keeps no saved logins (DECISIONS.md), so the agent logs in itself. The usernames and passwords are allowed in the prompt by TAC. We send them only for GitLab, matching the baseline. For other UIs the agent would need them, so the runner offers an optional `--browser-creds` mode, recorded in results, that appends the `servers/README.md` credential lines. **Open item:** a sidecar pre-login hook, like OpenHands' `pre_login`, would avoid putting passwords in the prompt. It would need a kernel change.

### 4.3 Moving files: sandbox ↔ services ↔ task container

Today an MCP result reaches only the model's context. Nothing writes it into the VM, and a tool argument cannot reference a file in the VM. TAC needs binary files (xlsx, docx, pdf, png, zip) to cross in both directions, and base64 through the model is not an option. **Proposed kernel feature, the smallest that fits the boundary:**

1. **Import.** When an MCP result contains an `EmbeddedResource` blob (or a `resource_link` the kernel fetches from the same connector), the kernel writes it to `/workspace/imports/<app>/<name>` in the run's VM. It records it with the connector's label, as attachments are labeled today (DECISIONS.md: files land in `/workspace/inputs`). The model sees the path and size, not the bytes.
2. **Export.** A tool argument of the form `{"$file": "/workspace/out/flagged_variances.xlsx"}` is replaced by the kernel, inside the boundary, with the file's bytes as an MCP blob. It is hashed into the approval binding and the idempotency key like any other argument.

With this, `tac-owncloud.download` → sandbox (pandas runs in the VM, offline) → `tac-owncloud.upload({"$file": …})`. The same path covers `tac-workspace.import_from_sandbox` and `export_to_sandbox`, which move files between the VM and the task container's `/workspace`.

**Fallback without the kernel change:** the agent computes inside the task container through `tac-workspace.exec`. The image has Python 3.12, and the finance image even has pandas and openpyxl. The bridge then moves files between ownCloud and `/workspace` itself (`tac-owncloud.download_to_workspace` / `upload_from_workspace`). This works for every task, but it skips the VM.

`/outputs` is runner-only: the runner writes the trajectory there and reads the result. Neither the agent nor the bridge touches it.

### 4.4 Parts of TAC that cannot run in a no-network sandbox, and the bridge for each

| TAC need | Count / example | Why the sandbox can't | Bridge |
|---|---|---|---|
| Talk to GitLab, ownCloud, Plane, RocketChat | 172 of 175 tasks | no network | `tac-*` MCP connectors (§4.2A), or the host browser (§4.2B) |
| `git clone`/`push` of TAC repos | most SDE tasks | no network | `tac-workspace.exec` (`git` inside the task container, credentials via a bridge-written `~agent/.git-credentials`). For edit-and-push without running anything, `tac-gitlab.commit_files` |
| `pip install`, `npm`, `go mod`, apt from the internet | e.g. copilot-arena, bustub, raft, janusgraph, rising-wave | no network | `tac-workspace.exec` in the task container (host network, as in the baseline) |
| A service running on the grader's `localhost:5000/8182/5432` | 7 evaluators (copilot-arena ×3, janusgraph, rising-wave, debug-crashed-server, example) | the grader runs in the task container's netns; a process in the VM is invisible to it | `tac-workspace.exec(background=true)`; the process lives in `tac-<task>` |
| Files graded in `/workspace` | 92 evaluators mention `/workspace` | the VM's `/workspace` is a different disk | `tac-workspace.import_from_sandbox` / `write`; with the fallback, work happens there directly |
| Binary files to and from ownCloud | most DS/finance/HR/admin tasks | no network; no file handoff | §4.3 import/export (or the fallback) |
| Chat with NPC coworkers | 41 tasks | no network | `tac-rocketchat.dm` + `wait_for_reply`; the NPCs live in `tac-<task>` |
| Trajectory substring checks | 66 tasks | — | canonical URLs in bridge results, plus a full timeline export (§4.6) |

`tac-workspace.exec` is a **networked shell** on a host-network container. It can reach everything that listens on this host's loopback, including the appliance API and local Postgres. The mitigations:
- risk class `irreversible`, so it is never auto-executed by default;
- user `agent` instead of root;
- `/utils` and `/npc` unreadable after init (the answer keys live there; TAC forbids examinee access);
- every exec is logged;
- the runner flags a run as invalid if the trajectory shows any access to `/utils`, `/npc` or `evaluator`.

A tighter option, which needs the owner's approval because it changes host firewall rules: run task containers on a bridge network with `--add-host the-agent-company.com:host-gateway`, plus one iptables ACCEPT rule from docker0 to host port 2999 (§1.4). That keeps loopback out of reach.

Note: OpenHands ran as root. A few tasks may need root, for example to write outside `/workspace`. Track failures with reason `needs_root`.

### 4.5 Policy, taint and approvals

These settings go in the benchmark config. They are recorded with the results, because they decide how autonomous the run is:
- **Label.** All `tac-*` apps are labeled `private`, not `principal`. This is the synthetic company's internal data, and the agent's chat history and files flow between them. `tac-rocketchat` is `private+untrusted`, because coworker messages are third-party text: NPCs and seeded chats.
- **Audience.** `private` for owncloud, gitlab, plane and workspace; `internal` for rocketchat.
- **Policy layer `org/tac-bench`.** Permit `execute` for `read` and `reversible_write` on `App::"tac-*"`. `irreversible` (`tac-workspace.exec`, deletes) is an owner standing approval for the benchmark person only, so it applies only on this test appliance.
- **The fixed kernel rules stay on (Rule of Two, audience).** Once a run has read RocketChat (untrusted) and any private app, every non-read action asks, and only a one-action approval clears it. NPC tasks hit this on every action. The runner has three modes, recorded per result:
  - `strict` (default): a `suspended` status ends the task. It is graded as-is and counted as `suspended_for_approval`.
  - `approver`: the runner, acting as a configured `approver:*` person, approves each pending `/v1/inbox` item one action at a time (`POST /v1/inbox/{id}`). It logs the item and its reason, then `POST /v1/tasks/{id}/resume`. The scores are then **assisted**, and `approvals_granted` is reported next to S.
  - `review`: the run pauses for a human.

  Scores from `strict` and `approver` must not be mixed on one leaderboard line.
- **Budget.** Use the appliance default (`max_model_calls 300`, `max_usd 50`) or match the baseline's $4 per task. Record the choice. A budget suspension is a terminal status for the runner.

### 4.6 Trajectory

`eval.py` takes any text file, and the graders do substring searches on it. The runner writes `traj.json` as a JSON array of every timeline event, in order:
- the task text;
- each tool call with its app, tool, full arguments and result text, including the bridge's canonical URLs;
- browser navigations with their URLs;
- the model's final message.

Secrets are already redacted by the kernel. The file goes to `/outputs/traj.json` in the container. The same file is kept in `$OUT` for audit. Never hand the agent the trajectory, the checkpoints or the evaluator.

### 4.7 NPCs and LLM graders

- **Environment LLM.** It is separate from the agent model and is needed by:
  - 41 NPC tasks: `init.sh` launches the NPCs with these variables;
  - 53 LLM-graded tasks (`evaluate_with_llm`, `evaluate_chat_history_with_llm`, `llm_complete`): `eval.py` needs them.

  The runner passes `LITELLM_API_KEY`, `LITELLM_BASE_URL` and `LITELLM_MODEL` to **both** `init.sh` and `eval.py`. The task images pin `litellm==1.23.16` (early 2024). The safe choice is an OpenAI-compatible endpoint with `LITELLM_MODEL` set in a form that version accepts (`gpt-4o`, or `openai/<model>` with `LITELLM_BASE_URL` set). Upstream asks for something comparable to `claude-3-5-sonnet-20241022` or `gpt-4o`; all baselines used claude-3-5-sonnet. Report which model was used. The runner refuses to start NPC or LLM tasks without these variables, and they must never reach the agent's config or prompt.
- **NPC timing.** A sotopia NPC replies seconds to a minute after a DM. `tac-rocketchat.wait_for_reply` (a read, bounded at 300 s) avoids busy polling. The appliance's `wait_until` record also works.
- **Which identity.** Graders check DMs between `theagentcompany` and `<npc_username>`, so the bridge must post as `theagentcompany`, never as a separate bot user.

### 4.8 Appliance config for a benchmark run (sketch)

```toml
[[connectors]]
name = "tac-owncloud"
url = "http://127.0.0.1:7810/mcp/owncloud"
credential = "tac-owncloud"
label = "private"
audience = "private"
tools = { list = {risk="read"}, download = {risk="read"}, upload = {risk="reversible_write"}, mkdir = {risk="reversible_write"}, delete = {risk="irreversible"} }
# ... tac-rocketchat (label "private+untrusted", audience "internal"), tac-gitlab, tac-plane, tac-workspace
```

The tool definitions must be approved once by the owner on first sight (DECISIONS.md, tool pinning). The runner does this in a setup step and records the pinned hashes, so a bridge change cannot slip in silently mid-benchmark.

---

## 5. Blockers and open items

1. **No environment LLM credentials are configured.** 41 NPC tasks and 53 LLM-graded tasks will score falsely low without them, about 90 tasks with overlap. The PM smoke init started its NPC with `LITELLM_*=None`.
2. **Kernel file handoff (§4.3) does not exist.** Without it, all file work goes through the `tac-workspace` fallback. The bridge server itself is also not written yet.
3. **`tac-workspace.exec` is a networked shell on host networking.** 96 task.md files mention `/workspace`, cloning, installing, building or running something, and 7 graders probe a local server; most of these need it or the §4.3 handoff. The tighter bridge-network option needs a firewall rule, and therefore the owner's OK.
4. **Rule of Two vs. NPC tasks.** In `strict` mode, most RocketChat tasks will suspend. A decision is needed on which mode counts as the official Magnus number.
5. **Upstream image rot.** `bitnami/mongodb` and `minio/minio` are gone from Docker Hub; we worked around both (§1.2). Plane's reset still downloads its data from GitHub releases on every Plane task. Consider caching `pgdata/redisdata/uploads.tar.gz` locally and setting `PLANE_DOWNLOAD_URL`.
6. **Browser pre-login.** The browser has no saved logins, so UI-only steps need the passwords in the prompt (`--browser-creds`) or a sidecar login hook.
