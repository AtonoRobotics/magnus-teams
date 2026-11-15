# The apps' code in modules, its sign-in checks pure

**Choice.** `connectors.rs` (1570 lines, on the oversize list) is a root of 230 lines over `connectors/`: `discovery` (fetching metadata, and the pure `check_resource_metadata` and `check_server_metadata`), `sign_in` (registration, the sign-in and its completion, and the pure `authorize_url`, `scopes_asked`, `check_answer_issuer`, `token_form`, `granted`, `fresh_tokens`, `dead_token`, `account_of`), `standing` (the person's token and its refresh, where they stand, disconnect, revoke), `admin` (the catalog as an admin keeps it), `slack` (the https callback) and `routes`. `random`, `challenge` and `loopback`, which `oidc` and the relay share, are `oauth.rs`. Each pure check has a test that fails without it: the PKCE challenge is the verifier's S256 and the verifier goes to the token endpoint only with it, the resource indicator only when there is one, an answer from another issuer or a missing `iss` from a server that sends one is refused, metadata for another resource or issuer is refused.

The sign-ins waiting for their code were a process-wide map (`pending()`); they are `SignIns`, a field of the kernel, so each server (and each test server) holds its own. The Slack callback reads it through the kernel.

`POST /v1/connectors` and `PUT /v1/connectors/{name}/tools/{tool}` take fixed shapes (`requests::AddConnector`, `requests::Classify`); the control API's `POST /v1/admin/connectors` reads the same shape. A body that does not fit is a 422 `{"error": ...}`.

**Reason.** One file held discovery, OAuth, token storage, the admin's catalog and the routes; the security checks of a sign-in sat inside network code, where no test reached them without a provider. Split, each RFC check is a function with its own test.

**Tightened.** A body read with `Value` took a wrong type as empty:
- `url` is required (it was read as empty, then refused as "not a URL", 400): now 422.
- `name`, `audience`, `label` and `token` that are not text, `access_groups` or `blocked_groups` that are not lists of text (non-text entries were dropped), and `system_of_record` that is not true or false (read as false): now 422.
- A `risk` that is not text was read as null, which stops offering the tool: now 422.
Missing `audience` and `label` keep their own 400s, which name what they must be.

**Follow-up.** The relay's waiting sign-ins (`relay::sign_in::waiting()`) and the one-refresh-at-a-time lock (`connectors::refreshing()`) were process-wide too. With `SignIns` they are now one kernel field, `connectors::Connecting { sign_ins, relayed, refresh }`, so two servers in one process (the tests run many) neither see each other's sign-ins nor wait on each other's refresh (`relay::tests::two_servers_in_one_process_keep_their_own_sign_ins`). `payments` keeps globals of its own (`payments/routes.rs` `sign_ins()`, `payments/link.rs` `refreshing()`), left for its own change.
