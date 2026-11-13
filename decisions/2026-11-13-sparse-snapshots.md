# A computer's snapshot leaves out the memory it never touched

**Choice.** A hibernating computer's memory and state are sealed chunk by chunk as before, but a chunk of zeros is kept as its tag alone (`crates/computerd/src/sparse.rs`). On wake it is opened as a hole in a sparse file, and the other chunks are opened in parallel. The snapshot's `format` file says `sparse-1`; a snapshot without it, made before this change, opens the old way. A chunk's kind and length are authenticated with it, and its nonce carries its place and whether it is last, so a chunk cannot be changed, moved, dropped, cut or turned to zeros unseen.

**Reason.** The owner (2026-11-13): "The computer is slow to load." The server's log: a cold boot took about 2.4 s, but a wake from a snapshot took 10 s, once 198 s, almost all of it opening the 4 GiB memory file whole (`open_snapshot`), most of which is zeros. The spec stores snapshots "as chunked copy-on-write so only changed chunks are uploaded".
