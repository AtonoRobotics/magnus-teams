# Agent computer: declared destinations replace computer taint (2026-11-08)

Spec revision 69. The owner withdrew computer-level taint after review showed nearly every computer would hold both taints after one web read and then ask about almost every host.

- Run taint stays as it is today. Computers carry no taint. Reason: a guest the agent controls cannot vouch for which process sent what, and sticky taint made the normal computer unusable.
- A computer reaches only declared destinations: package registries for downloads only, the hosts each app declared at install, and pages the agent opened with their own resources. Anything else is refused and logged, and adding a destination is a boundary action. Reason: an allowlist holds even as root and asks nothing in normal use.
- Rulings A and B and invariant tests 1, 10 and 15 are removed. New tests cover declared destinations, the outgoing classifier and the email login-code filter.
- Auto Review acts only where owner policy delegates to it, and the shipped default delegates. Reason: invariant 6 stands unchanged, because autonomy still comes from policy.
- A rule or grant covers an action only by a deterministic Cedar match. Reason: if the reviewer decided coverage, the floor would be its judgment after all.
- Backups include user volumes and browser profiles. Reason: `backup.rs` covers only the event log and vault, so a lost disk would lose the user's work.
- A stopped computer's volume is read and changed by a no-network helper VM, never mounted on the host. Reason: a root agent controls that filesystem, and mounting it attacks the host kernel.
- An edit made while a computer hibernates discards its memory snapshot, and the next wake is a cold boot. Reason: the snapshot holds that filesystem's cache, so resuming would corrupt it.
- Computers have no profile drive. Reason: profiles live only in browser VMs.
- Computer VMs outlive the kernel. Reason: `spawn.rs` kills children with the kernel, so each update would kill every live computer.
- An overlay is pinned to the base image it was made on, and any other base replays the manifest. Reason: a copy-on-write overlay is valid only on its own base.
- Egress is audited in its own table keyed by computer, outside the replayed event log. Reason: the log has one writer and is fully replayed at boot and on every backup check, and much of this traffic has no run.
- A proxied request that needs an ask fails at once with the ask's id and is retried after a decision. Reason: a connection cannot be held open for 30 minutes.
- The Auto Review cache key includes the destination, method, a hash of the content, the run's taint and the rule version. Reason: a key by host alone would let later bodies leave unreviewed.
- The proxy verifies each manifest entry's source and hash against what it saw, and installs replay from cached installers. Reason: a root agent writes the manifest.
- On a team computer, data fetched for a requester stays in that task's directory, readable only by tasks for requesters entitled to it. Reason: reads inside a VM are not reviewed, so shared paths would hand it to the next requester.
- Definitions: an identity is one account at one site, owned by a person or workspace. A vault scope is per person or workspace, not per computer. A throwaway computer is the per-run sandbox with no network, volume or profiles.
