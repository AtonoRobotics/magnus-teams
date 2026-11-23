# One rule for changing someone's account

**Choice.**
- One check, `Kernel::may_change_account(actor, target)` in `people/may_change.rs`, decides whether someone may change another person's account: their role, status (on or off), recovery address, invitation, or removal. The server itself (the console on the machine, the server's own upkeep) may change anyone's; a person their own; an owner anyone's; an admin anyone's but an owner's; no one else anyone's. `may_give_role` adds that only an admin gives a role, their own included, and only an owner gives the owner role.
- The check sits inside the operations, not the routes: `set_server_role`, `erase_person`, and the new `account_change` operations (`change_account_email`, `set_directory_role`, `turn_off_account`, `turn_on_account`, `withdraw_invitation`, `renew_invitation`, `set_invitation_password`). The control routes that change a person (moved to `control/people.rs`) call those operations and never the account store's setters, so the PATCH's email and status fields pass the same rule as its role.
- The asker is read from the actor the log names: `admin:<id>` or `person:<id>` is that person; anything else is the server.
- An account made a moment ago by a team invitation or an invite link gets its role through `new_account_role`, which skips the asker's check: the invitation or link was allowed when it was made, and a Team Admin who invites a guest is not a server admin.
- Resending an invitation and setting an invitation's password count as changing the account: either hands the admin a way into it.

**Reason.** The owner-only rule lived in the Admin section's routes, and each remembered only the fields it thought of. An admin could set an owner's recovery address to their own and then reset the owner's password, or turn an owner off through the PATCH that POST /disable refuses. A rule asked by each operation itself cannot be skipped by the next route.
