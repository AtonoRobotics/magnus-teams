# A computer credential is kept under its surrogate, and a rotation revokes the old one

The real value behind a computer's surrogate was kept in the vault under the
credential's name. Issuing a second credential with the same name overwrote
it, so the first token, bound to host X, started sending the second value
(meant for host Y) to X. The first token also kept working for ever.

Now:

- The vault keeps each credential under the surrogate's own hash
  (`surrogate/<computer>/token/<hash>`), so no later credential changes what
  an earlier token sends.
- Issuing a credential with a name the computer already has is its
  rotation: the same write records the new surrogate and a new
  `surrogate_revoked` change for each older one of that name, whose vault
  entries are then removed. The old token stands for nothing.
- A credential issued before this, kept under its name, is still read, but
  only for the newest surrogate of that name, the one the name-keyed value
  belongs to. An older one of the same name stands for nothing.
