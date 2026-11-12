# An erased person's id is not given again, and their payment and app secrets go with them

Spec rev 145, People: "Removing a person transfers or deletes their projects and agents and deletes their data". The owner, 2026-11-11: "users should be isolated from each other securely".

A person's id comes from the local part of their address (`invitations::derive_id`). After a person was deleted, the next account with the same local part (`ada@elsewhere`) got the same id, and with it what the erasure had left under that id: their accepted rules, their stored card and Link wallet in the vault, and the apps they built with those apps' tokens. Someone new could have seen the card's last digits and paid with it.

- An erased person's id is never given again: a sign-up, a workspace invite and an admin's invite all take the next free numbered id (`ada-2`) instead (`invitations::free_id`, which the admin invite now uses when the plain id belonged to an erased person).
- The erasure also removes the person's card, Link wallet and the tokens of the apps they built from the vault.

Left for later, and harmless once the id is never reused: the erased person's rule and app records stay in the state under the dead id.
