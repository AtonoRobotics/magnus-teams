# 2026-11-19: Unified side panel, CI ceremony removal, and platform hardening

## Context

A comprehensive codebase and feature review surfaced six areas for improvement:
1. **CI ceremony & bureaucracy**: `ci/ratchet.sh size` failed when a file in `ci/oversize.txt` shrank unless the baseline was manually edited in lockstep, and `desktop/tests/chat-view-check.mjs` enforced a hardcoded alphabetized list of every `data-*` attribute in `chat-view.js`.
2. **Duplicated right side panels**: Personal chat (`desktop/ui/app.js`) and Team chat (`desktop/ui/team-panel.js`) maintained separate HTML shells, headers, brand blocks, tab bars, waiting banners, active-work cards, and computer footers even though Team is a multi-user, multi-agent extension of the same chat interface.
3. **P0 security, privacy & WebView bugs**:
   - `admin_app::remove::changes()` and `remove()` added the successor (`to`) to a removed person's workspace whenever the removed person was the last `Role::Admin`, even if `to` was not a member of that workspace (or was a server admin outside the workspace).
   - Native `confirm()` calls remained in `admin-ops.js`, `admin-people.js`, and `admin-security.js`, which are non-functional in the Linux/Windows Tauri WebView.
   - Dead `case "contract_change":`, `case "review":`, and `case "policy_proposal":` branches remained in `desktop/ui/app.js`, `/ui/*` responses lacked `X-Frame-Options: DENY`, `proactive.rs` duplicated timezone fallback logic, and several internal API error messages still referred to `"appliance"` instead of `"server"`.
4. **Self-service account capabilities**: Users could not enroll/remove their own 2FA (TOTP), auto-detect/set their IANA time zone in Account settings, export their personal data (threads, memories, receipts) as a JSON archive, or delete their own account.
5. **Team routines & offboarding**: Team routines existed in the backend (`crates/magnus/src/team/routines.rs`) with no UI in the Team right side panel and no `GET`/`PATCH` endpoints, and routines created by an admin who later left or was demoted below `Role::Admin` were not automatically stopped in the ledger.
6. **Agent autonomy, steering, undo, and model failover**: Users could not steer an active task mid-execution without stopping it, reversible write receipts in the Evidence drawer had no 1-click Undo action, `config::Model` had no `fallbacks` list when a provider returned `Unavailable` or `UsageLimit`, and `.github/workflows/release.yml` built and published without running the fast verification suite first.

## Decision

1. **Trim CI ceremony without splitting cohesive files**:
   - Do not split cohesive modules like `crates/magnus/src/events.rs` or 799-line files like `crates/magnus/src/kernel.rs`.
   - Remove the `$2 < base[$1]` failure branch in `ci/ratchet.sh` so shrinking a file never fails CI.
   - Remove the hardcoded `DATA_ATTRIBUTES` list in `desktop/tests/chat-view-check.mjs` while keeping the guard against retired attribute names.
2. **Unify the Personal and Team right side panel in `desktop/ui/side-panel.js`**:
   - Extract `SidePanel` (`brand`, `head`, `tabs`, `waitingBanner`, `activeCard`, `computerFoot`, `shell`) into `desktop/ui/side-panel.js`.
   - Render both the Personal right panel (`app.js`) and the Team right panel (`team-panel.js`) through `SidePanel`, parameterizing only multi-user/multi-agent differences (`several`, tab list, and drawer body).
3. **Harden workspace removal and WebView confirmations**:
   - In `crates/magnus/src/admin_app/remove.rs`, guard workspace admin succession and routine transfer with `successor_in(team, ws, id, to)` so a person is never added to a workspace they do not already belong to.
   - Replace all remaining `confirm()` calls across `desktop/ui/admin-*.js` with `await ask(...)` and enforce the ban across every `.js` file in `desktop/ui/` via `companion-panel-check.mjs`.
4. **Add self-service TOTP, timezone selector, data export, and account deletion**:
   - Add `POST /v1/accounts/me/totp`, `POST /v1/accounts/me/totp/confirm`, `DELETE /v1/accounts/me/totp`, `GET /v1/accounts/me/export`, and `POST /v1/accounts/me/delete`, plus corresponding UI controls in the Account sheet (`desktop/ui/password.js`).
5. **Complete Team routines UI and offboarding**:
   - Stop a workspace routine automatically in `team::routines::apply` whenever its creator leaves the workspace or loses `Role::Admin` standing.
   - Add `GET /v1/workspaces/{ws}/routines` and `PATCH /v1/workspaces/{ws}/routines/{id}`, include routines in `GET /v1/workspaces/{ws}/panel`, and render routine management controls in the Team right side panel Activity tab.
6. **Add in-flight task steering, reversible receipt undo, model provider failover, and release verification**:
   - Add `POST /v1/tasks/{id}/steer` and forward chat messages in a thread with a running task as steering `Ev::Notice` events.
   - Expose `"risk"` and `"reversible"` on `/v1/tasks/{id}/receipts` and render a 1-click `Undo` button in `desktop/ui/evidence.js`.
   - Add `fallbacks: Vec<String>` (`"adapter/model"`) to `config::Model` and fail over automatically in `Kernel::step` on `ErrorClass::Unavailable` or `ErrorClass::UsageLimit`.
   - Gate `.github/workflows/release.yml` on the fast verification checks (`cargo fmt --all --check`, all `desktop/tests/*-check.mjs`, `ci/ratchet.sh size`, `ci/ratchet.sh clippy`, and `cargo test --workspace --lib --bins`).
