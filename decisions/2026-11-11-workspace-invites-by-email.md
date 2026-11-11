# Workspace and server invites by email or link

Spec rev 142: Team shared workspace, "Members are invited by email or link";
Administration, People, "Invite by email or link".

## Choice

- **Workspace, by email, known address**: they join at once, as before. A
  person already in the workspace keeps their role (an invite no longer
  overwrites it). The app says they are in.
- **Workspace, by email, unknown address**: the server makes an account
  invitation through the same path the admin's People screen uses
  (`Kernel::invite_account`, `invitations::email`, the `/invite` page). The
  account is a server member, or a guest when the workspace invite is for a
  guest, never more, whoever invites. The workspace invite is bound to the
  address and its workspace role; when they choose their password on
  `/invite` they join every workspace bound to their address, are signed in
  in that browser and land in the workspace. The answer says whether the
  email went (`emailed`, `mail`, `email_error`); the app says "emailed" only
  when it was, and otherwise says why and shows the link to send.
- **Workspace, by link**: the link now points at the server's `/join` page
  (whole when the server knows its public address, else below the address
  the app talks to). Someone with no account gives name, email and password
  there, gets a server member account (guest for a guest's link) and joins
  with the link's workspace role. Someone with an account opens it in
  Magnus as before. A link works for anyone until it expires (7 days), as
  before; a code bound to an address works only for its holder and once.
- **Admin, People, by link**: with no email address, the admin gets a
  single-use link (7 days) carrying a role (owner only from an owner). Whoever
  opens it on `/invite` gives name, email and password; the account is made
  with that role and they are signed in. The log keeps the code's hash only
  (`Ev::AccountLink`, a new kind). With no public address the app says so
  instead of showing a raw invitation code.
- **Safety**: both sign-up forms are over TLS from the network, and every
  unknown, ended or spent code is counted against the address it came from
  on the sign-in lockout counters (`signup:<ip>`), so codes cannot be
  guessed at speed. A form that is authorized by its code or token (`/reset`,
  `/invite`, `/join`) is not blocked by a stale session cookie.
- A workspace admin does not choose ids: an address whose id is taken gets
  the next free numbered one (`ann-2`). The admin's own invite still refuses
  a held id.

## Reason

The spec names email and link as the two ways in, for workspaces and the
server alike. Before this, a workspace email invite to a newcomer made only
a code and nothing was emailed, and joining needed a session a newcomer
could not have; the admin screen required an address, so there was no link.
Reusing the admin's account, mail and page path keeps one way to make an
account. Capping link and workspace accounts at server member or guest keeps
a workspace admin from minting server admins.

An address typed on a link's form is not verified: anyone holding a link
the inviter handed out may give any address. The link is the authorization,
as the spec's "invite by link" implies; password recovery still goes only to
that address.
