//! What a person may do, read from their roles here: every question the
//! server asks of a person's roles is asked here, in its own words, so no
//! call site spells a role name or decides from one (decision
//! `2026-11-14-one-answer-to-may-they.md`).
//!
//! These read the roles alone, without the server's own directory, so they
//! answer at once. "Do they administer the server" also reads the directory
//! and is [`super::Authority`]'s. The owner's rights here are the owner's
//! as they were; none of them reads another person's data
//! (`2026-11-11-a-magnus-admin-supports-and-reads-nothing.md`).

use super::Authority;
use crate::config::Person;
use crate::tools::Risk;

/// The role that makes someone the server's owner.
const OWNER: &str = "owner";
/// The role that makes someone an admin here.
const ADMIN: &str = "admin";
/// The role that shows the developer view: raw log, runs, experiments (K10,
/// K45).
const DEVELOPER: &str = "developer";
/// The role that puts someone on the test channel for the app.
const TESTER: &str = "tester";

/// `p`'s rights, read from their roles.
pub fn rights(p: &Person) -> Rights<'_> {
    Rights { p }
}

/// One person's rights, as their roles give them.
#[derive(Clone, Copy)]
pub struct Rights<'p> {
    p: &'p Person,
}

impl Rights<'_> {
    fn holds(&self, role: &str) -> bool {
        self.p.roles.iter().any(|r| r == role)
    }

    /// Whether they are the server's owner. The owner alone changes the
    /// server's own settings (mail, the updater, the server's name and
    /// people) and sees what an owner's screens show.
    pub fn is_server_owner(&self) -> bool {
        self.holds(OWNER)
    }

    /// Whether their roles here say admin. Who administers the server also
    /// reads the directory: [`super::Kernel::authority`].
    pub(super) fn holds_admin_role(&self) -> bool {
        self.holds(ADMIN)
    }

    /// Whether their roles here hold them to a guest's limits: the guest role
    /// and no role above it. Read without the server's directory, because
    /// the boundary asks it on every action without a wait;
    /// `set_server_role` keeps the two in step, and where they differ this
    /// errs toward a guest's limits.
    pub fn held_to_guest_limits(&self) -> bool {
        Authority::of(self.p, crate::accounts::Role::Member) == Authority::Guest
    }

    pub(super) fn holds_guest_role(&self) -> bool {
        self.holds("guest")
    }

    /// Whether a control policy reads them as an admin: an admin on the
    /// server, or an owner who also holds the admin role, as always.
    pub fn reads_as_admin(&self, authority: Authority) -> bool {
        authority == Authority::Admin || self.holds(ADMIN)
    }

    /// Whether they see the developer view.
    pub fn sees_developer_view(&self) -> bool {
        self.holds(DEVELOPER)
    }

    /// Whether they may approve, themselves, an action of `risk`: the owner,
    /// anyone with that risk's approver role, and anyone for a reversible
    /// write.
    pub fn may_approve(&self, risk: Risk) -> bool {
        self.is_server_owner() || self.holds(&format!("approver:{}", risk.as_str())) || risk == Risk::ReversibleWrite
    }

    /// Whether they may make a knowledge entry a company fact.
    pub fn may_endorse_knowledge(&self) -> bool {
        self.is_server_owner() || self.holds(crate::knowledge::ENDORSE)
    }

    /// Whether they may write, read as theirs, or accept a proposal for the
    /// policy `layer`. A person's own layer is theirs alone, with no role
    /// needed and nobody else's say, the owner's included. The
    /// organisation's is the owner's; a group's is the owner's and its policy
    /// role's. A task's layer is its person's, which the caller checks with
    /// the task in hand.
    pub fn may_write_policy_layer(&self, layer: &str) -> bool {
        if let Some(u) = layer.strip_prefix("user/") {
            return !u.is_empty() && u == self.p.id;
        }
        if layer == "org" {
            return self.is_server_owner();
        }
        if let Some(g) = layer.strip_prefix("group/") {
            return !g.is_empty() && (self.is_server_owner() || self.holds(&format!("policy:group:{g}")));
        }
        false
    }

    /// The app's update channel their own role gives them, or None for the
    /// one an admin set for everyone.
    pub fn own_update_channel(&self) -> Option<&'static str> {
        if self.is_server_owner() {
            Some("development")
        } else if self.holds(TESTER) {
            Some("test")
        } else {
            None
        }
    }

    /// Whether they decide deletion requests, and so see everyone's.
    pub fn decides_deletion_requests(&self) -> bool {
        self.is_server_owner()
    }

    /// Whether they answer a request to turn on an app's tools.
    pub fn decides_app_tools(&self) -> bool {
        self.is_server_owner()
    }

    /// Whether they moderate knowledge shared with groups: an entry only
    /// some people read is theirs, never the moderator's to delete.
    pub fn moderates_shared_knowledge(&self) -> bool {
        self.is_server_owner()
    }

    /// Whether they see and revoke every device, not only their own.
    pub fn manages_every_device(&self) -> bool {
        self.is_server_owner()
    }

    /// Whether they judge an experiment's trials, which run as the eval
    /// people, who have no sign-in of their own.
    pub fn judges_experiments(&self) -> bool {
        self.is_server_owner()
    }

    /// Whether they manage the apps the server shares (those no one person
    /// owns), their own groups' or not.
    pub fn manages_shared_apps(&self) -> bool {
        self.is_server_owner()
    }

    /// Whether they may use every model, whatever model access says.
    pub fn uses_every_model(&self) -> bool {
        self.is_server_owner()
    }

    /// Whether they give or take the owner role, or turn an owner off.
    pub fn may_change_owners(&self) -> bool {
        self.is_server_owner()
    }

    /// Whether they choose the identity provider: who it signs in, it signs
    /// in as anyone here.
    pub fn may_choose_identity_provider(&self) -> bool {
        self.is_server_owner()
    }

    /// Whether their roles here let them start a workspace. The server also
    /// keeps Team Admins of its own (`Kernel::is_team_admin`).
    pub fn holds_team_admin_role(&self) -> bool {
        self.holds(crate::team::team_admin::ROLE)
    }
}

/// Whether a password set here signs this person in: everyone's way in
/// without an identity provider, the owner's backup alone with one (owner,
/// 2026-10-30: "The back up password is only for me"). The owner is read
/// from the config, never from the provider.
pub fn may_use_password(config_person: Option<&Person>, provider: bool) -> bool {
    config_person.is_some_and(|p| !provider || rights(p).is_server_owner())
}
