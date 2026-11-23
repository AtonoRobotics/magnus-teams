//! One check for using a computer: the level a use needs, and the computer
//! if the caller has it. Anyone without it is told there is no such
//! computer (`decisions/2026-11-23-one-check-for-using-a-computer.md`).

use super::{Computer, Role};
use crate::config::Person;
use crate::controls::Refused;
use crate::kernel::Kernel;
use crate::people::Authority;

/// What a use needs of a computer.
#[derive(Clone, Copy, Debug)]
pub(super) enum Access {
    /// Its screen and its person's files: seeing them, taking control,
    /// exporting, and reading, saving or deleting one file. Its person, or
    /// any member of its workspace.
    Open,
    /// Changing what it is: its egress, credentials, apps, backups, resets.
    /// Its person, or its workspace's Team Admins.
    Control,
    /// Pausing it, which is always safe: those who control it, and a server
    /// owner or admin.
    Stop(Authority),
}

impl Kernel {
    /// The computer `id`, if `who` may use it at `access`.
    pub(super) fn computer_as(&self, who: &Person, id: &str, access: Access) -> Result<Computer, Refused> {
        self.read(|s| {
            s.computers
                .get(id)
                .filter(|c| c.role == Role::Computer)
                .filter(|c| match access {
                    Access::Open => super::takeover::may_open(s, who, &c.owner),
                    Access::Control => super::service::may_control(s, who, &c.owner),
                    Access::Stop(authority) => super::service::may_stop(s, who, authority, &c.owner),
                })
                .cloned()
        })
        .ok_or(Refused::NotFound("computer"))
    }
}
