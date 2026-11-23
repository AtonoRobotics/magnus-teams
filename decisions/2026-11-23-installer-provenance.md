# An installer's provenance: the decoded body, where it was asked for, kept on disk

An app is recorded only if the egress proxy carried its installer, with the
same sha256, from the host its source address names. Three faults made that
refuse good installs:

- A chunked response was hashed with its chunk framing, so its hash never
  matched the file the computer saved.
- A download that redirected (every GitHub release link does) was recorded
  under the host it ended at, not the one asked for.
- The record lived in memory, so any install after a restart was refused.

Now the proxy hashes the decoded body (a response whose chunked framing is
broken or cut short is not recorded), remembers each redirect for a short
while, and records a download under its own host and under the host the
first request of its redirect chain went to. The record is a small JSON file
per computer under the server's state directory (`computer-downloads/`, at
most 512 entries each), removed when the computer is deleted. It holds hosts
and hashes only.
