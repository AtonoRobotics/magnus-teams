# Deleted files leave the volume's free blocks at hibernate, and a VM survives big writes

A computer zeroes its free space before it hibernates, if a file was deleted
since the last time. Reason: an earlier version of a deleted file lives on in
free blocks, and only overwriting them removes it. Hibernating is when a fill
harms no one.

The guest opens ext4's reserved clusters for the fill and puts them back.
Reason: the reserve is held back even from root, and the old text was found
sitting in it.

computerd checks the host has room before the fill and punches holes in the
all-zero blocks after the VM stops. Reason: a fill writes every block of a
sparse file, so it needs the space briefly and must give it back. Without room
the zeroing is skipped and logged, and tried again at the next hibernate.

Each VM gets `memory.high` 128 MiB under its `memory.max`, and writable drives
use Firecracker's `Writeback` cache. Reason: the host's page cache of a VM's
writes is charged to its cgroup, and Firecracker's default drops guest flushes,
so a guest writing more than its headroom was OOM-killed. Measured: 3.6 GB
written in a 1 GiB VM killed it in 18 s before, and it now finishes. The cost is
throughput under heavy writes, about 40 MB/s in that test.
