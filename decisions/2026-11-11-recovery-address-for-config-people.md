# A recovery address for people named only in the config

A password recovery request finds its person by the address in the
directory (`account_people`). The server's owner is named in the config's
`[[people]]` and had no directory row, so "Forgot password?" could never
reach them however mail was set up.

The control socket's `PATCH /v1/admin/people/{id}` now takes `email`: a
string sets the recovery address, null or "" clears it. A person with no row
gets one under their config name; an existing row keeps its name. The
address is checked as an invitation's is.

This lives on the control socket, not the app API, because only the
server's owner (someone with the machine) should be able to point another
person's recovery at a new mailbox; that is a takeover path otherwise.
`add_person` moved beside it into `accounts/directory.rs`, which keeps the
oversized `accounts.rs` from growing.
