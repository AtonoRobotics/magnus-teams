# #210: recipient provenance from sent mail

The spec's second provenance source: under both taints, a send to an
address the person has mailed before passes with no card. The person's
own words stay the first source; the directory (#211) is still to come.

## Shape

Each mail connector declares, beside `destinations`, a `[sent]` search:
a read tool, the input field, a query template with `{address}`, and a
`found` pointer whose non-emptiness means mailed before. Gmail's is
`search_threads` with `in:sent to:{address}` read at `/threads`.

## Gate behaviour

For each recipient the person did not name, the gate runs the sent
search through the person's own sign-in, sharing the transport call
with `destinations()` (extracted byte-identical). Whatever stops the
read counts as not mailed: the send asks rather than passing on an
unchecked claim. No declaration, no waiver — connectors without `[sent]`
behave exactly as before.
