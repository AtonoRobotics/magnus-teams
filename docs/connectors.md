# The connectors against their spec

The owner's "Magnus Connectors Spec" (rev 48, 2026-10-31, Claude Docs) and
its catalog mockup ("Magnus Connector Catalog",
https://claude.ai/artifact/7k9iBoc6hFKYDUw4nMFxQQ: Library · Discover in
dark, light and glass; Connector details; Connecting from chat) are built as
written, amended only by the owner's rulings (`docs/proposals/spec-decisions.md`:
5, a person only tightens; 7, a signed manifest's classes are the owner's
classification; 12, a person turned off pauses; 14, a person's data is the
organisation's by default). The connectors are done when every line below is
checked, each with the test that proves it (owner, 2026-11-02: "do not build
min slices and give up before the job is done").

A line is checked only where code and a test that exercises it both exist;
the test is named. Audited against `console-admin-origin` (master at #85 plus
one commit), 2026-11-02.

## Scope and requirements

- [ ] Breadth: Google, Microsoft, coding and deployment services (Vercel and peers), Slack, the other common business services, and FreeCAD, Autodesk Fusion, SolidWorks and Blender (today: four Google manifests, `catalog/google-*.toml`, Slack's, `catalog/slack.toml`, and Sentry's, Hugging Face's, GitLab's, Supabase's (read only) Cloudflare's, GitHub's and Vercel's, `catalog/<id>.toml`, compiled in by `crates/magnus/src/catalog.rs` `SOURCES`)
- [ ] Plug and play: every connector is a Connect button that opens the provider's own login page and then works (the button and the page exist, `connectors.mjs`, `app-apps.mjs` against a fake app; a Google sign-in from the desktop app carries `iss` back, see Connect flow, and has not been completed against Google's own token endpoint)
- [x] No connector shows the user a form: no address, key, token, client ID or header fields in the app (`desktop/tests/connectors.mjs`, "asks nothing here", and the owner's Settings › Connectors)
- [ ] A service whose provider offers no sign-in is not in the catalog (every catalog manifest signs in; an admin can no longer add a server with a key, `add-by-sign-in`, but can still add one that asks for no sign-in at all: open with the owner)
- [ ] Desktop programs detected and enabled with one consent (no detection, no bridge)
- [ ] Non-technical users: nothing in connect, use or repair needs configuration, a terminal or a developer console (repair of a broken sign-in is untested; the Google client secret is put in by a CI step or by an admin in the app's Admin › Connectors)
- [ ] Removable modules: each connector added, disabled or removed without rebuilding Magnus (disable: the app's Admin › Connectors, `desktop/ui/admin-connectors.js`; add or remove a catalog connector needs a release, manifests are compiled in)
- [x] Isolation: a person's connections and tokens are theirs alone; no one else's agent acts through them (`connectors.rs` `the_owner_adds_an_app_each_person_signs_in_with_their_own_account_and_the_kill_switch_revokes_every_sign_in`)
- [x] Connector results are the person's own data: no admin, and not the server's owner, reads them (`decisions/2026-11-11-a-magnus-admin-supports-and-reads-nothing.md`, which replaces ruling 14)
- [x] Org control: admins decide which connectors their users may use: on or off for the organisation (`admin_connectors.rs`), and allowed or blocked per group (`library-groups`; `library_groups.rs`)
- [ ] Done: every launch-catalog connector meets all six on Windows and Linux, against an appliance reached through pyrallon.ai
- [ ] Done: Magnus, asked about any catalog service, says whether it can work with it and offers that connector's card (no catalog tools for the agent)

## What the user sees

- [x] Every catalog connector already listed and configured when the user opens Magnus; nobody sets one up (`google.rs` `google_s_connectors_are_in_every_library_already_set_up_and_sign_in_at_google`; only once Alpha Vector's Google secret is provisioned: until then the cards say "Not ready on this appliance yet")
- [x] Browse: the Library shows the whole catalog as cards (`connectors.mjs`)
- [x] Browse: the user searches (`connectors.mjs`, by name and by what is wanted)
- [ ] Browse: or asks Magnus (no catalog tools)
- [ ] Connect: click Connect, sign in on the provider's page, approve, the card turns Connected (works against a fake app, `app-apps.mjs`; never shown for a real provider. Google's completion was refused because the desktop app dropped `iss`; it now carries it back, see Connect flow, and is still untested against Google's token endpoint)
- [x] Never asked, in the app: no client ID, secret, address, key, token, redirect URI or scope (`connectors.mjs`)
- [ ] Never asked, in Admin: Admin › Connectors still asks an admin for Google's client secret until the relay holds it; adding an app asks for no key (`add-by-sign-in`). The VPN-only admin console that also asked is retired (`decisions/2026-11-11-console-retired.md`)
- [ ] Where the org's credentials live: Alpha Vector's client IDs and secrets in the relay's secret store, nowhere in the app, console or manifests, the appliance getting client IDs from the relay (today: client IDs in `catalog/registrations.toml`, compiled in; Google's secret in the appliance's vault, put there by `release/admin-on-appliance.sh` from the repository secret)
- [x] The connector form asking for a client ID, client secret and web address is deleted from the app, not hidden (#67; `connectors.mjs`, the owner's Settings › Connectors ask for nothing)
- [ ] Acceptance: on a fresh install a user connects Gmail, Slack, GitHub and Vercel with only clicks and the providers' login pages (Slack is brokered, `catalog/slack.toml`, and signs in through `https://magnus.pyrallon.ai/oauth/slack/callback`, which the appliance sends on to `http://127.0.0.1:17321/callback`; GitHub's and Vercel's manifests sign in with the owner's clients, Vercel's only once Vercel approves it, `decisions/2026-11-10-github-and-vercel-connectors.md`; Gmail's desktop completion dropped `iss` and was refused, which the app now returns, and no test exchanges a code with Google)
- [x] Acceptance: an automated UI test fails if the connect path renders any text input inside Magnus (`connectors.mjs` `typed()`, in the browser; the desktop path is not checked for inputs)

## Decisions this spec builds on

- [x] Agents run on the appliance; users work through the desktop app on Windows and Linux (before this spec)
- [ ] The appliance reachable remotely through pyrallon.ai for the connect flow (the relay's completion endpoint and the appliance registry are not built; proposed, not ruled)
- [x] Agent policy in Cedar, enforced on the appliance (`policy.rs`; `inv6_policy.rs`, `inv8_layers.rs`)
- [x] A connection is a delegated grant the user makes by signing in (`connectors.rs`)
- [ ] Where a provider offers a first-class agent identity, Magnus uses it (no manifest field, no code)
- [x] The agent chooses connectors and tools; no scripted workflows (`inv5_model_decides.rs` `every_app_call_traces_to_a_model_decision`)
- [x] Ruling: a signed manifest's tool classes count as the owner's classification; an unclassified tool cannot run (compiled-in manifests stand in for signed ones; `inv1_boundary.rs` `tools_the_owner_has_not_classified_cannot_run`)
- [x] Ruling: a layer below the organisation only narrows (`inv8_layers.rs`)
- [x] Ruling (2026-11-11, replacing ruling 14): a person's data, connector results included, is theirs; admins support accounts and the server and read no one's data (`decisions/2026-11-11-a-magnus-admin-supports-and-reads-nothing.md`)
- [ ] Ruling: turning a person's identity-provider account off pauses their connections and schedules (schedules pause, `people.rs` `a_person_turned_off_in_keycloak_loses_their_sessions_and_their_schedules_pause`; connections are not refused)
- [x] Ruling: the kill switch revokes every connection (`kill_switch.rs` `the_kill_switch_revokes_sign_ins_seals_the_vault_and_releasing_resumes_nothing`, `connectors.rs`)

## Connector kinds

- [x] Remote MCP: the provider's hosted server, Magnus the client (`catalog.rs`, `apps.rs`; `google.rs` `each_google_manifest_names_exactly_the_tools_its_live_server_lists`)
- [x] Brokered API: an Alpha Vector MCP server over the provider's API, in its own sandbox on the appliance (`brokered.rs`; Gmail, `gmail.rs`, `gmail_contract.rs`)
- [ ] Local: an MCP server in or beside a desktop program, bridged by the desktop app
- [ ] All three reach the agent the same way, as MCP tools on the appliance, in one library (remote and brokered run, `catalog::kind_problem`, `brokered.rs`, Gmail brokered; no local)
- [ ] Remote called from the appliance's MCP client, brokered beside it, local through the desktop app; the relay talks only to providers' token endpoints
- [ ] A service moves from brokered to remote with no change visible to the user
- [ ] An org admin adds a remote MCP server by its address: this lived in the retired admin console (`decisions/2026-11-11-console-retired.md`) and is not yet in the app's Admin › Connectors; it stays out of the person's own Connectors screen (`connectors.mjs`)
- [x] Magnus discovers the added server's sign-in from its address; then it is a card like any other, each user logging in (`connectors.rs`, the server's 401 and metadata)
- [x] A server that asks for a key, token or client details instead of a sign-in is refused, and the admin told why (`admin_connectors.rs` `the_console_sets_rotates_and_revokes_the_organisations_credentials_and_never_shows_one`: a key, a client ID, a server answering 401 with no sign-in, and one whose sign-in registers no client, each refused with the reason)

## Catalog and connector manifests

- [ ] Each connector defined by a signed manifest Alpha Vector publishes to a catalog service, added, updated or withdrawn without a Magnus release (compiled in, `catalog.rs` `SOURCES`)
- [ ] Appliances pull the catalog, verify every signature against Alpha Vector's catalog key, and reject any manifest that fails
- [x] `id`, `version` (`catalog.rs` `every_manifest_is_well_formed`)
- [ ] `name`, `icon`, `summary` (name and summary checked; no icon: `letters` and `tint` draw a tile)
- [x] `publisher`, shown as "by …" on the card and page (`every_manifest_is_well_formed`, `connectors.mjs`)
- [x] `category`, the Library's chips: email and chat, files, code, work, design and CAD (`every_manifest_is_well_formed`, `connectors.mjs`)
- [x] `abilities`, plain lines for the page before connecting (`every_manifest_is_well_formed`, `connectors.mjs`)
- [ ] `kind`: `remote`, `brokered` or `local` (`remote` and `brokered` are accepted, `catalog::kind_problem`; not `local`)
- [ ] `endpoint`: remote the MCP URL (done); brokered the image digest (done: `a_brokered_manifest_names_its_image_and_hosts`, checked before each boot, `brokered.rs`); local the per-OS detection rules, add-on package and hash, enable method and local address
- [ ] `auth.registration`: `preregistered`, `cimd`, `dcr`, or `none` for local (only `preregistered` and `dcr`)
- [ ] `auth.client`: public with PKCE, or confidential through the relay; the client ID from the relay for its environment (today `catalog/registrations.toml` `kind`, the secret on the appliance)
- [x] `auth.scopes`: exactly the scopes requested, each with its plain line (`scopes` and `asks`, `every_manifest_is_well_formed`; the request, `google.rs`)
- [ ] `auth.revoke`: the provider's revocation endpoint in the manifest (found from the authorization server's metadata instead)
- [x] `tools`: every tool's class, read, reversible, irreversible or financial (`every_manifest_is_well_formed`; named `reversible_write`)
- [ ] `platforms`: local only, `windows`, `linux` and supported program versions
- [ ] `signature`: Alpha Vector's catalog signature over everything above
- [x] A person's library is the catalog filtered by the org's policy: off for the organisation (`admin_connectors.rs`), and per group, allowed or blocked (`library_groups.rs`), plus servers the admin added by URL (`connectors.rs`)
- [ ] Admin-added entries are org-local manifests signed by the appliance

## Credential plane

- [ ] Alpha Vector holds every provider registration, tokens live only on the user's appliance, and a stateless relay at connect.pyrallon.ai is the one redirect target and the only holder of client secrets (tokens on the appliance: done; no relay, secrets on the appliance)

### Client registration

- [x] Pre-registered: Alpha Vector's own client for the provider (Google: `google.rs`)
- [ ] Pre-registered clients for Microsoft, Slack and the others under Provider registrations (Slack's manifest uses its client, `slack.rs`; GitHub's and Vercel's manifests use theirs; Microsoft's public client signs in Outlook, Outlook Calendar and OneDrive, brokered over Graph, through `organizations`, `catalog/microsoft-*.toml`, `issuers.rs`; Sentry's client ID is in `catalog/registrations.toml` and no manifest uses it; Sentry's catalog connector registers Magnus itself at `mcp.sentry.dev`, which does not take sentry.io's clients)
- [ ] Client ID Metadata Document, used when the provider advertises it, one document at pyrallon.ai naming the relay callback
- [x] Dynamic Client Registration as the fallback (`connectors.rs`, the fake app registers Magnus; against Sentry, Hugging Face, GitLab.com, Supabase and Cloudflare, `sentry.rs`, `huggingface.rs`, `gitlab.rs`, `supabase.rs`, `cloudflare.rs`)
- [ ] The order the MCP authorization spec (2026-07-28) sets: pre-registered, then CIMD, then DCR
- [x] Asking the user to type in client details is excluded (`add_connector` refuses a `client_id`; `admin_connectors.rs`)
- [ ] A server supporting none of the three cannot be added, and the admin is told why (an added server must offer Dynamic Client Registration, else it is refused with the reason, `admin_connectors.rs`; one offering only CIMD is refused too until CIMD is built)

### Connect flow

- [x] The user clicks Connect in the Library (`connectors.mjs`, `app-apps.mjs`)
- [ ] Or on a card the agent placed in chat
- [x] The appliance keeps the PKCE verifier (`connectors.rs`; `google.rs` checks S256)
- [ ] It signs a `state` naming the appliance, user, connector, a nonce and an expiry (a random `state` kept in memory, `connectors.rs` `pending`; lost on restart)
- [ ] The redirect URI is the relay callback (today a loopback address on the person's computer, RFC 8252)
- [x] The desktop app opens the system browser at the provider's sign-in page; Magnus never sees a password (`app-apps.mjs`)
- [ ] The provider redirects to the relay, which checks `state`, then sends the browser to the appliance's completion endpoint at its pyrallon.ai address, found from the appliance registry (proposed, not ruled)
- [ ] The browser shows "Connected, return to Magnus." (the loopback page says "Signed in. You can close this page and return to Magnus.")
- [x] A public client exchanges the code directly with the provider (`connectors.rs`, fake app)
- [ ] A confidential client exchanges through the relay's token proxy, which adds the secret and stores nothing (the appliance sends the secret itself)
- [x] The appliance stores the tokens, marks the connection live, and the agent sees the tools (`connectors.rs`)
- [x] A Google sign-in completes from the desktop app: Google advertises `authorization_response_iss_parameter_supported`, so the kernel requires `iss`, and the shell's `loopback_wait` now carries it back (`connector-sign-in-iss`; `desktop_app.rs` `the_owner_connects_an_app_from_settings_and_reapproves_a_changed_tool` against a server that names itself as Google does). Not yet: a test that reaches Google's own token exchange (it needs a Google test account CI can sign in with).

### Relay

- [ ] Bounces callbacks
- [ ] Proxies token-endpoint calls (exchange, refresh, revoke) for confidential clients
- [ ] Holds client secrets and no user data
- [ ] Appliances authenticate with the mutual-TLS certificate issued at provisioning
- [ ] Logs appliance, provider, operation, time and result, never a code or token
- [ ] Runs active-active in at least two regions (Lifecycle)
- [ ] A staging relay at connect.staging.pyrallon.ai with staging registrations (Building connectors)

### Custody

- [x] Tokens encrypted at rest in the appliance's vault (`vault.rs`, XChaCha20-Poly1305; `inv2_credentials.rs` `secrets_can_be_written_but_not_read_back`)
- [ ] Under a key that never leaves the appliance (the backup carries the vault's key, `vault.rs` "for a backup")
- [x] Bound to one user and one connector (each entry is encrypted with its name, `connector_<app>__<person>`, as associated data, so moved under another name it does not open; `vault.rs` `a_sign_in_moved_under_another_name_does_not_open`, and entries from before are bound on opening, `an_entry_from_before_binding_is_bound_to_its_name_on_opening`)
- [x] Tokens never enter model context or agent sandboxes; the MCP client attaches them (`inv2_credentials.rs` `credentials_are_used_but_never_seen`)
- [ ] A provider's first-class agent identity declared in the manifest and held the same way

## Remote MCP connectors

- [x] The appliance is a standard MCP client over Streamable HTTP (`apps.rs`; `google.rs` tool listing)
- [x] One session per user per connection, carrying that user's token, never shared (`connectors.rs`: carol's task uses carol's sign-in)
- [x] The appliance lists the server's tools (`run/mod.rs` `live_app_tools`)
- [ ] It follows `list_changed` notifications
- [x] A retired tool leaves the agent at once (a tool the app's live list no longer holds is not offered; its pin is kept, so its return unchanged needs no new review; `retired_tools.rs`)
- [x] A new or changed tool reaches the agent only once classified, by the next manifest version or, for an admin-added server, by the admin (`inv1_boundary.rs` `a_changed_tool_definition_is_disabled_and_hidden`, `tools_the_owner_has_not_classified_cannot_run`; a brokered connector's definitions come from the image its manifest names, so a differing one is the next manifest version's, pinned as new: `gmail.rs` `gmail_s_tools_from_its_hosted_server_are_offered_from_its_image`)
- [x] Server annotations inform the classification and do not replace it (`connectors.rs` `suggested_risk`; `connectors.rs` test)
- [x] Scope challenges: a call needing an ungranted scope (GitHub's server returns a challenge) becomes a Connect card asking for the added scope, and the call proceeds once approved (`agent_apps.rs` `a_call_that_needs_more_access_asks_for_it_with_a_card_and_goes_through_once_approved`: MCP's `insufficient_scope` challenge, the card in the thread, sign-in asking for the added scope, the call through)
- [ ] Provider errors, rate limits and retry-after included, return to the agent intact as tool errors, with no retry by the client (errors are returned; no test, and retry-after is not carried)
- [ ] Client gates: Magnus on providers' approved-client and redirect allowlists (Provider registrations)

## Brokered API connectors

- [ ] No third-party integration platform (Nango or similar); secrets only in the relay
- [x] Packaging: one server image per provider, named by digest in its manifest (`a_brokered_manifest_names_its_image_and_hosts`; built reproducibly by `brokered/build.sh`, its file checked against the digest before every boot; `brokered.rs` `each_person_has_their_own_microvm_called_with_their_current_token_reaching_only_its_hosts` boots one)
- [x] Isolation: one instance per user per connector, each in its own Firecracker microVM, started on first call, stopped when idle (`brokered.rs` `each_person_has_their_own_microvm_called_with_their_current_token_reaching_only_its_hosts`; Disconnect, deletion and the kill switch stop it too, `disconnecting_erasing_and_the_kill_switch_stop_the_persons_microvm`)
- [x] Credentials: a short-lived access token injected per request; no refresh token, no state between calls, nothing on disk (`each_person_has_their_own_microvm_called_with_their_current_token_reaching_only_its_hosts`: each call carries the token current then, never a refresh token, and an answer holding it is cleaned; the microVM has no disk, `an_instance_has_no_network_device_and_no_disk`; the server kit keeps nothing between calls)
- [x] Network: the microVM reaches only the manifest's API hosts, through an egress proxy on the appliance (`each_person_has_their_own_microvm_called_with_their_current_token_reaching_only_its_hosts`: another host refused through the proxy, a direct connection unreachable, another host port refused; `the_proxy_admits_only_the_manifests_hosts_exactly`)
- [ ] Tools at the level a person works (find, read, create, update, share, delete), schemas validated on both sides (the check on both sides is built, `schema::check` in the server kit and `brokered.rs`; Gmail's tools, `brokered/gmail`; Calendar's, `brokered/calendar`; Chat's, `brokered/chat`; Slack's, `brokered/slack`; not yet Drive, Docs or Sheets)
- [ ] Contract tests in CI against provider test accounts before publishing (Gmail's, `gmail_contract.rs`, Calendar's, `calendar_contract.rs`, and Slack's, `slack_contract.rs`: the provider is reached from the microVM in every run; every tool once the owner's test account is given to CI, which it is not yet; without it a run says `SKIPPED`, and `ci/acceptance.sh contracts` fails instead)
- [ ] Switching to Remote retires the image; users keep their connection when the grant carries over, else reconnect once

## Local desktop connectors

### The bridge

- [ ] Detect: on start and whenever installs change, supported programs and versions (Windows registry and install paths; Linux package records, Flatpak, Snap, install paths), reported to the appliance
- [ ] Enable: one consent per program; the app installs the pinned add-on from the catalog
- [ ] Enable: where the server is a built-in setting, the app shows that one setting and confirms the server answers
- [ ] Supervise: knows whether each program runs, starts its server; launches a closed program when policy allows, else asks
- [ ] Tunnel: loopback to each local server, carried over the app-to-appliance channel; servers never beyond loopback; the bridge takes calls only from its own appliance
- [ ] Tunnel through pyrallon.ai: the trust model names Cloudflare as seeing this traffic; pinned on the office network (proposed, not ruled)
- [ ] Report state per program per device: not installed, not enabled, program closed, ready; the agent sees them
- [ ] Each device its own set, named by device
- [ ] Files a program produces (STEP, STL) return as tool results through the tunnel

### Programs

- [ ] Autodesk Fusion: Autodesk's built-in Fusion MCP Server, Windows; Preferences > General > API > Fusion MCP Server, then Apply; the app confirms the port answers
- [ ] Blender: Blender Lab's official MCP server and add-on, Windows and Linux; the app installs the add-on; Blender 5.1 or newer
- [ ] FreeCAD: Alpha Vector's FreeCAD workbench hosting an MCP server over FreeCAD's Python API, Windows and Linux; the app installs the workbench
- [ ] SolidWorks: Alpha Vector's add-in hosting an MCP server over the COM API, Windows; the app installs and registers it; needs a licensed, running SolidWorks; single-session

### Guarding the user's computer

- [ ] Every local tool call passes the appliance's Cedar policy before the bridge forwards it
- [ ] The default policy requires the user's approval for tools that execute code inside a local program
- [ ] An admin can relax it for the organisation or a group; a user can only make it stricter (ruling 5)

## How the agent uses connectors

- [x] Index, then load: the context always holds the user's connector index (every connector policy makes available, name, one-line summary, state: connected, needs attention, not connected), generated from the catalog and connections at each session start and on every connection change (`app_tools.rs` `app_index`, built for every run and chat turn; `agent_apps.rs`: the index before the person's words, an app blocked for them absent, a refused sign-in as needing signing in again)
- [ ] The index's "on this computer" state and programs per device (with the local desktop connectors, above)
- [x] Tools searched across connectors and full schemas loaded on demand (past `LOAD_ALL`, 40 tools across the person's connected apps, a run is offered `find_tools` and what it found; `tool_search.rs` both ways)
- [x] One user at a time: an agent uses only its user's connections (`connectors.rs`; refusal names the Library, `boundary.rs:252`)
- [x] Untrusted content: tool descriptions and results inform but cannot change instructions or policy (`inv4_taint.rs` `content_claiming_authority_changes_nothing`; descriptions held for review, `run/mod.rs` `review_item`)
- [ ] The appliance labels each result's source in context
- [x] Finding connectors: catalog tools to search the library by service name or by what the user wants done (`find_apps`; `agent_apps.rs` `magnus_finds_an_unconnected_app_by_what_is_wanted_and_offers_its_card_with_why`)
- [x] Read a connector's details: what it can do, the access it asks for, the account it needs (`app_details`: abilities, can and cannot, each scope's plain line, whether the person signs in; `app_tools.rs`)
- [x] Place a connector's card in chat, saying what it is for; suggest one for something it could automate; only the user completes a sign-in (`offer_app`; `agent_apps.rs`: the card with why, nothing connected until the person signs in; `chat-app-card.mjs` in every look and at phone width)
- [x] Answers "can you work with my Gmail?" truthfully (the index says each app's state; `find_apps` says when nothing in the Library does it, and `offer_app` refuses an app outside it: `agent_apps.rs` `magnus_cannot_offer_an_app_outside_the_persons_library`)
- [ ] Isolation of results: what comes through a connection is kept in the content store under its person and recalled only for them (the walls exist for requests, memory and files, `content_walls.rs`; no test for a connector's results)
- [x] Writing a connection's content into shared org memory passes policy like any other action (`app_content_policy.rs`: a rule forbidding the knowledge base after private content refuses filing what Alice's calendar said, and lets web content through; who reads what is filed, `knowledge.rs` `nobody_retrieves_an_entry_they_may_not_read`)

## User experience

- [x] The card never shows protocol words: no OAuth, scope, token, MCP or port (`connectors.mjs`, `google.rs` `a_person_s_card_for_gmail_says_what_magnus_can_do_in_plain_words`, `every_manifest_is_well_formed`)

### The connector card

- [x] The same card in the Library, in chat, in a task and in approvals (`offered_card`: chat and task in `agent_apps.rs` and `chat-app-card.mjs`; an approval for an app's action in `approval_app_card.rs` and `approval-app-card.mjs`, for the person and for whoever decides it)
- [x] Not connected: name, icon, one sentence, Connect (`connectors.mjs`; the icon is a letters tile)
- [ ] Connecting: the system browser opens at the provider's page; on approval the browser says to return, and the card flips to Connected (`app-apps.mjs` against a fake app; not for Google)
- [ ] On this computer: "Blender 5.2 is on this computer" and Enable; the one setting to turn on; Ready when the server answers
- [ ] Needs attention: sign in again, approve more access, or open a program; one button (sign in again and approve more access: `appOfferCard` and `connectorAction`, the states in `agent_apps.rs`, not yet driven in the browser; open a program waits on local connectors)

### Library

- [x] Laid out as the mockup, in light, dark and glass (`connectors.mjs`, every look and phone width)
- [x] Two views: Discover the whole catalog; Yours connected, needs attention, and programs not yet enabled (`connectors.mjs`; Yours tested empty only)
- [x] One search box taking a name or a description of what is wanted (`connectors.mjs`: "drive", "find a time"; word matching over names, sentences, publishers and abilities)
- [x] Category chips: Email and chat, Files, Code, Work, Design and CAD (`connectors.mjs`)
- [ ] Sections in Discover: On this computer (never filled), Needs attention (untested), Popular (`connectors.mjs`); an empty section is not shown (`connectors.mjs`)
- [ ] The card: icon (provider's own: not yet), name, review mark, one sentence, publisher (`connectors.mjs` for all but the icon)
- [x] The card's action: a plus to connect (`connectors.mjs`)
- [ ] The card's action: a check when connected (drawn, untested with a real connection)
- [ ] The card's action: a spinner while signing in on the provider's page (drawn, untested)
- [ ] The card's action: sign in again when it needs attention (drawn, untested)
- [ ] The card's action: Enable for a program on this computer (drawn for a `local` state the appliance never sends)
- [ ] Icons: each provider's own mark from its listing, used as its brand guidelines allow (none in the repository)
- [x] Connector page, not connected: name, icon, publisher and review mark; abilities; each scope as its plain line; "You sign in on …'s own page. Magnus never sees your password, and you can disconnect at any time."; one Connect button (`connectors.mjs`)
- [ ] Connector page, connected: Connected mark; the account (email, workspace or tenant); what it can and cannot do; when connected and last used; recent actions from the event log; Disconnect (drawn in `app.js` `connectorBody`; DECISIONS.md: "tested with real calls in the next pull request", never done)

### Inside work

- [x] When a task or chat needs a connection, Magnus says so in that thread with the card: name, icon, why ("Needed to post in your production channel"), Connect; once connected it carries on (mockup: Connecting from chat) (`agent_apps.rs`: in a chat and on a task's page; the call goes through once approved; `chat-app-card.mjs`)
- [ ] Asked about a program not on this computer, Magnus says so, and that once installed it shows under On this computer, enabled with one click (mockup: "Can you work in SolidWorks?")

## Org policy, isolation and audit

### Policy

- [x] From the admin dashboard an admin allows or blocks each connector for the whole org; blocked, it leaves those users' Library (`admin_connectors.rs`: turned off, Gmail leaves the Library)
- [x] For groups: allowed or blocked per group, and blocked connectors disappear from those users' Library, cannot be opened or signed in to, and are refused at the boundary (`library-groups`; `library_groups.rs` `an_app_blocked_for_a_group_leaves_that_groups_library_and_cannot_be_signed_in_to`, `boundary.rs` `an_app_is_for_its_groups_and_never_for_the_groups_it_is_blocked_for`)
- [x] Cedar policies act on connector, tool, user, group and tool class (`policy.rs` entities `Person` in `Group`, `Tool` in `App` with `risk`; `inv6_policy.rs`, `inv8_layers.rs`)
- [x] A catalog manifest declares each tool's class, which counts as the owner's classification; annotations only inform vetting (ruling 7; `catalog.rs`, `inv1_boundary.rs`)
- [x] For an admin-added server, the admin classifies each tool when it first appears; an unclassified tool cannot run (`connectors.rs` test, through the owner's Settings › Connectors in the app; not in the admin console, where the spec puts admin work)
- [x] Every connector call passes policy on the appliance before it leaves (`inv1_boundary.rs`, `inv6_policy.rs`)
- [x] A tool that appears or changes after its manifest was signed is hidden and cannot run until classified (`inv1_boundary.rs` `a_changed_tool_definition_is_disabled_and_hidden`)

### Isolation

- [x] A connection belongs to one user; no admin or other user can act through it (`connectors.rs`)
- [x] What it returns is the person's own data: no admin reads it (`decisions/2026-11-11-a-magnus-admin-supports-and-reads-nothing.md`, which removed the admin data read and the data-access policy's organisation default)
- [x] Admins see metadata only: which connectors are connected and when last used, without content (the app's Admin › Connectors)
- [x] When an admin removes a user, every connection is revoked at the provider and its tokens deleted (`person_admin.rs` `erase_person`; `admin_people.rs`: Bob's calendar sign-in revoked at the app, both tokens)
- [x] When the identity provider turns a user off, their connections refuse every call and schedules pause until they are back; tokens stay in the vault unused (`PersonTurnedOff`, cleared by their next sign-in; `turned_off_apps.rs`; schedules, `people.rs`)

### Audit

- [ ] Each call writes one event: time, user, agent, task (the run's `Intent`, `Decision` and `ToolResult` events carry these through the run)
- [x] Connector, tool and the policy decision (`Decision` events; `inv3_event_log.rs`, `inv5_model_decides.rs`)
- [ ] Outcome and a digest of the arguments (the full input is in the `Intent` event, not a digest)
- [ ] Full arguments and results stored in the user's own space
- [ ] Users see their full history; admins see the events, and content where policy allows (the connector page shows the last ten; no full history view)

## Lifecycle

- [x] Refresh: access tokens refreshed before they expire, directly for public clients (`connectors.rs`: 30-second tokens, every use refreshes; `connectors_review.rs` `a_refresh_without_expires_in_keeps_the_token_refreshing`)
- [ ] Refresh through the relay for confidential clients
- [x] A rotated refresh token replaces the old in one vault write (`connectors.rs` `connector_secret`, one refresh at a time; `connectors.rs` test)
- [ ] Re-sign-in: a refresh rejected as invalid marks the connection Needs attention and raises the card (drawn; no test; the card is not raised into the person's work)
- [x] Revoked at the provider: the next call fails and the connection moves to Needs attention (`connector_lifecycle.rs` `a_sign_in_ended_at_the_app_asks_the_person_to_sign_in_again`)
- [x] Re-sign-in: a refresh rejected as invalid marks the connection Needs attention and raises the card (`agent_apps.rs` `a_sign_in_the_app_refused_is_offered_again_as_a_card_and_its_tools_wait`: the index says so, the tools wait, the card offers sign in again)
- [ ] Revoked at the provider: the next call fails and the connection moves to Needs attention (no test)
- [x] Disconnect deletes the person's tokens, and only theirs (`connectors.rs` test, "Alice ends her own sign-in")
- [x] Disconnect first calls the provider's revocation endpoint, then deletes the tokens; both ends of a connection are in the log (`connector_lifecycle.rs` `disconnect_revokes_at_the_app_first_and_both_ends_of_a_connection_are_in_the_log`)
- [ ] Disconnect through the relay for confidential clients
- [x] For a provider with no revocation endpoint, the tokens are deleted and the card names the provider page where the user removes Magnus (`connector_lifecycle.rs` `an_app_that_cannot_revoke_is_disconnected_here_and_the_person_is_told`; every catalog manifest names its page, `remove_access`, catalog.rs `every_manifest_is_well_formed`)
- [ ] Manifest updates: new versions pulled and applied when no call is in flight; brokered images by digest; local add-ons through the app's updater when the program is closed
- [ ] A new version needing more access asks through the scope-approval card the first time a tool needs it
- [ ] Withdrawal: appliances disable the connector, revoke its connections and tell affected users in their Library
- [ ] Relay availability: unreachable, confidential connections work until their access tokens expire; public ones unaffected
- [x] Kill switch, from the app or `magnus kill` on the server itself: refuses every connector call, revokes every connection at its provider, deletes the tokens and seals the vault; releasing restores none (`kill_switch.rs`, `connectors.rs`, `inv1_boundary.rs` `kill_switch_stops_actions_and_release_resumes_nothing`; the relay part with the relay)

## Building connectors

- [ ] One path: define, build with the toolkit, prove with the required tests, review, publish signed

### What every connector is made of

- [ ] Each connector in its own directory in the `magnus-connectors` repository (today `catalog/*.toml` in this repository)
- [ ] `manifest.toml`, the source of the signed manifest (today one TOML file per connector)
- [ ] `listing/`: icon, the one-sentence summary, a plain line per scope (summary and lines are in the manifest; no icon)
- [ ] `tools.toml`: every tool's class (in the manifest's `[tools]`)
- [ ] `server/` for brokered connectors
- [ ] `addon/` for FreeCAD and SolidWorks, a pinned vendor package for Blender
- [ ] `detect.toml` for local connectors
- [ ] `tests/`: the required suites
- [ ] `OWNERS`: the developer responsible
- [x] Listing copy passes the no-protocol-words rule (`every_manifest_is_well_formed`)

### The connector toolkit

- [ ] `magnus-connector` CLI: `new`, `dev`, `test`, `check`, `submit`
- [ ] Brokered SDK: TypeScript on the official MCP TypeScript SDK; injected token, egress-only HTTP client, schema validation, provider errors mapped to tool errors; reproducible images
- [ ] Local templates: a Python workbench for FreeCAD, a C# COM add-in for SolidWorks
- [ ] Development appliance wired to the staging relay; developers connect with provider test accounts; no production secret on a developer machine
- [ ] An inspector listing tools, calling them, showing each call's policy decision and event-log entry; the desktop app in development mode for local connectors

### Building each kind

- [ ] Remote: confirm the provider's server covers what users need, else build Brokered
- [ ] Remote: record the registration mechanism; open the registration item where Alpha Vector's client is needed
- [x] Remote: classify every tool the server lists and write the plain line for every scope (the four Google manifests; `each_google_manifest_names_exactly_the_tools_its_live_server_lists`, `every_manifest_is_well_formed`)
- [ ] Remote: tests that call each tool against a provider test account
- [ ] Brokered: one tool per operation; minimum scopes; every API host declared; the registration item opened
- [ ] Local: detection rules and enable method per OS, supported versions; the add-on built or pinned with its hash, installed only when it matches a signed manifest; tools that execute code marked

### Required tests

- [x] Manifest check, every commit: manifest, listing and tool classes valid; every scope has its line; every tool a class; no protocol words (`catalog.rs` `every_manifest_is_well_formed`)
- [ ] Sign-in, every release: through the staging relay with a provider test account a live connection results, refresh works, disconnect revokes at the provider (`google.rs` stops before the token exchange)
- [ ] Contract, every commit and nightly: every tool against a provider test account; schemas; provider errors and rate limits reach the agent intact
- [ ] Egress, every commit: a brokered image fails on any undeclared host
- [ ] Tool drift, nightly: the live tool list matches `tools.toml`, and a new or changed tool opens a classification task for the owner (the match is `google.rs` `each_google_manifest_names_exactly_the_tools_its_live_server_lists`, run per CI build, not nightly; no task is opened; Hugging Face's tools listed to anyone are checked by `huggingface.rs` `every_tool_hugging_face_lists_without_a_sign_in_is_classed`, those a person turns on are not; Sentry, GitLab, Supabase and Cloudflare list their tools only after a sign-in, so their manifests are unchecked until a test account exists)
- [ ] Program, every commit and nightly: detection, enabling and every tool on real Windows and Linux installs per version
- [ ] Policy, every commit: each tool allowed, refused or held as its class and the default policy require (general boundary tests only, `inv6_policy.rs`; none per connector)
- [ ] Isolation, every commit: one person's run never reaches another's connection or token (`connectors.rs`); results stored and recalled per person; the kill switch and a turned-off account stop every call (turned-off part missing)

### Review and publishing

- [ ] `magnus-connector submit` opens a pull request and CI runs every suite
- [ ] A second developer's review: minimum scopes, irreversible and financial tools classed, plain accurate listing, minimum egress hosts, the provider registration cleared; a security review of add-ons that run code
- [ ] On merge, CI asks the catalog signing service (key in a hardware security module, signs only reviewed CI builds of the protected main branch)
- [ ] The signed manifest goes to the staging catalog; staging appliances pull it and the sign-in suite runs there
- [ ] The same manifest, byte for byte, promoted to the production catalog
- [ ] Withdrawal by a signed withdrawal in both catalogs; key rotation through a successor the current key signed (proposed, not ruled)
- [ ] Client IDs and secrets never in the repository or manifest; the relay supplies the client per environment (client IDs are in `catalog/registrations.toml` today)

### Versions and upkeep

- [ ] A version adding scopes is minor, approved the first time a tool needs it; removing a tool or scope takes effect on landing; Remote to Brokered is major
- [ ] The `OWNERS` developer receives nightly failures and tool-drift tasks
- [ ] A failing nightly contract run is fixed, or the connector withdrawn while user tasks fail

## Provider registrations

- [ ] Google: a verified OAuth client for the Workspace MCP servers (client ID in `catalog/registrations.toml`, a Desktop client; its secret provisioned to strix-alpha by CI, HTTP 204 on 2026-11-02); a configured Chat app for the Chat server; Gmail restricted-scope verification and a CASA assessment, yearly; until then 100 users and weekly re-sign-in in Testing
- [ ] Slack: an app listed in the Slack Marketplace (client ID in `catalog/registrations.toml`, "Magnus (Veyra workspace)", confidential; the secret is the repository's `SLACK_CLIENT_SECRET`, provisioned like Google's; the manifest is brokered over the Web API, not Slack's hosted MCP server). PKCE stays off. Token rotation stays on. The one redirect, which a distributed app can use, is `https://magnus.pyrallon.ai/oauth/slack/callback`. Remove `http://127.0.0.1:17321/callback` from the Slack app before turning distribution on: Slack requires every redirect of a distributed app to be https. The appliance sends the browser on to that loopback address, where the desktop is listening. The edge must not ask the browser for a device certificate or an Access login on that path (DECISIONS.md). Slack's "Enable Slack MCP Server" switch is not required.
- [ ] Microsoft: a multi-tenant Entra app with publisher verification and the Graph permissions (a public client ID exists; no manifest, no brokered servers)
- [ ] GitHub: a GitHub App or OAuth App for the remote server (an OAuth app, confidential; its ID and secret provisioned by the release, `catalog/github.toml`; its callback must be `http://127.0.0.1/callback`); org owners approve it for restricted orgs
- [ ] Vercel: a place on Vercel's allowlist of approved MCP clients (a public client, "Magnus by Alpha Vector", `catalog/vercel.toml`; its callback must be `http://127.0.0.1:17321/callback`)
- [ ] Atlassian: the relay callback on Atlassian's supported client domains
- [ ] Asana: a registered client ID and secret for the V2 server (the repository's `ASANA_CLIENT_ID` and `ASANA_CLIENT_SECRET`, provisioned by the release; the catalog names no ID). Its redirect must be `http://127.0.0.1:17322/callback` until the relay cutover (decisions/2026-11-10-asana-and-shopify.md)
- [ ] Shopify: setup.shopify.com signs clients in only by a client ID metadata document (not built) or a redirect on Shopify's allowlist; `catalog/shopify.toml` is held until then
- [ ] Sentry (in the launch catalog; a client ID exists, "not yet published", confidential)
- [ ] The public material at pyrallon.ai: homepage, privacy policy, terms, support contact, and Google's demo videos
- [ ] Providers accepting Client ID Metadata Documents (Autodesk's cloud Fusion servers) need no registration (CIMD not built)

## Launch catalog

Each entry needs its manifest, its sign-in working end to end, and its tests.
The mockup draws Gmail, Google Drive, Google Calendar, Outlook Mail, Slack,
Notion, GitHub, Vercel, Jira, Figma, Autodesk Fusion Data, Blender and
FreeCAD.

Google (Remote, Google Workspace MCP servers)
- [ ] Gmail (brokered, manifest `google-gmail`: the image built from `brokered/gmail`, tools matched by `every_tool_is_classed_in_the_manifest`, sending only a draft the person approves, `gmail.rs`; the desktop app returns Google's `iss`, and no test exchanges a code with Google)
- [ ] Drive (manifest `google-drive`; same)
- [ ] Docs
- [ ] Sheets
- [ ] Slides
- [ ] Calendar (brokered, manifest `google-calendar`: the image built from `brokered/calendar`, tools matched by `every_tool_is_classed_in_the_manifest`, each change asked for first, `calendar.rs`; sign-in cannot complete from the desktop app)
- [ ] Chat (brokered, manifest `google-chat`: the image built from `brokered/chat`, tools matched by `every_tool_is_classed_in_the_manifest`, each message sent, edited or deleted asked for first, `google_chat.rs`; live account currently returns "Google Chat is turned off"; requires Chat enabled, consent for the requested access and the project's Chat app configured for writes)
- [ ] Contacts

Microsoft (Brokered over Microsoft Graph)
- [ ] Outlook Mail (in the mockup, "by Alpha Vector")
- [ ] Outlook Calendar
- [ ] OneDrive
- [ ] SharePoint
- [ ] Teams
- [ ] Word, Excel and PowerPoint files

Microsoft Work IQ (Remote; only tenants with a Microsoft 365 Copilot license that enabled them)
- [ ] Mail
- [ ] Calendar
- [ ] Teams
- [ ] SharePoint and OneDrive
- [ ] Word
- [ ] Copilot Search

Messaging
- [ ] Slack (brokered, manifest `slack`: the image built from `brokered/slack`, over Slack's Web API, user token only; search, read and post, posting asked for first, `slack.rs`. Sign-in is `https://magnus.pyrallon.ai/oauth/slack/callback`, handed to the desktop at `http://127.0.0.1:17321/callback`, no PKCE. The hosted MCP server is not called)

Code and deploy (Remote)
- [ ] GitHub (in the mockup)
- [ ] GitLab
- [ ] Vercel (in the mockup)
- [ ] Supabase
- [ ] Cloudflare
- [ ] Sentry

Work management (Remote)
- [ ] Notion (in the mockup, drawn needing attention)
- [ ] Linear
- [ ] Asana (manifest `asana`: the V2 server, Alpha Vector's client at `http://127.0.0.1:17322/callback`, all 44 tools classed; no sign-in test against Asana)
- [ ] Shopify (manifest `shopify`, held: all 37 tools of setup.shopify.com classed; sign-in needs client ID metadata documents)
- [ ] Jira (in the mockup, "by Atlassian")
- [ ] Confluence
- [ ] monday.com
- [ ] Todoist
- [ ] Airtable

Files
- [ ] Box (Remote)
- [ ] Dropbox (Brokered)

Design
- [ ] Figma (Remote; in the mockup)
- [ ] Canva (Remote)
- [ ] Autodesk Fusion Data (Remote; in the mockup)
- [ ] Autodesk Fusion Compute (Remote; Linux users' Fusion)

Sales and payments
- [ ] HubSpot (Remote)
- [ ] Stripe (Remote)
- [ ] Salesforce (Brokered)

Meetings
- [ ] Zoom (Brokered)

Desktop programs (Local)
- [ ] Autodesk Fusion
- [ ] Blender (in the mockup, "Blender 5.2 is on this computer", Enable)
- [ ] FreeCAD (in the mockup, connected, "FreeCAD 1.1 on this computer")
- [ ] SolidWorks (in the mockup's chat: not on this computer)

- [ ] Each Remote entry confirmed against the provider's current server before its manifest is published, moving to Brokered if it does not cover what users need (done for the four Google servers, `each_google_manifest_names_exactly_the_tools_its_live_server_lists`)

## Out of scope

- Agent authorization product (task-scoped capabilities, attenuation, a universal layer): its own spec.
- Signing in to Magnus: Magnus's identity work.
