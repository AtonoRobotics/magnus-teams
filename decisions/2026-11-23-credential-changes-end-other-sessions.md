# Credential changes take the password and end other sessions

**Choice.**
- A change to how someone signs in goes one way: their current password first, then the change, then every other session of theirs ends on the record (`SignedOut` each), and the session making the change stays. Adding an authenticator (the first one too), replacing it and removing it go through `credential_change` in `passwords.rs`; the password's own change already did all three in one transaction (`Accounts::change_password`) and is unchanged.
- Beginning an enrolment still takes only the session: it makes a secret in memory and changes nothing until it is confirmed.
- The desktop app's Settings asks for the current password whenever it confirms an authenticator, and says the change signs out the person's other devices, as the password form says.
- Lockout counters are stored in `account_lockouts`, beside `account_sign_in_policy`, created by the accounts migration as the other account tables are. The running server still counts in memory, at once, so guesses sent together cannot all pass; each check then writes the counters it changed (a failed write is logged and retried with the next). The store opens with every stored counter, so a restart or a deploy forgives no lock and no guessing in progress. Turning someone on still clears their counters, in the store too.
- Counting a wrong invitation-link code became async (`guessed_wrong`), so it is written like the rest.

**Reason.** A first authenticator could be enrolled from a leftover session, against the code's own comment, locking the real person out; adding or removing one left every other session signed in, unlike a password change; and every deploy forgave a permanent lockout and every guess in progress.
