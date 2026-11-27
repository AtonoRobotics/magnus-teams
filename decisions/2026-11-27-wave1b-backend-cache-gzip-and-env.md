# Wave 1B: Immutable UI Asset Caching, Pre-Gzipped Serving, and Startup `MAGNUS_LISTEN_FD`

## Decision
1. **Immutable `Cache-Control` + pre-gzipped UI assets (`crates/magnus/src/ui/assets.rs`)**:
   - Keep `Cache-Control: private, no-store, no-cache, must-revalidate, max-age=0` on `index.html` and `manifest.webmanifest`.
   - Serve version-stamped static assets (`.js`, `.css`, `.woff2`, `.webp`) with `Cache-Control: public, max-age=31536000, immutable`.
   - Pre-compress `.js` and `.css` assets once in a `LazyLock<HashMap<&'static str, Vec<u8>>>` using the existing `flate2` crate and serve `Content-Encoding: gzip` + `Vary: Accept-Encoding` whenever the client advertises `Accept-Encoding: gzip` (also gzipping `index.html` on the fly when accepted).
2. **Startup-only `MAGNUS_LISTEN_FD` (`crates/magnus/src/spawn.rs`, `crates/magnus/src/cli.rs`)**:
   - Capture `MAGNUS_LISTEN_FD` once at startup in `HostEnv::from_process()` and pass `cfg.host.listen_fd` into `cli::take_listen_fd`, storing it in a `OnceLock<i32>` for `release_listen_socket_for_exec()`. No runtime logic in `cli.rs` reads `std::env::var`.
3. **Clippy baseline (`ci/clippy-baseline`)**:
   - Verified `crates/magnus` and all non-invariant crates already have `0` clippy warnings; the only remaining `2` warnings are doc-comment indentation warnings in `tests/invariants/tests/inv4_compartments.rs:151-152`, which is protected by `AGENTS.md` and left untouched for the owner.

## Reason
- Serving 1.9 MB of uncompressed `.js`/`.css` with `no-store` forced browsers to re-download every script and stylesheet on every reload. Versioned URLs (`?v={version}`) make `immutable` caching safe, and pre-gzipping cuts wire transfer by ~80% with zero new dependencies.
- Reading `MAGNUS_LISTEN_FD` once in `HostEnv` brings `cli.rs` into full compliance with the Code Standard rule that logic never reads the environment.
