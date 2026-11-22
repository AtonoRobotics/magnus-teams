# Registered apps connect from a web browser

## Choice

After the browser connect flow (`2026-11-21-connect-on-the-web.md`), only
apps whose provider takes an address Magnus registers itself for could be
connected from a browser. The owner: "Apps are still only able to be added
via desktop app." The provider registrations of
`https://teams.pyrallon.ai/connected` already exist ("That already exists.
You were to build the server side").

Now the apps whose client Alpha Vector registered with the provider connect
from a browser too: Gmail, Google Calendar, Drive and Chat, Outlook,
Microsoft Calendar and OneDrive, GitHub, Slack, Asana and Vercel.
- **The catalog says which client a browser signs in through.** Each entry in
  `catalog/registrations.toml` can name `web`, the registration a browser's
  sign-in goes through, and `returns`, the servers' return addresses that
  client lists with its provider.
  - Google's desktop client takes only local addresses, so a browser signs
    in through Alpha Vector's web client with Google (`google_web`, the
    repository's `GOOGLE_WEB_CLIENT_ID` and secret).
  - On Microsoft's registration, a web address is a confidential client's,
    which sends its secret at the exchange (`microsoft_web`: the same client
    ID, with `MICROSOFT_CLIENT_SECRET`). The desktop's local address stays a
    public client's, which sends none.
  - GitHub, Slack, Asana and Vercel sign in through their same client.
- **The release provisions the two new clients** with the others
  (`release/provision-registrations.sh`), printing only whether each was
  provisioned.
- **A browser is offered the sign-in only where it can finish.** That means
  the server's own `<public_url>/connected` is in the client's `returns`,
  and that client is complete on this server. Otherwise the card says to
  connect from the desktop app, as before, instead of sending the person to
  a provider that would refuse the address.
- **The server takes the return address in place of a fixed redirect** for
  these apps only. The token exchange repeats the address and uses the web
  client, whose ID and secret are kept with the tokens, so refreshes use it
  too.
- **The desktop app is unchanged.** It signs in at its local address through
  the same clients as before.

## Reason

People use Magnus from a browser as well as the desktop app, and adding an
app should not depend on which one they opened. The owner registered the
server's address with each provider. Listing that address per client in the
catalog keeps a server with another public address from sending people to a
provider error.

## Verification

- `tests/scratch/tests/connect_registered_web.rs` (real app): with the
  server's public address set, the release's own script provisions
  stand-in clients. Then:
  - each of these apps' cards offers the browser's sign-in back to
    `/connected`;
  - Gmail's sign-in goes through the web client, Outlook's through
    Microsoft's client, and Asana's and Vercel's through their own;
  - the desktop's sign-ins are unchanged;
  - another server's address is refused.

  On the code before this change, no such card offered a browser sign-in.
- Unit tests in `connectors/web.rs` check which apps return to which server,
  and that the web clients are confidential.
