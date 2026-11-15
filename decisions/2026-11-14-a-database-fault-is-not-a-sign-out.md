# A database fault is not a sign-out

**Choice.** `Accounts::session` tells "no such session" (`Ok(None)`) apart from "the store could not answer" (`Err`). A signed-in request whose session the store cannot read gets 503 with `{"error": "The server could not check your sign-in just now. You are still signed in; try again in a moment."}`. The fault is logged (`api_error::session_unread`), and the session is kept. Only a token with no live session gets 401 "sign in again". The app follows the same rule:
- `Server.call` passes a 503 on as an error with the server's message, and a change answered 503 is "Outcome not confirmed".
- Only a 401 ends a session in `app.js`.
- At launch, a 5xx from `/v1/me` opens a "The server cannot answer just now" screen with Try again. It does not open sign-in. This holds for the app's `restore` (which keeps the token) and the browser's.

**Reason.** A failure is never silent (K24). Before this, any store error (a dropped connection, a failed query) read as no session. The person got 401 and the app signed them out, which blamed them for the server's fault and threw away a live session. The owner decided on 2026-11-14 that a database fault must not sign people out.

**Changed.** Requests made while the account store cannot answer get 503 instead of 401, both on routes that need a signed-in person and on those only the app's own session may use (`extract::app_session`).

Tested in `admin_app::session_fault_tests::a_session_the_store_cannot_read_is_503_and_stands`, which fails without the change (401), and in `desktop/tests/server-check.mjs`.
