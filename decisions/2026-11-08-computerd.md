# Computers are started by a small root service, magnus-computerd (2026-11-08)

Firecracker's jailer has to start as root, Magnus runs unprivileged, and computer VMs must outlive Magnus.

- A separate root service, `magnus-computerd`, starts and owns the VMs. Magnus reaches it over a Unix socket limited to the `magnus` group. Reason: a sudo rule for the jailer would let a compromised Magnus choose what runs as root, and running Magnus as root makes every Magnus bug a root bug.
- Requests carry a validated computer id and an operation, never a path, a command or a size. Computerd owns the computers' directory, creates every drive image itself and derives all paths from the id. Reason: Magnus writes nothing that root reads.
- Computerd refuses to start unless its config, programs, images and state directory, and every directory above them, are owned by root and not writable by anyone else. Reason: the rule above has to hold on the machine, not only in the code. It follows that base images are copied to a root-owned place at install, and computerd is updated by the owner's script and not by the appliance's own update.
- Each VM runs under the jailer chroot as its own unprivileged uid, with its CPU, memory, process and file-size limits taken from computerd's config. Reason: one escaped VM must not reach another's disk or exhaust the host.
- Computerd keeps VMs in their own cgroup, outside its service and outside Magnus, and finds, pauses and kills them through the cgroup. Reason: VMs survive restarts of both, and the kernel's list of processes cannot be forged from inside a chroot.
- The owner can kill every computer with `magnus-computerd kill-all`, which needs neither Magnus nor the running service. It blocks new starts until `magnus-computerd release`, and release resumes nothing. Reason: a kill that Magnus could immediately undo is not a kill.
