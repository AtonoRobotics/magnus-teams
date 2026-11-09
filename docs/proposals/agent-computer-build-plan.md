# Agent computer: build plan

Follows spec revision 69 and `decisions/2026-11-08-agent-computer-declared-destinations.md`. Each step lands on `main` behind a `[computer]` config section. Rules are revised on measured signals from the log, not further review rounds. No real user gets a computer until step 8 has landed.

0. **Confine the host Chromium**, as its own change.
1. **Foundations:** rulings recorded and invariant wording approved. Per-run VMs stay as they are.
2. **Computer lifecycle:** `magnus-computerd`, the root service that starts and owns VMs (`decisions/2026-11-08-computerd.md`), and a `computer/` module with an owner that is a person or a workspace. Jailer, resource caps, per-computer kill, bounded live VMs with a visible queue. VMs outlive the kernel. Separate drives for base, overlay and user volume, with no profile drive. Files through the guest agent when live and a no-network helper VM when stopped. Test 2.
3. **Rebuild path:** verified manifest with cached installers, overlays pinned to their base, snapshots, the four guest resets, cold boot after an edit while hibernated, and encrypted backups of volumes and profiles. Tests 3 to 6.
4. **Egress:** a TAP device in a per-computer network namespace, default-drop, to a host proxy with a per-computer CA. Declared destinations, a separate egress audit table keyed by computer, fail-fast asks. Tests 7, 8, 9, 31, 11.
5. **Auto Review:** a pipeline stage that asks when unavailable and acts only where owner policy delegates. Rules compile to Cedar and coverage is a deterministic match. The cache key covers destination, method, content hash, run taint and rule version. 30-minute expiry and one approval surface. Tests 12, 13, 14, 16, 17.
6. **Desktop and takeover:** display over vsock through a ticketed broker, the controller in the log, computer-use tools. Tests 18, 19, 29.
7. **Browser VM and credentials:** a browser VM per identity, the sign-in order, surrogates. Payments wait on a card issuer. Tests 20 to 23, 30.
8. **Safety:** inbound screening, the outgoing classifier, the email login-code filter, the independent monitor, proactive work on computers. Tests 32, 33.
9. **Sub-agents, then local desktop work.** Tests 24, 25.
10. **Team workspace:** requester and agent authority, channels, roles, team volume, data fetched under one member's entitlements kept in that requester's run and off the team computer, team agent, direct knowledge base writes. Tests 26, 27.

The UI catalog can run in parallel once step 5 has landed (test 28). Audit and deletion ship inside each step. Each step is done when its tests pass on a KVM machine in acceptance, since the fast tier cannot run them.
