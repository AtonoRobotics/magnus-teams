# Sign in with the email address as well as the username

A member (hartenfelsj) recovered their password twice, and the server recorded each
new password, yet they could not sign in afterwards. Sign-in took only the
username, while everything they had been sent ran on their email address: the
invitation, the recovery request, the recovery link. Neither email named their
username, and the invitation's last page already told people to "sign in with
your email address or name". A wrong name is answered "The name or password is
wrong", so the password got the blame.

Choice: the sign-in field takes either. An address resolves to the person holding
it, the first by id as recovery picks (`sign_in_name::resolve`), before anything
else runs, so lockout counters, the identity provider and the local store all see
the username. Usernames never contain '@', so the two cannot collide. An unknown
address goes on as an unknown name and gets the same answer and the same work.
The invitation email, the recovery email and the "password set" page now name the
username, and the field is labelled "Username or email".

Reason: it is what the product already told people, it is common practice, and it
adds no new promise about data. The test is in `tests/scratch/tests/password_reset.rs`.
