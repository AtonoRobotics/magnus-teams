# Auto Review reads only the policy layers that apply to the person

Auto Review was given every policy layer on the server as "their rules": the organisation's, every group's, every person's own layer and every task's. Spec rev 140 says it sees "the person's rules". A review for Alice therefore read Bob's own layer, which is Bob's, not Alice's, and could let a narrowing Bob wrote for himself change how Alice's action was judged, or a permit Bob wrote widen it.

It now reads the layers `policy::decide` reads for the run's person and task (`policy::layers_for`): the organisation's (or the default when none is written), the groups the person is in, their own layer and the task's. A credential review reads the person's layers without a task's. The cache key already hashes the texts it is given, so a change to any of them still changes the key.

Tests: `auto_review::tests::the_review_reads_only_the_persons_own_layers`, and the invariant tests of layering (`inv8_layers::a_user_layer_cannot_widen_the_organization_layer`, `a_user_layer_narrows_for_that_user_only`), whose stand-in reviewer reads the layers it is given.
