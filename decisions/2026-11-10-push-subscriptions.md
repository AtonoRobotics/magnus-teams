# Magnus subscribes to providers' notifications itself, on the server's public address

The teams server is public at `https://teams.pyrallon.ai` (a Cloudflare tunnel). Push (2026-11-09-app-push.md) worked only if someone copied a push address into the app, and the address the app copied began with whatever address the desktop app used, often a network address no provider can reach. Nothing subscribed for anyone.

**The public address is config.** `[appliance] public_url` is optional and checked when the config is read: `https://`, a host on the internet (not loopback, a private range or a single-label or `.local` name), and no user, query or fragment. A bad value stops the server from starting. Reason: config is typed and validated at startup, and an address no provider can call would fail quietly at every subscription.

**The push address is built on it.** `GET /v1/connectors/{name}/push` answers `url` (the whole address on `public_url`, or null without one), `path` as before, and `automatic`. An app's card says how its instant updates come (`instant`): `automatic`, `address` (copy it into the app), `needs_public_url`, or `none`. Without a public address, the app's page says instant updates need one, and copying is refused. Reason: an address on a network address reaches no provider, so offering it misleads.

**Magnus subscribes where the provider lets a person subscribe an https address to their own changes with their own connection.** A sweep (once a minute, from the proactive sweep) brings each person's subscription to each such app to what it should be: made once they are connected (and on start, for apps already connected), renewed with less than 24 hours left, made again when `public_url` changes or the subscription has ended, and deleted on disconnect (before the sign-in is revoked, so it can be) or when the app is turned off or removed. A failure waits an hour before the next try. Every step is a `push` event (`subscribed`, `renewed`, `unsubscribed`, `subscribe_failed`), new changes of the existing kind. The push address itself is never logged: it is a credential. A push is still only a hint that polls; nothing a provider sends is read.

The subscription calls are made by the server, with the person's access token from the vault (`connector_secret`), to fixed provider addresses, and send and receive only subscription metadata. Reason: the brokered microVM exists to keep the person's content away from the kernel; a subscription call carries none, and a remote MCP app (Drive) has no server of ours to make it in.

| App | How | Lifetime asked | Automatic now |
|---|---|---|---|
| Outlook | Graph subscription, `me/mailFolders('inbox')/messages`, `created` | 10,000 min (Graph's most is 10,080) | yes |
| Outlook Calendar | Graph subscription, `me/events`, `created,updated,deleted` | 10,000 min | yes |
| OneDrive | Graph subscription, `me/drive/root`, `updated` (the only one a root takes) | 42,000 min (most 42,300) | yes |
| Google Calendar | `events.watch` web_hook channel on the primary calendar; renewed by a new channel and `channels.stop` on the old | under a week; Google keeps the shorter of that and its own limit, and Magnus keeps what it answers | yes |
| Google Drive | `changes.watch` from `changes.getStartPageToken`; renewed likewise | under a week (Drive's most for changes) | yes, if the Drive API is on in the Google client's project |

Graph checks the address with `validationToken` before it answers, which `push::receive` already echoes. Google's current push guides for Calendar and Drive ask only for https with a valid certificate; the domain verification they once asked for is no longer listed.

**Not subscribed, and why.**

- **Gmail**: Gmail's `users.watch` publishes to a Cloud Pub/Sub topic, never to an address. It needs a topic in the Google client's project with `gmail-api-push@system.gserviceaccount.com` as publisher, and a push subscription to the server. The topic gets every mailbox's notifications, so its one address cannot be a person's: routing them needs the account named in the (authenticated) message, which the push rule does not read today. Polled.
- **Google Chat**: the Workspace Events API also delivers only to Pub/Sub. Polled.
- **Slack**: the Events API sends every user's events to the one Request URL in the Slack app's settings, so a person's push address cannot be registered. The same routing question as Gmail. Polled.
- **Asana**: the token is one from Asana's MCP app, and Asana says those work only with its MCP server, not its REST API, where webhooks are. Subscribing would need a second, REST sign-in. Polled.
- **Vercel**: webhooks belong to a team, not a person (Pro and Enterprise teams, at most 20 per team), so making one changes the team's settings for everyone; and whether the Vercel MCP sign-in's token reaches the REST API is unverified. Polled.
- **GitHub**: the watches cover issues and pull requests involving the person across every repository; GitHub has webhooks per repository and organisation only, needing admin rights on each, and none for a person's notifications. Polled.

**Owner steps.** Set `public_url = "https://teams.pyrallon.ai"` under `[appliance]`. The tunnel must let providers' POSTs reach `/v1/push/` without a challenge (no Bot Fight Mode, Access or WAF challenge on that path), and `/robots.txt` must not disallow it, since Google's notifier (`APIs-Google`) respects robots.txt. Make sure the Google Drive API is enabled in the Google Cloud project of the Magnus Desktop client.
