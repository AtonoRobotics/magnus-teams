# Browser VMs and credentials

**A browser VM is a computer with a different role.** `Role::Identity { site }` or `Role::Throwaway { run }` on the same ledger, the same computerd and the same proxy. A VM's role is set when it is created. The ordinary computer routes, the agent's desktop tools and takeover ignore anything whose role is not `Computer`.

**No agent shell.** computerd boots it with `magnus.role=browser`, and the guest then serves page commands, `ping` and `sync`. There is no `exec`, no file read or write, no script evaluation, no cookie read and no developer tools. The guest enforces this, so a mistake in Magnus cannot reach past it.

**Its way out.** The same per-VM TAP and proxy as a computer. An identity reaches its site, subdomains, and what that site's own pages load, for reading. A throwaway reaches any public web address and nothing private. Chromium ignores certificate errors because the only thing it talks to is the VM's proxy, which checks the real site's certificate.

**The profile.** It lives on the identity's own volume, and exists in plain form only while the VM runs. When the VM stops, computerd seals the volume with a key from the vault. A browser that sits unused for 10 minutes stops. It never hibernates, because a memory snapshot would hold live sessions. A kill through computerd happens without Magnus, so the idle loop seals any open profile it finds on a stopped VM.

**Chromium's own sandbox is off in the VM.** The VM is the boundary, it holds one identity, and the Firecracker kernel's namespaces were not relied on. The browser runs as an unprivileged uid.

**Sign-in order.** `browser_sign_in` uses a saved vault entry, then a secure form. There is no password manager connection yet, so that step is absent.

**The secure form.** `POST /v1/identities/{id}/secure-form` passes the value to the guest's `fill_secret` and, only if the person says so, to the vault. It is not logged, and the structure holding it does not derive `Debug`. The sidecar marks the field so a snapshot never repeats it, and removes the value from later snapshot text and URLs. A screenshot of a page that prints it back would still show it.

**Surrogates.** A person gives a credential to `POST /v1/computers/{id}/surrogates` with one host. The real value goes to the vault and the computer gets a token. The proxy replaces the token in request headers only at that host, and only after an Auto Review call allows that request. Sending it anywhere else, or a denial, or an unreachable reviewer, refuses the whole request. The review has no run behind it, so it uses the appliance's default model.

**Attachments from outside.** `from_outside` files on a new task never enter the run's workspace. `open_attachment` puts one in a sandbox of its own, runs an extractor or the agent's command, and destroys it. Its output is labelled untrusted. `POST /v1/tasks/{task}/attachments/{name}/save` is the person's way to keep the file.

**Not here yet.** The payments part waits on a card issuer. A response that echoes a real credential back is not scrubbed. Only headers are swapped. No connector or inbound email path sets `from_outside` yet: the API does. Session cookies with no expiry do not survive a VM stop, as in any browser restart.

**Three things the tests taught.** The audit row for an identity keeps the host and path and drops the query, because a secure value can land in a URL. A read at the identity's own site does not count as sending data out, or an identity could not read its own account after any untrusted page. Sealing a volume and booting one share a lock, in computerd and in the idle loop, because the two raced.
