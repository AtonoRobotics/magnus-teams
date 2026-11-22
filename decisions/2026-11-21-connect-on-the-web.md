# Connecting an app from a web browser

## Choice

- **Before:** in a browser, Connect on an app said "Connect apps from the
  Magnus desktop app." and did nothing else. Only the desktop app could
  connect an app, because it catches the sign-in's answer on a loopback
  address on the person's computer, and a browser has nothing listening
  there.
- **The way back is the server's own address.** In a browser, Connect goes
  to the app's own sign-in page in the same tab. The redirect is
  `<public_url>/connected`, for production `https://teams.pyrallon.ai/connected`.
  That path serves the app itself (`connectors/web.rs`, with
  `Referrer-Policy: no-referrer`). The app then:
  1. takes the answer out of the address before anything else reads it
     (`connect-web.js`);
  2. checks it against the sign-in this tab started, kept in the tab's
     session storage for the server's 15 minutes;
  3. finishes it through the existing `sign-in/complete` route, in the
     person's own session;
  4. goes back to where they connected from.

  The session cookie (SameSite=Strict) is not sent on the way back from
  another site. The cookie is sent with the page's own requests that
  finish the sign-in, so the "only the person who started it can finish
  it" check is unchanged.
- **The server accepts that redirect** only for an app whose provider
  takes an address Magnus names, that is, an app Magnus registers its own
  client with (dynamic client registration). The redirect must be exactly
  the public address's `/connected`. A browser on the server's own computer
  uses its own loopback `/connected`, which was already accepted. Apps added
  by their MCP address, GitLab, Hugging Face, Supabase, Cloudflare and
  Sentry connect this way.
- **Apps whose provider client was registered beforehand** (Google's
  desktop client, Microsoft, GitHub, Slack, Asana, Vercel, Shopify) list
  only the desktop's addresses with their provider, so they still connect
  from the desktop app. In a browser they now say why: "Gmail can be
  connected only from the Magnus desktop app: its sign-in comes back only
  to the desktop app." The card carries `web_sign_in` and `web_return`.
- **The same applies to a workspace's own sign-in**, which a Team Admin
  makes from the workspace's Apps.

## Reason

The owner: "App connectors don't authenticate on web." The connectors spec's
long-term path is the relay
(`decisions/2026-11-13-relay-handoff-server-side.md`), which does not exist
yet. Until it does, the server's own address is a return a provider can be
told, and the answer only ever passes through this server and the person's
own session.

Adding `https://teams.pyrallon.ai/connected` to Google's, Microsoft's,
GitHub's, Slack's, Asana's and Vercel's consoles would let those apps
connect from a browser too. Each change is in a provider console and
needs a web client where the client is a desktop one (Google). That is
the owner's to do. The server then needs only to list those providers as
taking the address.

## Verification

- `connectors::web` unit tests: only an app that takes a named address
  returns to the server, and the return page is the app and sends no
  referrer.
- `tests/scratch/tests/connect_web.rs` (`desktop/tests/connect-web.mjs`),
  in the real app in a plain browser: Connect goes to a fake app's sign-in
  page, which sends the tab back to `/connected` from another site, and
  the app is connected. The code leaves the address, and the person is
  back on Apps. A forged answer connects nothing. An app that comes back
  only to the desktop app (Vercel there) says so and goes nowhere. On the
  code before this change the same click only said to use the desktop app.
