# A person is told when the owner lets a new sign-in become them

`DELETE /v1/people/{id}/account` unbinds a person from the identity provider account that first signed in as them, so the next account that signs in as them becomes theirs, with all their data. It is a real need (an account made again at the provider), and it was audited (`account_unbound`), but the person was not told. With control of the identity provider, it is a way into someone's data that the spec's rule does not allow silently ("Every admin read of a person's data is logged, and the person is told", rev 145, People).

Now the unbinding and a notice to the person, naming who did it, are written together.
