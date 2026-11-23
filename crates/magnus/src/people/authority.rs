//! What a person may administer on the server: one answer, from both places
//! "admin" is kept (spec rev 141, People; spec, Roles).
//!
//! Their roles here (the config's, `RolesSet`'s or the identity provider's)
//! name owner, admin or guest; the server's own directory
//! (`accounts::Role`, what the People screen and the command line set)
//! names admin or member. An owner is an owner; an admin in either place is
//! an admin. This is the server's axis, not a workspace's (`team::Role`): a
//! Team Admin administers their workspace and nothing here.

use crate::accounts::Role as Directory;
use crate::config::Person;

/// The one role of the four a person holds on the server, the highest when
/// several, least trusted first.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Authority {
    /// Reads and chats only.
    Guest,
    Member,
    Admin,
    Owner,
}

impl Authority {
    /// `p`'s, with the server's own directory's role for them.
    pub fn of(p: &Person, directory: Directory) -> Authority {
        let r = super::rights(p);
        if r.is_server_owner() {
            Authority::Owner
        } else if r.holds_admin_role() || directory == Directory::Admin {
            Authority::Admin
        } else if r.holds_guest_role() {
            Authority::Guest
        } else {
            Authority::Member
        }
    }

    /// Whether they administer the server: an owner or an admin. What this
    /// lets them do is support, never another person's data (decision
    /// `2026-11-11-a-magnus-admin-supports-and-reads-nothing.md`).
    pub fn administers(self) -> bool {
        self >= Authority::Admin
    }

    /// The one of the four `role` names, if it names one.
    pub fn named(role: &str) -> Option<Authority> {
        [Authority::Owner, Authority::Admin, Authority::Member, Authority::Guest].into_iter().find(|a| a.as_str() == role)
    }

    /// Its name, as the People screen shows it.
    pub fn as_str(self) -> &'static str {
        match self {
            Authority::Owner => "owner",
            Authority::Admin => "admin",
            Authority::Member => "member",
            Authority::Guest => "guest",
        }
    }
}
