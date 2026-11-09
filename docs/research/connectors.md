# Connectors: GitHub, Google Workspace, Microsoft 365, Files (research, 2026-10-28)

How to read this:
- Every claim has a source next to it. **[PROBE]** means I checked it myself today with an unauthenticated `curl` against the live endpoint. **[SRC]** means I read it in the project's repo on its `main` branch today.
- **UNCONFIRMED** means I found no primary source, or it needs a live sign-in to check.
- Versions below are only today's snapshot. The install step should read them from the URL given next to each one; none of them should be written into Magnus.

Where Magnus stands today ([SRC] `crates/magnus/src/apps.rs`, `crates/magnus/src/config.rs`):
- MCP over streamable HTTP only.
- It sends one `initialize`, then `tools/list` or `tools/call`, with protocol `2025-06-18`.
- It sends an optional `Authorization: Bearer` header taken from the vault.
- `[[connectors]]` has `transport` (default `mcp_http`), `url` and `credential`. Nothing refreshes a token yet.

What the MCP spec says now:
- The latest protocol is `2026-07-28` (`LATEST_PROTOCOL_VERSION`, https://github.com/modelcontextprotocol/modelcontextprotocol/blob/main/schema/2026-07-28/schema.ts).
- It has no `initialize` handshake. Every request carries its own version, and clients detect older servers by probing (https://modelcontextprotocol.io/specification/2026-07-28/basic/versioning).
- Servers may still answer older clients that send `initialize` (same page). The GitHub server says it still accepts older protocol versions without the new `Mcp-*` headers (https://github.com/github/github-mcp-server/blob/main/docs/streamable-http.md).
- So Magnus's `2025-06-18` client still works. It is one version behind.

---

## Summary

| Connector | Server to use | Transport | Sign-in that needs no browser on the appliance | Main blocker |
|---|---|---|---|---|
| GitHub | Official: `github/github-mcp-server`, hosted at `https://api.githubcopilot.com/mcp/` | Remote streamable HTTP. No change to Magnus. | **Device flow** (RFC 8628) on a GitHub App the owner registers | None found |
| Google Workspace | Official: Google's remote servers `gmailmcp` / `calendarmcp` / `drivemcp` `.googleapis.com/mcp/v1`. They are **Developer Preview**. | Remote streamable HTTP | **Device flow is not possible**: Google does not allow Gmail or Calendar scopes on it. Use the **desktop-app loopback + PKCE** flow in the Magnus desktop app. | Preview terms, and Google's limits on testing-mode and restricted scopes |
| Microsoft 365 | Official: **Work IQ MCP** at `https://workiq.svc.cloud.microsoft/mcp`. Work and school accounts only, billed in Copilot Credits. For other cases: community `softeria/ms-365-mcp-server`. | Remote HTTP (Work IQ), or a local Node process in `--http` mode (softeria) | **Device code** on an Entra public client the owner registers | Work IQ excludes personal accounts. softeria is a community server. |
| Files | Reference `@modelcontextprotocol/server-filesystem` | **stdio only** | None needed | Magnus has no stdio transport |

---

## 1. GitHub

### Server
- **Official:** `github/github-mcp-server`. MIT licence, last push 2026-10-28, latest release `v2.0.3` (2026-10-21). Read the version at install time from https://api.github.com/repos/github/github-mcp-server/releases/latest.
- **Release assets:** Linux x86_64/arm64 tarballs plus a checksums file (same URL). The image is `ghcr.io/github/github-mcp-server` (https://github.com/github/github-mcp-server#readme).
- **Hosted remote:** `https://api.githubcopilot.com/mcp/`. It is GA and open to all GitHub users. Paid tools need the matching licence (https://github.blog/changelog/2025-09-04-remote-github-mcp-server-is-now-generally-available/).

### Transport
- **Remote:** streamable HTTP. This fits Magnus's current `mcp_http` with no change.
- **Path variants:** `/readonly`, `/x/{toolset}`, `/insiders`. **Headers:** `X-MCP-Toolsets`, `X-MCP-Tools`, `X-MCP-Readonly`, `X-MCP-Lockdown` (https://github.com/github/github-mcp-server/blob/main/docs/remote-server.md).
- **Self-hosted option:** the same binary has an `http` subcommand. It listens on port 8082 by default and can serve OAuth protected-resource metadata ([SRC] `cmd/github-mcp-server/main.go`; https://github.com/github/github-mcp-server/blob/main/docs/streamable-http.md). Use it if the owner does not want to depend on `api.githubcopilot.com`.

### Auth
- **[PROBE]** An unauthenticated POST gets `401` with `resource_metadata=https://api.githubcopilot.com/.well-known/oauth-protected-resource/mcp/`.
  - That metadata names `authorization_servers: ["https://github.com/login/oauth"]`.
  - Its scopes are `repo, read:org, read:user, user:email, read:packages, write:packages, read:project, project, gist, notifications`.
- GitHub's guide for MCP host authors says: "you may also supply any valid access token". It also says the remote server does **not** support dynamic client registration, and that each host must register its own GitHub App or OAuth App (https://github.com/github/github-mcp-server/blob/main/docs/host-integration.md).
- **Does it take a device-flow token from an OAuth App or GitHub App? Yes, as far as documents and the probe show.**
  - The authorization server is GitHub's own OAuth (probe above).
  - Device-flow tokens are ordinary user tokens (https://docs.github.com/en/apps/oauth-apps/building-oauth-apps/authorizing-oauth-apps#device-flow).
  - GitHub's governance doc lists both OAuth Apps and GitHub Apps as accepted on the remote server (https://github.com/github/github-mcp-server/blob/main/docs/policies-and-governance.md).
  - **UNCONFIRMED by a live call:** how the remote server filters tools for a GitHub App token, which has fine-grained permissions rather than OAuth scopes. Tool filtering by OAuth scope is documented (https://github.com/github/github-mcp-server/blob/main/docs/oauth-login.md#scope-filtering). Test this with one sign-in.
- **Device flow details** (https://docs.github.com/en/apps/oauth-apps/building-oauth-apps/authorizing-oauth-apps#device-flow):
  - It must be enabled in the app's settings.
  - It needs only the client ID, no secret.
  - The poll `interval`, `slow_down` and expiry come from the responses.
- **The server's own device-code fallback:** the local server can run GitHub's device flow when it has no browser (https://github.com/github/github-mcp-server/blob/main/docs/oauth-login.md#headless-and-device-code-fallback). Magnus should run the flow itself instead, so the token lands in the vault and the boundary keeps control of it.

### What the owner registers
Recommended: a **GitHub App**. GitHub's host guide marks it RECOMMENDED (https://github.com/github/github-mcp-server/blob/main/docs/host-integration.md).
1. Open Settings → Developer settings → GitHub Apps → New GitHub App (https://docs.github.com/en/apps/creating-github-apps/registering-a-github-app/registering-a-github-app).
2. Enter a name and a homepage URL.
3. Tick **Enable Device Flow**.
4. Tick **Expire user authorization tokens**.
5. Turn the webhook off.
6. Choose repository permissions: Metadata (read), Contents, Issues, Pull requests, and others as needed.
7. Note the **Client ID**. No client secret is needed for device flow or for refreshing a device-flow token (https://docs.github.com/en/apps/creating-github-apps/authenticating-with-a-github-app/refreshing-user-access-tokens).
8. **Install** the app on the person's account and on each organisation it should reach.
   - The token reaches only accounts where the app is installed, only resources the user can access, and only what the app's permissions allow (https://docs.github.com/en/apps/creating-github-apps/authenticating-with-a-github-app/authenticating-with-a-github-app-on-behalf-of-a-user).
   - Organisations may need an admin to approve the install (same host guide).

Alternative: an **OAuth App** (https://docs.github.com/en/apps/oauth-apps/building-oauth-apps/creating-an-oauth-app). Register it, enable device flow, and request scopes from the list above.
- It is simpler: there is nothing to install.
- Its tokens are broader.
- An organisation's OAuth-app access restrictions can block it (https://docs.github.com/organizations/managing-oauth-access-to-your-organizations-data/about-oauth-app-access-restrictions).

### Refresh
- **GitHub App with expiring tokens** (https://docs.github.com/en/apps/creating-github-apps/authenticating-with-a-github-app/refreshing-user-access-tokens):
  - The access token (`ghu_`) lasts 8 hours (`expires_in` = 28800).
  - The refresh token (`ghr_`) lasts 6 months.
  - Refresh with `grant_type=refresh_token`; no secret is needed for a device-flow token.
  - Each refresh rotates both tokens; the old ones stop working.
- **OAuth App:** tokens do not expire. GitHub revokes a token after one year unused (https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/token-expiration-and-revocation).

---

## 2. Google Workspace (Gmail, Calendar, Drive)

### Server
- **Official:** Google's remote Workspace MCP servers. Each product has its own endpoint (https://developers.google.com/workspace/guides/configure-mcp-servers):
  - Gmail `https://gmailmcp.googleapis.com/mcp/v1`
  - Drive `https://drivemcp.googleapis.com/mcp/v1`
  - Calendar `https://calendarmcp.googleapis.com/mcp/v1`
  - Docs, Sheets, Slides, Chat, People and Classroom have their own too.
- **Status: Developer Preview.** Google opened public developer preview on 2026-05-01 (https://workspaceupdates.googleblog.com/2026/05/agent-tools-and-security-updates-for-workspace-developers.html).
  - Prerequisite: **membership in the Google Workspace Developer Preview Program** (configure page above).
  - To join, you apply with a *Google Workspace* account and a Cloud project.
  - The program terms say preview features "may not be included in public applications prior to the General Availability announcement" (https://developers.google.com/workspace/preview).
  - **Flag:** Magnus ships to owners, so this term matters. It is UNCONFIRMED whether a personal @gmail.com account can join the program.
- **Gmail tools:** "drafting, searching, and read/write" (blog above). The guide's suggested Gmail scopes are only `gmail.readonly` and `gmail.compose`.
  - **[PROBE]** The server's resource metadata also lists `gmail.send`, `gmail.modify` and `https://mail.google.com/`.
  - Whether any Gmail MCP tool actually sends mail is UNCONFIRMED.
- **No official alternative:** the `googleworkspace/cli` repo says it is "**not** an officially supported Google product" (https://github.com/googleworkspace/cli).
- **Most credible community server:** `taylorwilsdon/google_workspace_mcp`, PyPI `workspace-mcp` (https://github.com/taylorwilsdon/google_workspace_mcp).
  - MIT licence, release `v2.2.1` on 2026-10-27, about 3.4k stars. Read the version from https://pypi.org/pypi/workspace-mcp/json.
  - It speaks streamable HTTP (`--transport streamable-http`).
  - It supports OAuth 2.1 bearer tokens and has an "External OAuth provider mode — … validate bearer tokens only".
  - It does not depend on the preview program.

### Transport
- **Official:** remote streamable HTTP (configure page, "Others"). This fits `mcp_http` with no change.
- **Community fallback:** a local Python process (`uvx workspace-mcp --transport streamable-http`) or its Docker image (README above).

### Auth
- **[PROBE]** Each server's metadata (`/.well-known/oauth-protected-resource/mcp/v1`) names `authorization_servers: ["https://accounts.google.com/"]`. It lists per-product scopes; Drive's are `drive`, `drive.readonly` and `drive.file`.
- **Device flow cannot cover Gmail or Calendar.** Google's device flow accepts only these scopes: `openid`, `email`, `profile`, `drive.appdata`, `drive.file`, `youtube`, `youtube.readonly` (https://developers.google.com/identity/protocols/oauth2/limited-input-device#allowedscopes). It has no Gmail or Calendar scopes and no `drive.readonly`.
- **The standard alternative is the desktop-app flow:** an OAuth client of type **Desktop app**, with a **loopback IP redirect** (`http://127.0.0.1:<port>`) and **PKCE**. Google recommends loopback for macOS, Linux and Windows desktop apps (https://developers.google.com/identity/protocols/oauth2/native-app).
  - The Magnus desktop app can run this on the person's own machine: open the browser, catch the redirect on loopback, exchange the code.
  - It then hands the refresh token to the appliance vault. This matches DECISIONS.md: "a browser redirect lands on the person's own machine, not the appliance".
  - Google says "desktop apps are public clients and cannot keep secrets confidential", so a client secret in the app is not a real secret (same page).
- **Why the appliance cannot take the redirect:** web-client redirect URIs must be HTTPS, must not be raw IPs, and need a public-suffix TLD. Only localhost is exempt (https://developers.google.com/identity/protocols/oauth2/web-server#uri-validation). A LAN-only address like `https://192.168.x.x` or `magnus.local` is refused.
- **UNCONFIRMED: will the official servers accept a Desktop-app client?**
  - Google's MCP guide shows only **Web application** clients with "Use this client for an AI-powered agent" ticked.
  - Google's credentials guide offers that tick box under the Web application type (https://developers.google.com/workspace/guides/create-credentials).
  - I found nothing saying the MCP servers reject tokens from a client without the tick. This needs a live test.
- **Refresh tokens need extra parameters:** Google issues one only with `access_type=offline`, and a new one only with `prompt=consent` (configure page; https://developers.google.com/identity/protocols/oauth2/native-app).

### What the owner registers
1. **A Google Cloud project** (https://developers.google.com/workspace/guides/create-project).
2. **Enable the APIs.** For the official servers: `gmailmcp`, `calendarmcp` and `drivemcp` `.googleapis.com`, plus the underlying `gmail`, `calendar-json` and `drive` APIs. Command: `gcloud services enable …` (configure page).
3. **Join the Workspace Developer Preview Program.** This applies to the official servers only (https://developers.google.com/workspace/preview).
4. **Configure the consent screen and audience** (https://support.google.com/cloud/answer/15549945).
   - **External + Testing:**
     - Up to 100 test users.
     - Each test user sees a warning screen.
     - **Consent and refresh tokens expire after 7 days**, so the person must sign in again every week.
   - **Internal:** possible only for a project inside a Google Cloud Organization, with members of that organisation as users. Admins may still have to allow the restricted Gmail and Drive scopes (same page).
   - **External + In production:** needs app verification.
5. **Add the scopes** under Data Access → Manually add scopes. Google's list for the MCP servers (configure page):
   - Gmail: `gmail.readonly`, `gmail.compose`
   - Drive: `drive.readonly`, `drive.file`
   - Calendar: `calendar.calendarlist.readonly`, `calendar.events.freebusy`, `calendar.events.readonly`
   - **These are read-heavy.** Write access to Calendar needs scopes such as `calendar.events`; the probe shows the server supports it.
6. **Create the OAuth client:** type **Desktop app** (https://developers.google.com/identity/protocols/oauth2/native-app). Note the client ID.
   - If the official servers turn out to need the AI-agent client, create a **Web application** client instead. Give it a `http://127.0.0.1` or `localhost` redirect and tick "Use this client for an AI-powered agent". The tick cannot be changed later (create-credentials page). UNCONFIRMED which of the two clients works.
7. **Know the verification limits.**
   - `gmail.readonly`, `gmail.compose`, `gmail.modify` and `drive.readonly` are **restricted** (https://developers.google.com/workspace/gmail/api/auth/scopes, https://developers.google.com/workspace/drive/api/guides/api-specific-auth). `gmail.send` is sensitive, and `drive.file` is non-sensitive.
   - A production app on restricted scopes needs restricted-scope verification, which includes a security assessment (https://developers.google.com/identity/protocols/oauth2/production-readiness/restricted-scope-verification).
   - An unverified app shows the unverified-app screen and is capped at 100 new users (https://support.google.com/cloud/answer/7454865).
   - For one owner's own use, Testing mode avoids verification, at the cost of the weekly sign-in above (restricted-scope page, "Projects used in Development, Testing, or Staging").
   - I could not get Calendar's sensitive/restricted labels from Google's Calendar auth page: UNCONFIRMED.

### Refresh
- **Access token:** about one hour (configure page: "sign in again about every hour").
- **Refresh token:** valid until it is revoked or expires (https://developers.google.com/identity/protocols/oauth2#expiration). Causes:
  - The person revokes access.
  - Six months without use.
  - A password change, when Gmail scopes are granted.
  - The limit of 100 live refresh tokens per account per client; past it, the oldest is dropped silently.
  - An admin restriction.
  - **7 days while the project is in Testing.**

---

## 3. Microsoft 365 (Outlook mail, calendar, OneDrive)

### Server
- **Official, current: Work IQ MCP.**
  - One endpoint, `https://workiq.svc.cloud.microsoft/mcp`, with 10 generic tools across Microsoft 365 data (https://learn.microsoft.com/en-us/microsoft-agent-365/tooling-servers-overview, https://learn.microsoft.com/en-us/microsoft-365/copilot/extensibility/work-iq/mcp/overview).
  - It is GA as part of the Work IQ API (same overview).
  - It is paid per use in **Copilot Credits** (https://learn.microsoft.com/en-us/microsoft-365/copilot/extensibility/work-iq/api-overview).
  - It uses delegated Entra auth only (same page).
  - **[PROBE]** Its resource metadata names `authorization_servers: ["https://login.microsoftonline.com/organizations/v2.0"]` and the scope `fdcc1f02-fc51-4226-8753-f668596af7f7/WorkIQAgent.Ask`.
  - `/organizations` excludes personal Microsoft accounts, so this server is **work and school only**.
- **Official, legacy:** the Agent 365 Mail, Calendar and OneDrive servers (`https://agent365.svc.cloud.microsoft/agents/tenants/{tenant_id}/servers/mcp_CalendarTools`, …; https://github.com/microsoft/mcp). Microsoft says they are "only intended for backward compatibility … shouldn't be used in production environments or new projects" (Agent 365 overview above). **Do not use them.**
- **Official, not relevant:** Microsoft MCP Server for Enterprise covers Entra directory data, read-only (https://learn.microsoft.com/en-us/graph/mcp-server/overview).
- **Community: `softeria/ms-365-mcp-server`**, npm `@softeria/ms-365-mcp-server` (https://github.com/softeria/ms-365-mcp-server).
  - MIT licence, `v0.176.0` on 2026-10-28, about 1.1k stars, releases very often. Read the version from https://registry.npmjs.org/@softeria/ms-365-mcp-server/latest.
  - It works for personal accounts too.
  - It covers Mail, Calendar and OneDrive by default; Teams and SharePoint need `--org-mode`.
  - It is pre-1.0 and run by one company. It ships a built-in Azure app; Magnus should use the owner's own app instead.

### Transport
- **Work IQ:** remote streamable HTTP, a direct fit for `mcp_http`. UNCONFIRMED whether it answers Magnus's `2025-06-18` `initialize` (not tested; needs a token).
- **softeria:** a Node ≥ 20 process (`npx @softeria/ms-365-mcp-server --http <port>`) or stdio (README).
  - In `--http` mode it "**Requires** `Authorization: Bearer <token>` for all MCP requests" and "Validates tokens with Microsoft Graph API" (README, Authentication §2).
  - So Magnus can get a Graph token itself and pass it as the bearer, the same way it does today.
  - Its dynamic client registration is on by default in HTTP mode; disable it with `--no-dynamic-registration` (README).

### Auth
- **Device code with a public client is supported** (https://learn.microsoft.com/en-us/entra/identity-platform/v2-oauth2-device-code).
  - The request is `POST https://login.microsoftonline.com/{tenant}/oauth2/v2.0/devicecode`, with `tenant` set to `common`, `consumers`, `organizations` or a tenant ID.
  - The default `expires_in` is 15 minutes.
  - Personal accounts work through `/common` or `/consumers`.
  - A refresh token is issued only if `offline_access` is requested.
- **The app registration must be a public client:** Authentication → Advanced settings → **Allow public client flows = Yes**. Without it, device code is treated as a confidential-client request (https://learn.microsoft.com/en-us/entra/identity-platform/scenario-desktop-app-registration).
- **The Graph scopes Magnus needs all allow delegated user consent** (https://learn.microsoft.com/en-us/graph/permissions-reference):
  - `Mail.ReadWrite`, `Mail.Send`, `Calendars.ReadWrite` and `Files.ReadWrite` are delegated with **AdminConsentRequired: No**.
  - `Mail.Send`, `Calendars.ReadWrite` and `Files.ReadWrite` are documented as consentable by personal accounts.
  - Add `offline_access`, plus `User.Read` for identity.
  - **Caveat:** a tenant admin can turn user consent off or limit it, and then admin consent is required (https://learn.microsoft.com/en-us/entra/identity/enterprise-apps/configure-user-consent).
- **UNCONFIRMED: Work IQ with a third-party public client.** Can an owner-registered public client request `WorkIQAgent.Ask` through device code, and what consent does that need? Microsoft documents the scope and the authorization server (probe above), but I found no statement on third-party public clients.

### What the owner registers
1. Go to Entra admin center → App registrations → New registration (https://learn.microsoft.com/en-us/entra/identity-platform/quickstart-register-app).
2. Choose the supported account types: "Any Entra ID Tenant + Personal Microsoft accounts" to cover both kinds, or the owner's own tenant only (same page).
3. Under Authentication → Advanced settings, set **Allow public client flows = Yes** (desktop registration page above).
   - No redirect URI is needed for device code.
   - No client secret is needed.
4. Under API permissions → Microsoft Graph → Delegated, add `User.Read`, `offline_access`, `Mail.ReadWrite`, `Mail.Send`, `Calendars.ReadWrite` and `Files.ReadWrite`.
   - For Work IQ, add the Work IQ `WorkIQAgent.Ask` permission instead (UNCONFIRMED; see above).
5. If the tenant restricts user consent, an admin selects **Grant admin consent** (user-consent page above).
6. Note the **Application (client) ID** and the tenant (or `common`). If softeria is used, pass these as `MS365_MCP_CLIENT_ID` and `MS365_MCP_TENANT_ID` (README).

### Refresh
- **Access token:** a random lifetime of 60 to 90 minutes. With Continuous Access Evaluation it can reach up to 28 hours (https://learn.microsoft.com/en-us/entra/identity-platform/configurable-token-lifetimes).
- **Refresh token:** 90 days for non-SPA clients (https://learn.microsoft.com/en-us/entra/identity-platform/refresh-tokens).
  - It is replaced on every use, so Magnus must store the new one. The old one is not revoked.
  - Max inactive time is 90 days; max age is until revoked (token-lifetimes page).

---

## 4. Files (a folder on the appliance)

### Server
- **Reference:** `@modelcontextprotocol/server-filesystem`, from `modelcontextprotocol/servers`.
  - npm `2.0.1`, published 2026-10-25. Read the version from https://registry.npmjs.org/@modelcontextprotocol/server-filesystem/latest.
  - Licence: "SEE LICENSE IN LICENSE"; the repo's licence is not machine-readable through GitHub.
- **Transport: stdio only.**
  - `index.ts` imports only `StdioServerTransport` ([SRC] https://github.com/modelcontextprotocol/servers/blob/main/src/filesystem/index.ts).
  - The README says it "serves both protocol eras on stdio" (https://github.com/modelcontextprotocol/servers/tree/main/src/filesystem).
- **Allowed folders:** command-line arguments. Under 2026-07-28 these are the whole allow-list, because Roots is deprecated (same README).
- **I found no official streamable-HTTP filesystem server.** The only route to HTTP is a third-party bridge:
  - `sparfenyuk/mcp-proxy` (MIT, active; https://github.com/sparfenyuk/mcp-proxy)
  - `supercorp-ai/supergateway` (MIT, active; https://github.com/supercorp-ai/supergateway)

### What the spec says about stdio
- **2025-06-18** (the version Magnus speaks) defines two standard transports, stdio and streamable HTTP, and says "Clients **SHOULD** support stdio whenever possible" (https://modelcontextprotocol.io/specification/2025-06-18/basic/transports).
- In stdio, the client launches the server as a subprocess and exchanges newline-delimited JSON-RPC on stdin and stdout (same page).
- **2026-07-28** keeps both transports as standard bindings (https://modelcontextprotocol.io/specification/2026-07-28/basic/transports).
  - I did not find the "SHOULD support stdio" sentence in that version.
  - The stdio rules (https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/stdio):
    - Messages are newline-delimited and contain no embedded newlines.
    - The server writes nothing to stdout that is not MCP.
    - stderr is not an error signal.
    - A client that supports both eras probes with `server/discover`.

### Options (smallest first)
1. **Add a stdio transport to the kernel**, as `transport = "mcp_stdio"` with a command and arguments.
   - This uses the spec's standard transport with the reference server unchanged.
   - It adds no network listener.
   - The server process can run under the appliance's existing sandbox, with only the shared folder visible.
2. **Run a bridge**, such as mcp-proxy or supergateway, that exposes the stdio server on localhost HTTP. This needs no kernel change, but it adds a third-party process and a local port.
3. **Write a small built-in folder server.** This is the most code and gives up the reference implementation.

---

## Changes to Magnus these findings imply (not decided here)
- **Token refresh.** Every OAuth connector needs a refresh token in the vault, plus a refresher that stores the token returned by each refresh:
  - GitHub every 8 hours.
  - Google every hour.
  - Microsoft every 60 to 90 minutes.
  - Today `credential` holds only a static bearer.
- **A Google sign-in flow on the desktop app** (loopback + PKCE). Device code cannot cover Gmail or Calendar.
- **`mcp_stdio`** if the Files connector uses the reference server.
- **Protocol `2026-07-28`** when convenient. `2025-06-18` still works with the servers checked.

## Not confirmed (needs a live sign-in or the owner)
- Whether the GitHub remote server filters tools correctly for a GitHub App token, which has permissions rather than scopes.
- Whether Google's official MCP servers accept a Desktop-app client, or require a Web client with the "AI-powered agent" tick.
- Whether a personal @gmail.com account can join the Workspace Developer Preview.
- Whether any Gmail MCP tool sends mail.
- The sensitive/restricted labels of the Calendar scopes.
- Whether Work IQ MCP accepts a device-code token from an owner-registered public client, and what consent `WorkIQAgent.Ask` needs.
- Whether Work IQ answers a `2025-06-18` `initialize`.
- Whether the community `workspace-mcp` accepts a Google access token obtained outside it as a bearer. Its "external OAuth provider" mode suggests it does.
