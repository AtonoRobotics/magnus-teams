# Dead addresses: proxy timeout plus a deterministic DNS test (#426)

The private-name browser test flaked 9 times with `page.goto: Timeout
exceeded`. The first fix attempt, a 45s to 90s navigation budget bump,
failed in CI with the timeout exceeded at 90s too, and is reverted: a
budget cannot fix a stall longer than any budget.

Two faults, one issue, one pull request. First, every connection
Chromium makes goes through the egress proxy
(`browser/destinations.js`), whose `connect()` tried a name's
addresses serially with no per-attempt timeout: a stale address that
blackholes SYNs holds the whole navigation for the kernel's
minutes-long SYN timeout (~127s on Linux), past 45s and past 90s.
Second, the test navigated to this machine's own name, but DNS here
answers that name with a stale address the machine does not have
(`strix-alpha` resolves to 10.0.1.10, which is not on any local
interface), so the test was roulette on top of the hang.

Ranked hypotheses, before the fix: (1) proxy connect blackhole — fits
structurally, failures only where sockets open while the same name
resolves fast seconds earlier, and only a >=90s mechanism explains
90s exceeded; (2) a `getaddrinfo` stall in `destination()` — needs a
coincidental 90s resolver transient starting in a seconds-wide window
after two fast lookups of the same name; (3) a Chromium-side stall —
Chromium talks only to the 127.0.0.1 proxy for a 50-byte page;
(4) runner CPU starvation — refuted, 9/9 failures the same test and
90s is not rendering slowness. Local reproduction on the CI host
settled it: the base hangs the full 90s budget, the timeout-only fix
bounds it to 10s plus a fast proxy error.

Fix: each connect attempt gets 10s (`CONNECT_MS`), then its socket is
destroyed and the proxy moves to the next checked address; all dead
errors instead of hanging. Serial order is kept, and only checked
addresses are ever tried, so the anti-rebinding guarantee is
unchanged. 10s: a handshake is one RTT, so even a 2s-RTT link passes,
while a 4-dead scan (40s) still fits inside the 45s page budget; the
asymmetry favors longer, since too short would be a functional
regression for slow origins. `connect()` gains an `opts` ({ dial, ms })
seam so tests can drive the real control flow against a hanging dial.
And the test navigates to `localhost` instead of the machine's name:
still a name resolving to a private address, but one that resolves
everywhere. Either change alone leaves a flake (fast-fail roulette
without the rename, an unbounded wait without the timeout), so both
ride this issue's one pull request.
