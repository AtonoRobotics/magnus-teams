# Provider adapters + TheAgentCompany: research notes (2026-10-27)

How to read this:
- Every fact has a source next to it.
- **[BIN]** means I found it with `strings` or `--help` on a local CLI binary. It shows what the shipped client does. It is not a documented contract.
- **[SRC]** means I read it in a shallow clone of a GitHub repo, taken today. Commit hashes are listed below.
- **UNCONFIRMED** means I could not find it in a primary source.
- I did not open any credential, token or auth file, and none are reproduced here.

Local binaries I inspected:
- `~/.local/bin/muse` reports "Muse Code 1.4.13 (1.4.13-R6996.1)".
- `~/.local/bin/grok` reports "grok 1.0.70 (f50904eaee6c) [stable]". It points to `~/.grok/downloads/grok-1.0.70-linux-x86_64`.

Repos I cloned:
- `openai/codex` at `eec8bde1` (2026-10-27).
- `TheAgentCompany/TheAgentCompany` at `98b68ef8` (2025-11-17).
- `TheAgentCompany/experiments` (2026-07-31).
- `TheAgentCompany/theagentcompany.github.io` (2026-07-31).

---

## 1. Meta Model API ("Muse" family)

### Endpoint and auth
- **Base URL:** `https://api.meta.ai/v1`. It is an HTTP API that takes a bearer token. https://dev.meta.ai/docs/api-reference
- **Auth header:** `Authorization: Bearer $MODEL_API_KEY`. https://dev.meta.ai/docs/authentication.md
  - Keys look like `LLM_<numeric id>_<secret>`. You create them in the dashboard at https://dev.meta.ai/ (API keys tab). https://dev.meta.ai/docs/authentication.md
- **Muse Code CLI auth.** Source: https://dev.meta.ai/docs/muse-code/auth.md
  - It reads the `META_API_KEY` environment variable first, then a stored key, then a stored browser session.
  - `muse auth set` stores a key.
  - Meta Managed Accounts (MMA) cannot use browser sign-in.
- **Muse Code browser sign-in internals [BIN].** Seen in the `strings` output of `muse-bin-1.4.13-R6996.1`. Treat as inferred.
  - The issuer is `https://auth.meta.com`.
  - It uses the OIDC **device-code** grant:
    - `/oidc/device/authorization`
    - `/oidc/device/token`
    - grant type `urn:ietf:params:oauth:grant-type:device_code`
  - It uses **dynamic client registration**. The registration request carries `client_name` "Muse Code", `redirect_uris`, `grant_types` and `token_endpoint_auth_method`. The error string "the registration response carried an empty client_id" fits this.
  - **I found no static client ID.**
  - After the grant, the CLI "mints" an API key. Strings: `https://api.meta.aimuse-code/key`, "mint returned an empty api key".
  - It also calls these paths:
    - `/muse-code/models`
    - `/muse-code/browser_open`
    - `session/read`
  - Other strings: "No Meta credentials were found. Run `muse login`", "finish onboarding at https://dev.meta.ai", and the subscribe link `https://accountscenter.meta.com/muse_code/?ep=no_payg`.
  - Request headers seen [BIN]:
    - `x-client-id: tbh:{tui|exec|desktop|serve}`
    - `x-api-version: 1.0.0`
    - `x-meta-ai-gateway-session-id`
    - `x-tbh-thread-id`, `x-tbh-root-session-id`, `x-tbh-parent-thread-id`, `x-tbh-invocation-kind`
    - `x-tbh-contributor-mode`
    - `x-fb-routing-control`
    - `traceparent`
  - **There is no documented OAuth flow for third-party apps.** UNCONFIRMED whether Meta allows other clients to use this flow.

### Wire protocols (all three are first-party)
- **Chat Completions:** `POST /v1/chat/completions`. It is OpenAI-SDK compatible. https://dev.meta.ai/docs/api-reference/chat-completions/create-chat-completion.md
  - Chat does **not** carry reasoning across turns.
  - These return HTTP 400: `stop`, `n>1`, `logit_bias`, `prediction`, `logprobs: true`, and `reasoning_effort: "none"`.
  - `frequency_penalty` and `presence_penalty` are not supported.
  - Source: https://dev.meta.ai/docs/protocols/chat-completions.md
- **Responses API:** `POST /v1/responses`, plus retrieve, delete, cancel and count-input-tokens. https://dev.meta.ai/docs/api-reference/responses/create-response.md
  - Meta recommends **stateless encrypted replay** for agents: `store: false` with `include: ["reasoning.encrypted_content"]`.
  - Every replayed reasoning item needs a `summary` field (use `[]` if empty). The `rs_` id is optional.
  - Sending `include: ["reasoning.encrypted_content"]` together with `previous_response_id` returns 400 (`param: "include"`).
  - `instructions`, `tools` and `reasoning.effort` do not persist across `previous_response_id`. Resend them on every request.
  - Source: https://dev.meta.ai/docs/protocols/responses.md
  - The Muse CLI itself uses this request shape [BIN]. Its strings include `previous_response_id`, `store`, `parallel_tool_calls`, `include` and `encrypted_content`.
- **Anthropic Messages compatible:** `POST /v1/messages` and `POST /v1/messages/count_tokens`. https://dev.meta.ai/docs/protocols/messages.md
  - Anthropic SDK setup: `base_url="https://api.meta.ai"` with `auth_token=` (a bearer token, not `x-api-key`).
  - These return 400: `stop_sequences`, `top_k`, `container`, `inference_geo`, unknown top-level fields, `thinking: {type: "disabled"}`, and named tool choice (`{type:"tool"}`).
  - Supported tools are `custom` functions and the built-in `web_search`.
  - Reasoning comes back as `thinking`. Encrypted reasoning comes back as `redacted_thinking`.
  - Usage reports `cache_read_input_tokens` and `output_tokens_details.thinking_tokens`.

### Tool calling and streaming
- Tool calling uses `function` tools (and `custom` tools in Responses), with `tool_choice` and `parallel_tool_calls`. https://dev.meta.ai/docs/tool-calling.md
- Without `previous_response_id`, tool results must stay in the same order as their tool calls. https://dev.meta.ai/docs/tool-calling.md
- **Responses SSE events:** https://dev.meta.ai/docs/protocols/responses.md and https://dev.meta.ai/docs/cookbook/streaming-responses.md
  - `response.created`, `response.in_progress`
  - `response.output_item.added` / `.done`
  - `response.content_part.added` / `.done`
  - `response.output_text.delta`
  - `response.function_call_arguments.delta` / `.done`
  - `response.reasoning_summary_text.delta`
  - `response.completed`, `response.incomplete`, `response.failed`
- **Mid-stream failures** arrive as a terminal `response.failed` event, or as a top-level `type:"error"` event carrying `code` and `message` (for example `server_shutting_down`). https://dev.meta.ai/docs/error-handling.md
- Messages streaming uses the standard Anthropic event sequence (`message_start` … `message_stop`). https://dev.meta.ai/docs/protocols/messages.md

### Models (IDs are version-numbered, not date-stamped)
Source: https://dev.meta.ai/docs/models.md

| Model ID | Notes |
|---|---|
| `muse-spark-1.3` | Latest. Recommended. Supports all effort levels, including `max`. |
| `muse-spark-1.3-contributor` | Contributor mode (your data is used for training). No `max` effort. |
| `muse-spark-1.2` / `muse-spark-1.2-contributor` | Previous version. |
| `muse-spark-1.1` | Original. Standard mode only. Not offered in Muse Code. |
| `muse-wonder-1.0` | Preview. Meta's most capable model. Standard mode only on pay-as-you-go. "Muse Wonder Full" exists only in a Muse Code subscription. |

- All text models above have a 1,048,576-token context window. https://dev.meta.ai/docs/models.md
- Fixed-version IDs: each ID names a version (`-1.3`), and there are no dated snapshots. **UNCONFIRMED** whether the model behind an ID can change in place. No alias or pinning policy is documented.
- **[BIN]** IDs seen in the Muse Code catalog: `muse-spark-1.3-internal`, `muse-spark-1.2-internal`, `muse-image-1.0`. These are internal or subscription-only.
- **Served model ID in the response:** **UNCONFIRMED** whether it is always echoed, and whether it is the same as the requested ID.

### Pricing (per 1M tokens)
Source: https://dev.meta.ai/docs/pricing-rate-limits.md. The Spark entry price is confirmed by https://developer.meta.com/ai/resources/blog/build-with-muse-spark/

| Model | Mode | Input | Cached input | Output |
|---|---|---|---|---|
| Muse Wonder | Standard | $4.00 | $0.20 | $20.00 |
| Muse Spark | Standard | $1.25 | $0.15 | $4.25 |
| Muse Spark | Contributor | $0.10 | $0.002 | $0.20 |

- I took the column labels from the doc's table headers. The doc does not repeat per-row which Spark versions share these prices; it shows one table for "Muse Spark".

### Rate limits
Source: https://dev.meta.ai/docs/pricing-rate-limits.md
- `muse-spark-1.3`, `muse-spark-1.2`, `muse-spark-1.1` and `muse-wonder-1.0`: 3,000 RPM and 4M TPM each.
- The `-contributor` variants: 150 RPM and 3M TPM.
- Every response includes `x-ratelimit-limit-tokens`, `x-ratelimit-remaining-tokens`, `x-ratelimit-limit-requests` and `x-ratelimit-remaining-requests`.
- Background responses are limited to 600 submissions per minute per team. Over that, you get 429 with `Retry-After`.

### Errors
Source: https://dev.meta.ai/docs/error-handling.md

The error body is OpenAI-style: `{"error":{"message","type","param","code"}}`.

| HTTP | `type` / `code` |
|---|---|
| 400 | `invalid_request_error` (including context overflow) |
| 401 | `authentication_error` / `invalid_api_key` |
| 402 | `billing_error` / `billing_not_configured` |
| 403 | No permission for the model or feature |
| 404 | `model_not_found`, `file_not_found` |
| 413 | `payload_too_large` |
| 429 | `rate_limit_error` / `rate_limit_exceeded` |
| 500 | `server_error` |
| 503 | `service_overloaded`, `backend_unavailable`, `server_shutting_down` |
| 504 | `gateway_timeout` |

- Meta tells you to retry 429 and 5xx with exponential backoff plus jitter, honoring `Retry-After`.
- **[BIN]** Other codes in the CLI: `ai_spend_budget_exceeded`, and the subscription message "Muse Code requires a payment method".

---

## 2. xAI Grok: OAuth / SuperGrok sign-in

### OAuth server (official OIDC discovery, fetched live)
Source: https://auth.x.ai/.well-known/openid-configuration

| Field | Value |
|---|---|
| issuer | `https://auth.x.ai` |
| authorization_endpoint | `https://auth.x.ai/oauth2/authorize` |
| token_endpoint | `https://auth.x.ai/oauth2/token` |
| device_authorization_endpoint | `https://auth.x.ai/oauth2/device/code` |
| revocation_endpoint | `https://auth.x.ai/oauth2/revoke` |
| grant types | `authorization_code`, `refresh_token`, `urn:ietf:params:oauth:grant-type:device_code` |
| PKCE | `S256` only |
| token_endpoint_auth_methods | `client_secret_basic`, `client_secret_post`, `none` (public clients are allowed) |

Scopes listed: `openid profile email offline_access grok-cli:access grok-workspace:serve api:access grok-plugins:access team:read teams:read/write org:read orgs:read/write conversations:read/write workspaces:read/write api-keys:read/write logs:read billing:read/write`

### How the official Grok CLI uses it [BIN]
These come from strings in grok 1.0.70.
- `grok login` defaults to the browser flow: "Sign in through SpaceXAI OAuth at `auth.x.ai`".
- `grok login --device-auth` (alias `--device-code`) uses the device code flow. Also documented at https://docs.x.ai/build/cli/reference.md
- **Browser flow:**
  - Loopback redirect `http://127.0.0.1/callback` on a random ephemeral port (RFC 8252).
  - Query parameters: `response_type, client_id, state, code_challenge, code_challenge_method=S256, redirect_uri, scope`.
- **Default scopes:** the CLI's embedded docs show `["openid","profile","email","offline_access","api:access"]`, and say `offline_access` enables silent refresh. Note that this example sits in the enterprise-IdP section of those docs.
- **Client ID: UNCONFIRMED.**
  - It is not visible as a literal in the binary.
  - The CLI fetches a remote `/login-config` (string "Fetched remote login-config"). The client ID probably comes from there.
  - Overrides exist: `GROK_OAUTH2_CLIENT_ID` and `GROK_OIDC_CLIENT_ID`.
  - Third-party projects say xAI has **no public OAuth client for third-party apps** and that they reuse the Grok CLI client. That is unofficial. https://github.com/gm2211/skim/pull/210
  - OpenClaw says xAI's consent screen may show "Grok Build" because it uses a shared xAI OAuth client. https://docs.openclaw.ai/providers/xai
- **Tokens:** stored in `~/.grok/auth.json` and refreshed via `refresh_token` [BIN]. I did not read that file.
- **Request headers [BIN]:**
  - `Authorization: Bearer …`
  - `x-grok-client-version`
  - `x-grok-client-mode`
  - `x-grok-client-identifier`
  - `x-grok-agent-id`
  - `x-email`
  - Enterprise proxies can change the token header with `auth.token_header` (default `xai-grok-cli`).

### Which API base the subscription token works against
- **The SuperGrok/OAuth token goes to `https://cli-chat-proxy.grok.com/v1`, not `api.x.ai`.**
  - [BIN] Strings: `GROK_CLI_CHAT_PROXY_BASE_URL`, `https://cli-chat-proxy.grok.com/v1`, `/models-v2`.
  - Third-party confirmation: https://docs.openclaw.ai/providers/xai says to set `baseUrl` to `https://cli-chat-proxy.grok.com/v1` for a subscription token. https://pi.dev/packages/pi-xai-oauth says OAuth traffic goes to cli-chat-proxy and API-key traffic goes to api.x.ai.
  - Unauthenticated `GET https://cli-chat-proxy.grok.com/v1/models` returns 401 `{"error":"Invalid or expired credentials (auth_kind=none, x_xai_token_auth=none, …)"}`. I checked this live.
- The proxy speaks OpenAI Chat Completions and Responses. [BIN] Strings: `/v1/chat/completions`, `/v1/responses`, "Sending responses API stream request". The CLI also has an Anthropic Messages client.
- The per-account model catalog is `/models-v2` on the proxy. https://pi.dev/packages/pi-xai-oauth
- **SuperGrok quota and API credits are separate billing buckets.** https://docs.openclaw.ai/providers/xai
- xAI decides which accounts get OAuth API tokens. Some tiers get 403. https://docs.openclaw.ai/providers/xai and https://github.com/gm2211/skim/pull/210
- **Usage-limit signals [BIN]:**
  - status 402 "run out of credits"
  - status 403 "You hit your free usage limit"
  - "You hit your weekly limit"
  - "usage limit reached", "spending limit", "usage balance exhausted"
  - **UNCONFIRMED:** the exact JSON error shape.
- **Officially:** xAI's docs describe shared sign-in for Cursor and Grok, but no third-party OAuth program. https://docs.x.ai/x-subscription/sign-in.md
  - **UNCONFIRMED** whether xAI's terms allow third-party clients to use the CLI client ID.

### API-key path (api.x.ai)
- It is OpenAI-SDK compatible: `base_url="https://api.x.ai/v1"`, `Authorization: Bearer <XAI_API_KEY>`. https://docs.x.ai/developers/model-capabilities/legacy/chat-completions.md
- Chat Completions is now under "legacy". The Responses API is primary: https://docs.x.ai/developers/rest-api-reference/inference/responses.md
- Streaming is OpenAI-style SSE (`data: {...}` … `data: [DONE]`). https://docs.x.ai/developers/model-capabilities/text/streaming.md
- Function calling: https://docs.x.ai/developers/tools/function-calling.md
- Encrypted reasoning: `include: ["reasoning.encrypted_content"]` works on Responses. `grok-4.7` **always** returns it. https://docs.x.ai/developers/model-capabilities/text/reasoning.md
- Errors are listed at https://docs.x.ai/developers/debugging.md: 400, 401, 403, 404, 405, 415, 422, and 429 (rate limit), plus 202 for deferred completions.
- Rate limits are per model, in RPS (RPM/48, minimum 2) and TPM, and scale with spend tier. Exceeding them returns 429. https://docs.x.ai/developers/rate-limits.md
- **Alias policy:**
  - `<model>` is the latest stable version.
  - `<model>-latest` is the latest version.
  - `<model>-<date>` is fixed.
  - Source: https://docs.x.ai/developers/models.md
- **Dated or fixed IDs currently listed:** `grok-4.20-0309-reasoning`, `grok-4.20-0309-non-reasoning`, `grok-4.20-multi-agent-0309`. The newer models are version-numbered: `grok-4.7`, `grok-4.6`, `grok-4.5`, `grok-4.3`, `grok-build-0.1`. https://docs.x.ai/developers/models.md
- **Pricing per 1M tokens, for prompts under 200k tokens (input / cached / output). The price doubles at 200k or more.** Source: https://docs.x.ai/developers/models.md

| Model | Context | Input | Cached | Output |
|---|---|---|---|---|
| `grok-4.7` | 500k | $2 | $0.50 | $6 |
| `grok-4.6` | 500k | $2 | $0.50 | $6 |
| `grok-4.5` | 500k | $2 | $0.30 | $6 |
| `grok-4.3` | 1M | $1.25 | $0.20 | $2.50 |
| `grok-4.20-0309-*` | 1M | $1.25 | $0.20 | $2.50 |
| `grok-build-0.1` | 256k | $1 | $0.20 | $2 |

- The Grok CLI defaults to `grok-4.5` [BIN]. Its embedded examples also use `grok-4.6`.
- Branding: both the docs and the binary say "SpaceXAI" or "SpaceXSI" where older material says "xAI".

---

## 3. OpenAI Codex with ChatGPT sign-in (codex-rs, commit eec8bde1)

### OAuth (authorization code + PKCE)
- **Issuer:** `https://auth.openai.com`. [SRC] `codex-rs/login/src/server.rs:77`
- **Authorize URL:** `{issuer}/oauth/authorize`. **Token URL:** `{issuer}/oauth/token`. [SRC] `server.rs:595`, `:708`
- **client_id:** `app_EMoamEEZ73f0CkXaXp7hrann`. [SRC] `login/src/auth/manager.rs:1789`. You can override it with the env var `CODEX_APP_SERVER_LOGIN_CLIENT_ID` (`manager.rs:231`).
- **Redirect:** `http://127.0.0.1:{port}/auth/callback`. The default port is **1455**. A comment there says to keep it in sync with the "Codex CLI Hydra redirect URI allow-list". [SRC] `server.rs:78`, `:194`
- **Scope:** `openid profile email offline_access api.connectors.read api.connectors.invoke`. [SRC] `server.rs:598`
- **Extra authorize parameters:** [SRC] `server.rs:587-590`
  - `id_token_add_organizations=true`
  - `codex_cli_simplified_flow=true`
  - `originator=<originator>` (default `codex_cli_rs`, from `login/src/auth/default_client.rs:42`)
  - optional `allowed_workspace_id`
- **PKCE:** S256. The verifier is base64url-encoded random bytes; the challenge is base64url(SHA256(verifier)) without padding. [SRC] `login/src/oauth/pkce.rs`, `oauth/authorization.rs:31`
- **Token exchange:** `grant_type=authorization_code` plus `code_verifier`. [SRC] `oauth/client.rs:88`
- **Optional API key:** after login, the CLI can exchange the `id_token` for an API key using RFC 8693 token exchange (`grant_type=urn:ietf:params:oauth:grant-type:token-exchange`, `requested_token=openai-api-key`). [SRC] `server.rs:1034-1037`
- **Refresh:**
  - Request: `POST https://auth.openai.com/oauth/token` with a JSON-encoded refresh grant (`TokenEncoding::Json`). Override with `CODEX_REFRESH_TOKEN_URL_OVERRIDE`.
  - The client refreshes proactively when the access token is within **5 minutes** of expiry, or when the last refresh is more than **8 days** old.
  - It treats `invalid_grant` or 401 as permanent failure.
  - [SRC] `manager.rs:218-229`, `:1700-1740`, `:3340-3344`
- **Device code flow (headless):** [SRC] `login/src/device_code_auth.rs:68-206`
  - `POST {base}/api/accounts/deviceauth/usercode`
  - The user opens `{base}/codex/device`.
  - Poll `POST …/deviceauth/token` with `{device_auth_id, user_code}`.
  - That exchange uses redirect `{base}/deviceauth/callback`.
- **Account ID:** comes from the id_token claim `https://api.openai.com/auth` → `chatgpt_account_id`. [SRC] `login/src/token_data.rs`, `app-server/src/user_verification_test_support.rs:276`

### Requests made with a ChatGPT token
- **Base URL:** `https://chatgpt.com/backend-api/codex`. This is the default whenever the auth mode is ChatGPT. [SRC] `model-provider-info/src/lib.rs:85`, `:458-470`
  - Endpoint: **`POST /responses`**, which gives `https://chatgpt.com/backend-api/codex/responses`. [SRC] `codex-api/src/endpoint/responses.rs:67`
  - A WebSocket variant exists, gated by `OpenAI-Beta: responses_websockets=2026-02-06`. [SRC] `core/src/client.rs:168`, `:183`
  - Models list: `GET {base}/models?client_version=…`, which returns ETag `x-models-etag`. [SRC] `model-provider/src/models_endpoint.rs:48`
  - Chat wire API: `wire_api="chat"` has been **removed**. Only `responses` is supported. [SRC] `model-provider-info/src/lib.rs:108`
- **Headers:** [SRC] `model-provider/src/auth.rs:101-113`, `http-client/src/openai_headers.rs:3`, `login/src/auth/default_client.rs:133`
  - `Authorization: Bearer <access_token>`
  - `chatgpt-account-id: <account_id>`
  - `X-OpenAI-Fedramp: true` for FedRAMP accounts only
  - `originator: codex_cli_rs`
  - `User-Agent`
  - Optional ones seen: `x-client-request-id` (the thread ID), `x-codex-turn-state`, `x-codex-window-id`, `x-codex-installation-id`, `x-openai-subagent`, `x-openai-account-routing-override`
- **Request body:** [SRC] `codex-api/src/common.rs:282-301`, `core/src/client.rs:1130-1174`
  - Fields: `model, instructions, input[], tools, tool_choice:"auto", parallel_tool_calls, reasoning{effort,summary}, store:false, stream:true, include:["reasoning.encrypted_content"]`, plus `output.encrypted_content` behind a flag.
  - Also: `prompt_cache_key, text{verbosity, format}, service_tier, client_metadata`.
- **Model IDs in the bundled catalog:** [SRC] `codex-rs/models-manager/models.json`
  - Listed: `gpt-6.1-sol`, `gpt-6-astra`, `gpt-6-sol`, `gpt-6-luna`, `gpt-5.6-sol`, `gpt-6.1-terra` (500k context), `gpt-5.6-terra`, `gpt-5.6-luna`, `gpt-5.5`.
  - Hidden: `gpt-daybreak-blue-latest`, `gpt-daybreak-red-latest`, `codex-auto-review`.
  - Context is 272k unless noted. Reasoning levels are `low|medium|high|xhigh|max|ultra`.
  - The live list comes from `/models`. These IDs are not date-pinned.
- **Usage-limit errors:** [SRC] `codex-api/src/api_bridge.rs:190-230`, `codex-api/src/rate_limits.rs`, `codex-api/src/sse/responses_error.rs:47-56`
  - HTTP **429**. The body is `{"error":{"type":"usage_limit_reached","plan_type":…,"resets_at":<unix seconds>,"limit_window_minutes":…}}`.
  - Rate-limit headers:
    - `x-codex-primary-used-percent`, `x-codex-primary-window-minutes`, `x-codex-primary-reset-at`
    - `x-codex-secondary-*`
    - `x-codex-active-limit`, `x-codex-rate-limit-reached-type`, `x-codex-promo-message`
    - `x-codex-credits-has-credits`, `x-codex-credits-unlimited`, `x-codex-credits-balance`
  - Other 429 types: `usage_not_included`, `insufficient_quota`.
  - Mid-stream `response.failed` error codes:
    - `context_length_exceeded`
    - `insufficient_quota`, `credit_balance_exhausted`, `organization_spend_limit_exceeded`, `project_spend_limit_exceeded`
    - `usage_not_included`
    - `cyber_policy`, `bio_policy`, `misalignment_policy_violation`
    - `server_is_overloaded`
- **ToS caveat:** **UNCONFIRMED** whether OpenAI allows clients other than its own to use this client ID and backend.

---

## 4. GLM (Z.ai international, and BigModel in China)

### Base URLs
- **Z.ai pay-as-you-go:** `https://api.z.ai/api/paas/v4/`. Chat endpoint: `POST /paas/v4/chat/completions`. https://docs.z.ai/api-reference/llm/chat-completion.md, https://docs.z.ai/guides/overview/quick-start.md
- **Z.ai GLM Coding Plan:** source https://docs.z.ai/devpack/tool/others.md and https://docs.z.ai/guides/llm/glm-5.3.md
  - OpenAI Chat: `https://api.z.ai/api/coding/paas/v4`
  - OpenAI Responses (Codex): `https://api.z.ai/api/v1`
  - **Anthropic Messages:** `https://api.z.ai/api/anthropic`
- **BigModel (China):**
  - `https://open.bigmodel.cn/api/paas/v4` (chat at `/chat/completions`)
  - Anthropic-compatible: `https://open.bigmodel.cn/api/anthropic`, which gives `/v1/messages`
  - Sources: https://docs.bigmodel.cn/cn/guide/develop/claude/introduction.md, https://docs.bigmodel.cn/llms.txt
- Auth is `Authorization: Bearer <key>`. Errors 1000/1001 refer to the auth header. https://docs.z.ai/api-reference/api-code.md

### Features
- **Streaming:** OpenAI-style SSE (`data: {...}` … `data: [DONE]`). Reasoning arrives in `choices[0].delta.reasoning_content`. The final chunk carries `usage.prompt_tokens_details.cached_tokens`. https://docs.z.ai/guides/capabilities/streaming.md
- **Tool calling:** OpenAI `tools` / `tool_calls`. `tool_choice` supports **`auto` only**. https://docs.z.ai/guides/capabilities/function-calling.md
- **Streamed tool arguments:** `tool_stream` parameter. https://docs.z.ai/guides/capabilities/stream-tool.md
- **Other Z.ai-specific parameters:** `do_sample`, `request_id`, and an `Accept-Language` header. https://docs.z.ai/api-reference/llm/chat-completion.md

### Models
- Current IDs, lowercase: `glm-5.3`, `glm-5.3-flash`, `glm-5.3-flashx`, `glm-5.2`, `glm-5.1`, `glm-5`, `glm-4.7`, `glm-4.6`.
  - Pricing page with display names: https://docs.z.ai/guides/overview/pricing.md
  - ID usage: https://docs.z.ai/guides/llm/glm-5.3.md
- `glm-5.3` is text-only, with **1M context** and **128K max output**. Reasoning cannot be turned off: `thinking.type:"disabled"` fails, so use `enabled` with `reasoning_effort:"low"`. https://docs.z.ai/guides/llm/glm-5.3.md
- There are no dated snapshot IDs. **UNCONFIRMED** whether a version ID is immutable.

### Pricing
Z.ai, USD per 1M tokens (input / cached input / output). Cache storage is "limited-time free". Source: https://docs.z.ai/guides/overview/pricing.md

| Model | Input | Cached input | Output |
|---|---|---|---|
| GLM-5.3 | $1.4 | $0.26 | $4.4 |
| GLM-5.3-Flash | $0.15 | $0.03 | $0.50 |
| GLM-5.3-FlashX | $0.37 | $0.075 | $1.25 |
| GLM-5.2 / 5.1 | $1.4 | $0.26 | $4.4 |
| GLM-5 | $1 | $0.2 | $3.2 |
| GLM-4.7 / 4.6 | $0.6 | $0.11 | $2.2 |

BigModel, CNY per 1M tokens. Source: https://docs.bigmodel.cn/cn/guide/start/pricing.md

| Model | Input | Output | Cache hit |
|---|---|---|---|
| GLM-5.3 | ¥8 | ¥28 | ¥2 |
| GLM-5 (input under 32K) | ¥4 | ¥18 | not recorded |

### Errors
Source: https://docs.z.ai/api-reference/api-code.md

The body is `{"error":{"code":"<business code as string>","message":"…"}}`, sent alongside the HTTP status.

**Authentication (401):**
- 1000 auth failed
- 1001 no auth header
- 1003 token expired

**Bad request (400):**
- 1210–1215 parameter errors
- 1211 unknown model
- 1261 prompt too long
- 1301 unsafe content

**Rate, usage and quota (all HTTP 429):**
- 1113 insufficient balance or no resource package
- 1302 request rate limit reached
- 1305 temporarily overloaded
- 1308 usage limit reached, resets at `{next_flush_time}`
- 1309 Coding Plan expired
- 1310 weekly or monthly limit exhausted
- 1311 plan does not include this model
- 1313 fair-use throttling
- 1314 enterprise package expired
- 1315 key is restricted to the enterprise coding package
- 1316 / 1318 / 1320 five-hour window exhausted (variants with and without extra usage)
- 1317 / 1319 / 1321 seven-day window exhausted (same variants)

**Server (500):** 1200, 1230, 1234.

---

## 5. Anthropic Messages API

Sources are under https://platform.claude.com/docs/en/…

- **`anthropic-version`:** the current version is **`2026-10-21`**. `2023-06-01` is still listed. https://platform.claude.com/docs/en/api/versioning
  - **Breaking change in 2026-10-21:** `temperature`, `top_p` and `top_k` are **removed** and return 400 on every model. A `document` block in an assistant message, and user `text` blocks with non-empty `citations`, also return 400.
  - Migration guide: https://platform.claude.com/docs/en/api/migration-2026-10-21
- **Streaming events:** `message_start`, `content_block_start`, `content_block_delta` (delta types `text_delta`, `input_json_delta`, `thinking_delta`, `signature_delta`), `content_block_stop`, `message_delta`, `message_stop`, `ping`, and an in-stream `error` event (for example `{"type":"error","error":{"type":"overloaded_error",…}}`). https://platform.claude.com/docs/en/build-with-claude/streaming
- **Tool calls:** a response with `stop_reason: "tool_use"` contains `{"type":"tool_use","id","name","input"}` blocks.
  - Reply with a user message of `{"type":"tool_result","tool_use_id","content","is_error"?}` blocks.
  - Tool results go first in that user message.
  - https://platform.claude.com/docs/en/agents-and-tools/tool-use/handle-tool-calls
- **Errors:** `{"type":"error","error":{"type","message"},"request_id"}`. https://platform.claude.com/docs/en/api/errors

| HTTP | Error type |
|---|---|
| 400 | `invalid_request_error` (also returned when an org or workspace usage limit is reached) |
| 401 | `authentication_error` |
| 402 | `billing_error` |
| 403 | `permission_error` |
| 404 | `not_found_error` |
| 409 | `conflict_error` |
| 413 | `request_too_large` |
| 429 | `rate_limit_error` (also spend caps; a tier spend-cap 429 has no `retry-after`) |
| 500 | `api_error` |
| 504 | `timeout_error` |
| 529 | `overloaded_error` |

- **5.5-generation restrictions** (https://platform.claude.com/docs/en/api/errors):
  - Opus 5.5, Sonnet 5.5 and Fable 5.1 reject forced tool use (`tool_choice` `any` or `tool`) with 400.
  - Sonnet 5.5 rejects `thinking:{type:"disabled"}`. Use `between_tools` instead.
  - Prefill is rejected on Claude 4.6 and later.
- **Served model ID:** the response's top-level **`model` field** names the model that served it. With server-side fallback (beta `server-side-fallback-2026-07-01`, `fallbacks:"default"`), you check `model` plus a `fallback_message` entry in `usage.iterations`. https://platform.claude.com/docs/en/build-with-claude/refusals-and-fallback
  - The `message_start` stream event also carries `model`. https://platform.claude.com/docs/en/build-with-claude/streaming
- **Model IDs** (https://platform.claude.com/docs/en/models/overview): `claude-opus-5-5`, `claude-sonnet-5-5`, `claude-haiku-5-5`, `claude-fable-5-1`.
  - The ID and the alias are the same string; there is no dated suffix.
  - All four have a 1M context window and 128K max output.
  - Default effort: Opus 5.5 `medium`, Sonnet 5.5 `high`, Haiku 5.5 `medium`.
  - Retirement no sooner than: Opus 2027-09-22, Sonnet 2027-09-28, Haiku 2027-10-07.
- **Pricing per MTok** (base input / 5m cache write / 1h cache write / cache hit / output). Source: https://platform.claude.com/docs/en/about-claude/pricing

| Model | Base input | 5m cache write | 1h cache write | Cache hit | Output |
|---|---|---|---|---|---|
| Opus 5.5 | $4 | $5 | $8 | $0.20 (0.05×) | $20 |
| Sonnet 5.5 | $2 | $2.50 | $4 | $0.10 (0.05×) | $10 |
| Haiku 5.5 (prompt ≤100k) | $0.10 | $0.125 | $0.20 | $0.01 | $0.50 |
| Haiku 5.5 (prompt >100k) | $0.50 | $0.625 | $1 | $0.05 | $2.50 |
| Fable 5.1 | $10 | $12.50 | $20 | $0.25 | $50 |

- Opus 5.5 fast mode costs $8 input / $40 output.
- Batch processing is half price: Opus 5.5 $2/$10, Sonnet 5.5 $1/$5.

---

## 6. TheAgentCompany (TAC)

### Server stack
- **Setup** (https://github.com/TheAgentCompany/TheAgentCompany/blob/main/docs/SETUP.md):
  - `sudo chmod 666 /var/run/docker.sock`
  - then `curl -fsSL https://github.com/TheAgentCompany/the-agent-company-backup-data/releases/download/setup-script-20241208/setup.sh | sh`
  - Needs `docker compose` v2, curl, and **30+ GB free disk**. Mac and Windows need Docker Desktop's "Enable host networking".
  - The baselines ran on an **AWS EC2 t3.2xlarge**.
  - **RAM: not stated in the TAC docs.** A t3.2xlarge has 8 vCPU and 32 GiB, according to AWS's own spec (that figure is not in TAC's docs).
  - The first startup can take up to about 30 minutes.
- **What `setup.sh` does** (`servers/setup.sh`):
  - Pulls these images:
    - `ghcr.io/theagentcompany/servers-api-server:1.0.0`
    - `servers-rocketchat-npc-data-population`, `servers-owncloud`, `servers-gitlab`
    - `servers-plane-{admin,frontend,backend,space,proxy}`
    - `minio`, `collabora/code:24.04.9.2.1`, `valkey`, `redis-stack-server:7.4.0-v0`, `postgres:15.7-alpine`, `bitnamilegacy/mongodb:5.0`, `rocket.chat:5.3.0`
  - Runs the **api-server** container with `--network host`, with the Docker socket mounted. The api-server launches every other service.
  - Waits for `localhost:2999`, then polls `/api/healthcheck/{rocketchat,owncloud,gitlab,plane}`.
- **Services** (`servers/README.md`): all are reached through the synthetic hostname `the-agent-company.com`.

| Service | Port | Credentials |
|---|---|---|
| GitLab | 8929 | `root` / `theagentcompany` |
| ownCloud | 8092 | `theagentcompany` / `theagentcompany` |
| Plane | 8091 | `agent@company.com` / `theagentcompany` (API key is in the README) |
| RocketChat | 3000 | `theagentcompany` / `theagentcompany` |
| API server | 2999 | — |
| Collabora | 9980 | — |
| Redis (NPCs) | 6379 | — |

### Tasks
- **Count:** **175 tasks**, one Docker image each: `ghcr.io/theagentcompany/<task>-image:1.0.0`. Sources: `docs/EVALUATION.md`, `evaluation/run_eval.sh:109`, and leaderboard `number_of_tasks: 175`.
- **The `example` task is part of the 175.** Directory counts in `workspaces/tasks`:

| Prefix | Tasks |
|---|---|
| `sde` | 69 |
| `hr` | 29 |
| `pm` | 28 |
| `admin` | 15 |
| `ds` | 14 |
| `finance` | 12 |
| `ml` | 2 |
| `qa` | 2 |
| `research` | 2 |
| `bm` | 1 |
| `example` | 1 |

- `summarise_results.py` reports six categories (sde, pm, ds, admin, hr, finance). Everything else is "other".
- **Service dependencies** (from each task's `dependencies.yml`): rocketchat 79, gitlab 71, owncloud 70, plane 17, none 3.
- **NPC tasks:** 41 tasks ship a `scenarios.json`, which defines LLM-backed coworkers in RocketChat (sotopia, with Redis at `the-agent-company.com:6379`).

### How one task runs (`docs/EVALUATION.md`, `workspaces/base_image/*`)
1. Start the task image: `docker run --name X --network host -it <image> /bin/bash`. Use `--network host` only when the servers run on the same host.
2. Inside the container, run:
   `SERVER_HOSTNAME=… LITELLM_API_KEY=… LITELLM_BASE_URL=… LITELLM_MODEL=… bash /utils/init.sh`
   - This appends `<server-ip> the-agent-company.com` to `/etc/hosts`.
   - It runs `/utils/reset.sh`, which does `POST http://the-agent-company.com:2999/api/reset-<service>` and waits up to 15 minutes for health checks.
   - Then it runs `pre_init`, the NPCs (`/npc/run_multi_npc.py`), `populate_data.py` and `post_init` if present.
   - It can take up to 10 minutes.
3. The agent gets the prompt **"Complete the task in /instruction/task.md"**. This is the baseline prompt. OpenHands also adds the GitLab `root`/`theagentcompany` credentials when the task depends on GitLab (`evaluation/run_eval.py:129-132`).
   - The agent does **not** have to run inside the container. The docs allow an external browser.
   - However, the graders inspect state **inside the container** (for example `/workspace` files and the local `/utils`), as well as server state. Files the task produces must end up in the container. The docs suggest mounting.
   - Service logins can be done any way you like. The baseline pre-logs in with Playwright (`evaluation/browsing.py`).
4. Grade with:
   `LITELLM_*=… DECRYPTION_KEY='theagentcompany is all you need' python_default /utils/eval.py --trajectory_path <abs path> --result_path <out.json>`
   - `/utils/evaluator.py` is Fernet-encrypted at image build time (`ONBUILD … encrypt.py`). `eval.py` decrypts it and calls `grade_checkpoints(trajectory)`.
   - **Doc vs code mismatch:** `EVALUATION.md` says `--output_path`, but `eval.py` only accepts **`--result_path`** (default `./result.json`). `run_eval.py` uses `--result_path`.
   - The trajectory file is optional but is used for partial credit. Its format is free-form, but it must record every step.

### Scoring (`workspaces/base_image/scoring.py`, `evaluation/summarise_results.py:162-178`)
- Each task has checkpoints `(total, result)`. A task's `final_score` is the sum of its checkpoints, optionally passed through a bonus strategy.
- **Bonus strategies:**
  - `bonus_for_completing_final` (42 tasks): if the last checkpoint is fully met, the task gets full points.
  - `bonus_for_completing_any` (14 tasks): if any checkpoint is fully met, checkpoint 1 is credited.
  - `bonus_for_completing_any_of_given_checkpoints([..])` (2 tasks).
  - `bonus_for_completing_last_two` (1 task).
  - All other tasks use the plain sum.
- **Per-task score:** `S = 0.5 * result/total + 0.5 * floor(result/total)`. Full completion gives 1.0; partial credit is capped below 0.5.
- **Benchmark score:** the mean of S over all tasks, times 100.
- **"Resolved"** is the percentage of tasks fully completed. The leaderboard reports both.

### LLM graders and environment LLM
- 53 task evaluators call `evaluate_with_llm`, `evaluate_chat_history_with_llm` or `llm_complete`. Counted by grep over `workspaces/tasks/*/evaluator.py`.
- These calls go through **LiteLLM 1.23.16**, using `LITELLM_API_KEY`, `LITELLM_BASE_URL` (default `https://api.openai.com/v1`) and `LITELLM_MODEL` (default `gpt-4o`). Source: `workspaces/base_image/config.py`, `common.py:52-59`, `Dockerfile`.
- The same "environment LLM" also drives the NPCs.
- The docs ask for a model comparable to `claude-3-5-sonnet-20241022` or `gpt-4o`, and ask you to report which one you used. The baselines used claude-3-5-sonnet-20241022.

### OpenHands integration (`evaluation/README.md`, `run_eval.py`, `pyproject.toml`)
- Pins `openhands-ai = 0.42.0`, run through Poetry with Python 3.12.
- `config.toml` holds the `[llm.<name>]` groups. You pick them with `--agent-llm-config` and `--env-llm-config`.
- Each task image becomes the sandbox `base_container_image`, and OpenHands builds a runtime image on top of it, one per task. `build_oh_runtime_images.sh` pre-builds them.
- Settings: `max_iterations=100`, and `/outputs` is mounted to collect trajectories.
- The steps run as `CmdRunAction` inside the runtime container: `init.sh`, then the solver, then `eval.py`.
- `summarise_results.py` builds the report.

### Leaderboard
Source: `theagentcompany.github.io/src/data/leaderboard.json`, last commit 2026-07-31, shown at https://the-agent-company.com/

| Score | Resolved | Date | Agent | Env LLM |
|---|---|---|---|---|
| **52.40** | **42.86** | 2025-11-10 | TTE-MatrixAgent + DeepSeek-V3.2 | Qwen_Plus |
| 51.78 | 41.14 | 2025-10-13 | MUSE + Gemini 2.5 Flash (an agent framework, not Meta Muse) | GPT-4o |
| 43.19 | 33.14 | 2025-06-14 | OpenHands-Versa + Claude Sonnet 4 | Claude 3.5 Sonnet |
| 40.18 | 30.86 | 2025-06-14 | OpenHands-Versa + Claude 3.7 Sonnet | Claude 3.5 Sonnet |
| 39.28 | 30.29 | 2025-05-10 | OpenHands + Gemini 2.5 Pro | Claude 3.5 Sonnet |
| 36.41 | 26.29 | 2025-05-10 | OpenHands + Claude 3.7 Sonnet | Claude 3.5 Sonnet |
| 34.4 | 24.0 | 2024-12-17 | OpenHands 0.14.2 + Claude 3.5 Sonnet (paper baseline) | Claude 3.5 Sonnet |

- An IRIS-GraphAgent (gpt-5.4) entry was added in June 2026 and **reverted on 2026-08-01**. Sources: theagentcompany.github.io PRs #7 and #9, experiments PRs #15 and #16, via the GitHub API.
- Third-party aggregators disagree. For example, https://benchmarklist.com/benchmarks/the_agent_company/ lists a different leader. Do not use them.
- Trajectories for every entry are at https://github.com/TheAgentCompany/experiments/tree/main/evaluation/1.0.0
- **Paper:** arXiv 2412.14161 v3 (2025-09-10). It describes 175 tasks; the best baseline completes about 30% of tasks autonomously. https://arxiv.org/abs/2412.14161

### Container vs network
- Services are reached over HTTP through `the-agent-company.com`, which `init.sh` adds to `/etc/hosts`.
- Init, reset and grading **must run inside the task container**, because they use `/utils`, `/npc`, `/instruction` and `/workspace`.
- The agent's actions can come from anywhere, but outputs the grader checks locally must end up in the container (for example under `/workspace`).
- The same task container must also reach the LLM endpoint for LLM-graded tasks and NPCs.

---

## Biggest uncertainties
1. **xAI OAuth client ID** and whether third parties may use it at all. No public program is documented. The live discovery document confirms the endpoints. Subscription tokens only work against `cli-chat-proxy.grok.com`. Exact usage-limit JSON is unknown.
2. **Meta subscription OAuth.** The CLI uses a device flow at `auth.meta.com` with dynamic registration, then mints an API key. This is inferred from the binary only and is not documented for third parties. The API-key path is fully documented.
3. **Codex ChatGPT backend** is an internal, undocumented surface. The client ID, headers and body come from source and may change. ToS is unconfirmed.
4. **Fixed versions.** Meta, Z.ai and Anthropic 5.5 use versioned names with no date snapshots. xAI documents `-<date>` pins only for grok-4.20.
5. **TAC RAM** is not specified (the stated baseline is t3.2xlarge). The `--output_path` vs `--result_path` docs bug is real.
