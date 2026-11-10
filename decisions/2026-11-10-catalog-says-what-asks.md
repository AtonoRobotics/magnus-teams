# The catalog says what rev 140 asks, not what it used to

Fifteen catalog entries told the person that Magnus writes or sends "with your approval", "when you allow it", "asks you before each" or will not "change anything without asking you first". Under spec rev 140 none of that is true. Connecting an app grants its use. A write or send that a request needs goes ahead once Auto Review finds it fits what was asked. The person is asked only for an action outside that, a payment, private data to a new destination after untrusted input, or their own ask-first rule.

The cards now say "when your request needs it", "each send is checked against what you asked for" and "change anything you did not ask for without asking you first". Payment text is unchanged, because a payment still asks every time.

Reason: a card is the person's account of what Magnus will do with their data. Rev 140 made the decision, and text that promises asks that no longer happen misleads them. This is wording that follows from the spec, not a new promise.

Test: `catalog::tests::promises_only_the_four_asks` runs for every manifest, shipped and held, and fails on any of the old phrases.
