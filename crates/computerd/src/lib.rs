//! The root service that starts and owns computer VMs. Magnus asks it, by
//! computer id, over a socket only the `magnus` group can open. Requests carry
//! no paths or sizes: everything root touches comes from its own config and
//! directory (`decisions/2026-11-08-computerd.md`).

pub mod computers;
pub mod config;
pub mod forward;
pub mod id;
pub mod relay;
pub mod seal;
pub mod server;
