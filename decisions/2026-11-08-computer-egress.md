# A computer's only way out is Magnus's proxy

**No route to drop.** A computer runs in its own network namespace holding one TAP device and nothing else. There is no route, bridge, NAT or firewall rule that could be wrong, because nothing in the namespace reaches the rest of the host. A guest with root can rewrite its own routes, addresses and resolver and still has nowhere to send a packet except the gateway.

**The forwarder.** `magnus-computerd forward` listens on the gateway address inside the namespace, drops to the computer's own uid, and carries each connection, byte for byte, to Magnus's proxy over a Unix socket. It does not parse anything, and stops when the VM is gone. With Magnus down the connection simply closes. A Unix socket crosses network namespaces, so no address on the host is ever opened to a guest.

**Who is asking.** The proxy reads the uid of the forwarder from the socket, and computerd says which computer has that uid. Nothing the guest sends can claim to be another computer.

**TLS.** Each computer has its own CA, kept in the vault and generated on first use. Its certificate goes into the guest's trust store on every cold boot, and the proxy ends the guest's TLS with a leaf for the name in the CONNECT. Upstream TLS is checked against the public roots.

**One lookup.** The proxy resolves a name once, refuses if any answer is private, and connects to that address. A redirect comes back to the guest and is checked as its own request.

**The audit table.** `computer_egress`, keyed by computer, separate from the log, with a row per request: `intent` before anything is sent, `result` after, `refused` for refusals. If the intent row cannot be written the request is not sent. Magnus's role can insert and read and cannot change a row.

**Declared destinations** (`policy.rs`): registries for GET and HEAD only, hosts an app declared at install, destinations added by a person, and pages opened with the resources their markup names, for GET and HEAD and only while open. A page's resources are learned from the response the proxy itself carried. Anything else is refused at once with the row's id in `X-Magnus-Refused`.

**Not here yet.** Adding a destination is a person's API call until Auto Review arrives in step 5. The source and hash of an install are not yet checked against what the proxy saw. Body inspection waits for the outgoing classifier in step 8. Only ports 80 and 443 are carried, and non-HTTP protocols end at the TLS handshake.
