# #533 password recovery via email (2026-11-08)

- `accounts.rs`: `account_reset_tokens` table (rides the accounts dump), `request_reset`/`confirm_reset` (+ `_at` variants), `reset_live` peek.
- New `mailer.rs` (lettre 0.11, rustls) + `[mail]` config (host/port/tls/username+credential/from/link_base); `lettre` added with default-features off.
- `api.rs`: `POST /v1/password-reset/request`, `GET /reset`, `POST /reset` (form), all behind `over_tls`.
- Harness: `Spec.mail` config support; `mail::Inbox::start_local` runs mail-sink as a host child (no Keycloak container needed).
- Tests: `reset_round_trip_sets_clears_and_consumes` unit (green locally, 1.3s); `password_reset.rs` e2e (mailed link round trip through the sink) runs in CI per user preference.
- Next: #535 (console invites email again via the mailer), then #534 (approval signup).
