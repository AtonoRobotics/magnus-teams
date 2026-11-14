# Routes take a caller and return a result

**Choice.** A route names who may call it in its arguments, and returns `ApiResult`:
- `extract::Caller` (anyone signed in), `Owner` (the server's owner), `Developer` (the developer view), `AppSession` (the app's own bearer session) resolve the caller once, before the route runs. `team::caller::Member<at::Guest | Member | Admin | Owner>` is a member of the path's `{ws}` with at least that role.
- `api_error::ApiError` is the one refusal: `?` on an `anyhow` error gives what `internal` always gave (the kill switch's or the spend limit's own refusal, else a 500 `internal error`), and `refuse(code, msg)` gives what `err` always gave, `{"error": msg}`. Status codes and bodies are unchanged, so the app's one error policy (`desktop/ui/server.js`) reads them as before.
- `extract::Body<T>` takes a body of a fixed shape. One that does not parse, has the wrong type or is not JSON is refused as `{"error": ...}` with axum's status (400, 415, 422), not read as an empty string. Bodies are typed where the shape is fixed: the connector registration (`client_secret` required), sign-in and sign-in completion.
- What a workspace member may do beside acting on an entry is `team::may::act` (`Act::Post { with_agent }`, `Remove`, `DeleteFile`, `LinkChannel`): each team route makes one call, `allow(role, me, act)?`, and the guest and admin rules are no longer written inline.
- The copied early-return macros are gone: `person!` (chat actions, trash, computer routes), `found!`, `owner!` and `curated!`.

**Reason.** The same four lines resolved the caller in 37 routes in `api.rs` alone and three macros copied them; the guest rules were written again in each team route although `team::may` decided message actions; and a body read loosely turned a missing field into an empty one (`client_secret` stored as `""`). An extractor cannot be forgotten, and one refusal type keeps every error the app reads the same.

**Changed with it.**
- A request with no valid sign-in and a malformed body is now refused 401 for the sign-in, not 400/422 for the body: the caller is resolved before the body is read. Safer: a stranger learns nothing about the body's shape.
- A body that is not JSON, or not the route's shape, answers `{"error": ...}` instead of axum's plain text.
- `PUT /v1/registrations/{provider}` without `client_secret` is refused (422) instead of storing an empty secret.
