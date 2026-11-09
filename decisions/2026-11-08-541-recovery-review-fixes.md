# Recovery review hardening (2026-11-08, #541)

- No trusted link base, no recovery mail: `[mail].link_base` → pairing hostname, and nothing else. The request Host is attacker-controlled, so it never builds a link, even for local requests — tests set `link_base` explicitly.
- `remove_person` deletes the recovery token row, and confirming checks the person record exists (not the status: command-line people carry no status row). Orphaned tokens are spent on sight.
- POST /reset is exempt from the cookie guard: the emailed token is the authorization, which a plain form can carry and a custom header cannot.
- Delivery runs in a spawned task behind the `{ok: true}` answer, so active addresses answer no slower than unknown ones; send failures stay in the log, as before.
