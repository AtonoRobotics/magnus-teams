# crates/magnus: the kernel. Every action passes the boundary before it runs.

## Where things go
| If you need to... | Do this | Not this |
| --- | --- | --- |
| name who a send goes to | read the recipients from the draft (`destinations()`) | take them from the model's text |
| decide whether an address is the person's own | `tools::addresses_in`, whole addresses only | cut an address out of longer text |
| check a URL a tool will fetch | the web module's address check, which handles `::ffff:` forms | compare host strings |
| read an app through the person's own sign-in | a read, which asks no card under any taint | treat it as sending |

## Do not
- Count a tool result, the model's text or a schedule's text as something the person wrote (#176).
- Add a boundary action without a log row before it opens (invariant 3).
- Let a connector read through the person's own sign-in ask a card (#175).

## Checks
`cargo test -p magnus --lib`. `tests/invariants/` is protected and needs the owner to change.
