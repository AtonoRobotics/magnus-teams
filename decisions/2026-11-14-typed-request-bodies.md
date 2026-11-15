# The server's own routes take typed bodies

**Choice.** Seventeen of the 24 routes in `api.rs` that read a `Json<Value>` now take `extract::Body<T>`, with `T` in `api_bodies`: enrolling a device, signing in, choosing a password, starting a device or browser sign-in and finishing it, accepting an invitation, a verdict, the inbox items opened, a policy layer, a secret, the kill switch, an adapter, its conformance check, transcribing, speaking and the default model. A field the route checks itself, with its own refusal (`"name the redirect address"`, `"no value"`, `"verdict is right, partly or wrong"`), is an `Option`, so that refusal stays the one the app reads. Seven stay `Value`, each for a reason written in `api_bodies`: the task and chat routes (`submit`, `chat_post`) and the schedule routes (`add_schedule`, `edit_schedule`) hand the body to helpers that read the same keys for the agent's tools; a card's or an inbox item's answer (`chat_card`, `respond`) depends on its kind; an adapter's sign-in completion passes the adapter's own `flow` and `input` through.

**Reason.** Reading `b["x"].as_str().unwrap_or_default()` turns a missing field into an empty one, and the route then acts on the empty value: a policy layer written with no text, a sign-in tried with an empty password. A typed body refuses what the route cannot act on, as `{"error": ...}` (`2026-11-14-handlers-take-a-caller.md`).

**Tightened.** Each of these was read as empty and is now refused with 422 and `{"error": ...}`:
- `POST /v1/sign-in` without `person` or `password` (it answered 401, wrong password).
- `POST /v1/password` without `person`, `password` or `new_password`.
- `POST /v1/invites/accept` without `token` or `password`.
- `POST /v1/devices` without `name` or `csr` (the device check refused the empty one with 400).
- `PUT /v1/policies/{layer}` without `cedar`: it wrote the layer with empty text.
- In every typed body, a field of the wrong JSON type (a number for a string, a non-string in `ids`, a string for `engaged`) and a body that is not an object, which were read as absent.

**Kept.** A kill switch request that does not say `engaged` engages the switch, as before. An adapter without `settings` gets `{}`, and one with `"settings": null` keeps null, as before. The app sends every required field already (`desktop/ui/bridge.js`, `server.js`, the Tauri enrolment).

Tested in `api_bodies::tests::a_missing_field_is_refused_not_read_as_empty`.
