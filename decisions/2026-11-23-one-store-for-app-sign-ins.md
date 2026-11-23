# One store for app sign-ins

Every read and write of a kept app sign-in (a person's, a shared project's,
and a person's Link wallet) goes through one store, `connectors::SignInStore`:
the fresh token, a new sign-in replacing it, more access asked of it, and its
end.

- Each sign-in has its own refresh turn. One slow token endpoint holds up
  only the calls through that sign-in, not every app call on the server.
- Each sign-in has a generation, moved on by every replacement and every
  end. A refresh writes back only onto the sign-in it read, and merges its
  tokens onto what is stored then, so access asked for meanwhile stays.
  A Disconnect, a new sign-in asking for more access, or ending Link while a
  refresh is on its way is never undone by it.
- Ending is two steps: from the moment it starts, no one is given the
  sign-in and no earlier refresh writes it back; it is revoked at the app,
  then deleted here (unless a new sign-in took its place). Where Link
  refuses to end it, it is given out again, as before.
- When a refresh finishes after its sign-in ended, the tokens it was given
  are revoked too. When it finishes after a new sign-in replaced it, they
  are left alone: some providers revoke a whole grant on any revocation,
  which would end the new sign-in.

One owner type, `SignInOwner` (a person or a shared project), reads vault
names for project deletion, withdrawal and the admin counts.

- Deleting a shared project ends its own sign-ins (revoked at the app,
  deleted, recorded) before the project goes: afterwards no one could.
- Withdrawing a catalog app tells a project's admins about the project's
  sign-in, instead of a person named after the project.
- The admin console's count of people signed in to an app counts people
  only.
- A project admin can end the project's sign-in to an app that is turned
  off, as a person can end their own, so it does not quietly work again when
  the app is turned back on.

Reason: the architecture review of 23 November (candidate 6) found each of
these, all from callers reading and writing the vault entry themselves with
only refresh taking a lock.
