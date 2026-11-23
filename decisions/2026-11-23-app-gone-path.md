# One path for an app that is gone

Removing an app an admin added, deleting an app a person built, and
withdrawing a catalog app all take away what names the app, through one
path (`connectors::gone`): every grant that covers it, anyone's, revoked,
and every rule made for it, anyone's, removed. Removal and deletion also
revoke its sign-ins and key and then remove the app; withdrawal ends its
connections as before, telling each person or project. When an app is
removed, the tool ledger forgets its tools' screened definitions, held
changes and connect-screen acceptances, so another server added later under
the same name starts afresh and inherits nothing. A withdrawal keeps the
screened definitions (a later list may restore the same app), but its
grants and rules stay taken away: taking a permission away is always safe.

Connecting an app again accepts a held change no model could judge for that
person only. The ledger keeps who accepted which held definition; their own
runs (and calls through their own sign-in) are offered it, everyone else's
still have it held until a model passes it. A later change is held again
for them too.

The password manager's login read goes through the one authorised read
(`Kernel::password_manager_read`): the switches, the app's groups, the
screened definition, policy and an audited decision, as every other read
the server makes for a person. Its tools stay prohibited to the agent; this
one read is classed a read. The switch of its caller in
`computer/identity.rs` is left to the r5-computers change (that file is
theirs in this round) and handed over as a patch.

Reason: the architecture review of 23 November (candidate 10) found removal
dropped only consents, so a different server added under the same name
inherited people's "always allow" rules and grants; one person reconnecting
released a held, changed definition for everyone; and the password-manager
read skipped every check the other server reads go through.
