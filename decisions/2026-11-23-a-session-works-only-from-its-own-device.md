# A session works only from its own device

**Choice.** Through the tunnel, once device certificates are configured, a
request's live device must also hold for its session
(`devices::session_holds`): the device is the session person's own, and a
session bound to a device (pairing binds it, a sign-in through the tunnel
opens it bound) is used from that device alone. A config token's person is
held to the same first rule. A request with no session (signing in, health)
needs only a live device, as before. LAN and loopback requests keep what
`devices::admit` says: no certificate is asked for there.

**Reason.** The tunnel asked only whether the certificate was some paired
device, so a session token copied off Alice's computer worked from Bob's,
and revoking Alice's computer did not stop a copy used elsewhere. Someone
signing in through the tunnel on another person's paired computer is now
refused on their next request; the safer reading. Fourth review,
2026-11-23, item 7.
