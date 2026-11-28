# The API's JSON answers are gzipped for a client that asks

The owner, 2026-11-27: "turn on gzip for the API responses too", after a
reply showed half a minute late because each refresh downloaded megabytes
of lists over Tailscale.

Choice:

- The server gzips a JSON answer of 1 KB or more when the request accepts
  gzip (`api/gzip.rs`, a layer on the API router). Streams (the event
  stream, the screen), answers of unknown size and the UI's files, already
  gzipped, pass through untouched.
- The desktop app asks for gzip on its API calls and reads the answer back
  itself with flate2 (`gunzip.rs`), which its lockfile already holds.
  Turning on reqwest's own gzip would have added a crate the locked
  release build does not have. A browser asks and decodes by itself.
- An app from before asks for nothing and gets the answer as it was, so
  the server and the apps can update in any order.

Reason: lists compress about ten to one, and the app reads them on every
change. Smaller lists (2026-11-28, the lists carry what the app shows) were
the first fix; this is the second, for what the lists still hold.
