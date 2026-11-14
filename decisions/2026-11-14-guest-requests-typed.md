# The guest agent reads typed requests, and no one reads its answer unbounded (2026-11-14)

The guest agent (`sandbox/guest`) read each request as a JSON value and each
field with a default. A missing or misspelt field became "" or 0 and the op
went ahead: a `read` with no path read /workspace itself, a `list` with no
path listed it, a `write` with no data emptied the file, and an `exec` with
no command ran an empty shell and said it succeeded. The run sandbox
(`crates/magnus/src/sandbox.rs`) and computerd built their guest requests by
hand, and both read the guest's answer with no limit. The guest runs the
model's commands and opens attachments, so it is untrusted: one that never
ended its line grew the reader's memory until the timeout, and computerd's
reader, which is root's, had no timeout that a slow stream would reach.

- `GuestRequest` (`crates/computerd-proto`) is the guest's whole vocabulary:
  it gains `reset`, `freeze` and `thaw`, the three computerd sends, with the
  fields computerd sends. Reason: one definition for every sender and the
  guest.
- The guest parses each line as a `GuestRequest`. An unknown op, a missing
  field or a field of the wrong type is answered
  `{"error": "bad request: ..."}`, the words computerd uses. A field the
  guest does not know is ignored. Every answer it gives to a good request is
  as before. Reason: a request that is wrong must fail, not act on a default;
  an unknown field must not, so a newer server still reaches an older image.
- The run sandbox and computerd send `GuestRequest`. Magnus's computer client
  already did.
- Every reader of a guest answer takes at most `MAX_GUEST_REPLY_BYTES`
  (64 MiB, now in `computerd-proto`, shared by the computer client, the run
  sandbox and computerd), and an answer that does not end its line within it
  is an error. Reason: a file read comes back whole in base64, and nothing
  else is near that size.
- The wire does not change, either way. Each typed request serialises to the
  JSON its sender wrote before, and the guest accepts every line any current
  sender writes. `crates/computerd-proto/guest_lines.jsonl` holds one line
  per request; the proto's tests hold each typed request to its line, and
  the guest's tests parse each line. Reason: running computers keep their
  image while the server updates, and a new image may meet an older server.
  Every exec any sender ever wrote carries `timeout_s`, so the guest no
  longer supplies 120 when it is missing.
- A browser VM's requests stay JSON values, and its op list stays in
  `browser.rs`. Reason: they are the model's tool arguments passed through to
  the page sidecar, which checks them itself.
- `sandbox/image.sh` hashes `crates/computerd-proto` with `sandbox/`. Reason:
  the guest is now built from it, and a change there must make a new image.

The guest side takes effect only when the image is rebuilt and a computer
boots from it. Until then old images answer as before, which the server
reads the same way.
