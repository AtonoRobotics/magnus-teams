# What Magnus says to computerd is typed, once, for both (2026-11-14)

Magnus built every computerd request by hand, `json!({"op": ...})`, at about
fifty places in `crates/magnus/src/computer/`. Computerd had its own typed
`Request`, which Magnus did not use. A misspelt op or field compiled, and
reached computerd as "bad request". The tests had three fake computerds of
their own, and each answered a little differently from the real one.

- A new crate, `crates/computerd-proto` (`magnus-computerd-proto`), holds the
  requests: `Request` (computerd's 26 ops, with `OPS`, `Request::op()` and
  the `version` answer, `VersionReply`) and `guest::GuestRequest` (the 11 ops
  of the guest agent that Magnus sends through `connect`). Computerd's server
  reads `Request` from it, and Magnus's client writes it. Reason: the wire is
  defined once, and an unknown op cannot be written.
- The wire does not change. Each typed request serialises to the JSON the
  old `json!` sent, and computerd reads that JSON back as the same request; a
  test holds every op to it, with the exact line for a few. Reason: computerd
  is root's and updated apart from the server
  (`decisions/2026-11-11-computerd-version-and-update.md`), so an updated
  server must still talk to the computerd it finds, and the other way round.
  A new op still needs computerd to list it in `version` before it is sent.
- Magnus reaches computerd only through `computer::client`: `ask(socket,
  Request)`, `guest(socket, id, GuestRequest)`, `guest_for` and `display`.
  It hides the socket, the op names, the version check and the guest's
  reopening. It stays functions keyed by the socket path, not a trait object
  in the kernel. Reason: the socket path is already the key of what computerd
  said it serves, and the seam that matters is the socket, where both the
  real computerd and the fake speak the same types.
- Answers stay JSON values, read leniently, except `version`. Reason: a newer
  computerd may add fields or states, and an older server must not fail on
  them. What a guest sends back is untrusted and is checked where it is used.
- A browser VM's requests stay a JSON value (`client::guest_value_for`).
  Reason: they are mostly the model's tool arguments, passed through.
- The tests share one fake, `computer/fake_computerd.rs`. It reads the first
  line of each connection as `Request`, so an unknown op is refused as
  computerd refuses it, and answers guest lines as `GuestRequest`. A script
  makes it full, old, unclean on handback, or a relay that drops a
  connection. Reason: three fakes had drifted from computerd and each other.
- The test that sent `"a_request_from_the_future"` cannot be written any
  more. Computerd's server now tests that such a request is refused with
  "bad request", the words Magnus reads as an older computerd. Magnus's test
  for a request computerd does not list now uses `erase_volume`.

Not done: the guest agent (`sandbox/guest`) still reads its requests as JSON
values, and the attachment sandbox (`sandbox.rs`), which reaches the same
agent straight over vsock, still builds its own. Moving them to
`GuestRequest` changes the guest image, so it is left for its own change.
