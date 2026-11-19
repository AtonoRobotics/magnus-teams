# A repository is a destination, and hard to undo is no ask of its own

The owner reported that Magnus asked for approval on nearly every GitHub
change of a task they had already given and approved, where a code agent
would just do the work. Two things made that happen.

**The Rule of Two had nothing to remember.** GitHub's manifest labels what
it returns `private+untrusted`, so the first read of a repository taints the
run, and every write after it (all to a `public`-audience app) is case 3 of
the asks: private data to a new destination after untrusted input. A
destination comes from the recipients a gate read or the addresses a call
names, and a GitHub call names neither, so a GitHub write had no
destination. With none, an approved destination could never be remembered
(`asks::remembered` needs one), a person's Allow always rule could not be
saved (`rules::compile` needs one), and an Allow covered only that one tool
for the rest of the task. A commit, then a pull request, then a comment each
asked again.

A manifest can now name where a write lands as a template over the call's
fields, `place = "github.com/{owner}/{repo}"` (`places.rs`). When the gate
read no recipients, the place is the action's destination everywhere one is
used: case 3, an Allow's scope, a remembered destination, the person's rules
and the card's sentence. So allowing a repository once is remembered for it,
and a repository the person named in their own words (`owner/repo`, or its
`github.com` link) is theirs, as an address they wrote already is: injected
text can no more choose a repository they named than a recipient they wrote
(#176, the owner's ruling of 2026-11-04 that the content is theirs to
judge). A call whose fields do not name a plain repository (a new
repository has none yet) has no place and asks as before.

**Auto Review asked about everything hard to undo.** Its instructions said
"Ask when you are unsure, and when the action would be hard to undo", and
the GitHub manifest classes nearly every write `irreversible` (a commit
stays in history, a comment notifies watchers). So a commit the person asked
for was an ask even with no taint. The spec's case 1 is an action Auto
Review judges *outside what was asked*; irreversibility alone is not one of
the four asks. The instruction now allows an action that does what the
person asked even when hard to undo, and asks when unsure whether it serves
the request or when a hard-to-undo action goes beyond it. Auto Review still
judges every boundary action, and payments, the person's own ask-first
rules, prohibited tools and the hard denials are unchanged.

GitHub's Library text said Magnus would commit and merge "when you allow
each one"; it now says "when your request calls for it, and ask you first
about anything beyond it", which is what happens.

Tests: `places::tests` (a GitHub write's destination is its repository, and
none without the place; the person names a repository by its path or link),
`auto_review::tests::what_cannot_be_undone_asks_only_beyond_the_request`,
and the catalog check that a place names a host and a field.
