# Web reads are checked for private data, not for where they go

**Choice.** A run that holds untrusted content and private data browses
anywhere: any address, any path, any query, any site, with no card. What is
checked is what the request sends (the address, and what is typed into a
page), against the private values the conversation has read
(`private_values.rs`, `boundary.rs` policy stage):

- Every tool result from a private source is recorded as private
  (`ResultRec::private`). Its distinctive values (figures, codes, ids,
  addresses: tokens with a digit, an `@`, or 16 characters or more) that
  appear nowhere in what the person wrote or the web returned are the
  conversation's private values: the run's own results, its parents', and
  the earlier turns of the chat it continues.
- A web or browser read whose input carries one of them, written plainly,
  percent-encoded, without its punctuation, reversed, or in base64 or hex,
  is refused without a card, and the agent is told to leave the data out.
  Anything else opens.
- Where the private data cannot all be listed (an uploaded file, or privacy
  carried in with nothing on record), the address rules of
  `decisions/2026-11-22-browsing-without-asks.md` decide, as before.

Web reads no longer bring the Rule of Two's ask: what a read can carry out is
checked directly. Writes to people (mail, posts, records) keep the envelope,
drafts and asks they have. `web_search` keeps its query gate.

**Reason.** The owner: an agent that cannot use the internet without
approval of every address is more work than doing it by hand; the address
rules, however loosened, kept limiting the browser. The leak they guarded
against is private data in a request, and the kernel knows exactly which
private values each conversation has read, so it can check for them in
every request by fixed rules, the way data-loss prevention does, instead of
limiting where the agent may go or asking a model.

Residual risk, stated plainly: an agent that obeys an injected instruction
could still send private data in a form the check does not recognise: a
value spelled out in words, split across several requests, or encoded some
other way; and ordinary words from private mail (a name, a project) are not
counted as values. Which pages it opens can also carry a few bits. The
inbound screen that withholds content trying to steer the agent comes first,
and the injection suite (`tests/scratch/tests/injection.rs`) checks that no
attack's secret reaches the attacker.

Tests: `private_values::tests`, `tests/scratch/tests/web_reads.rs`
(`any_address_opens_unless_it_carries_private_data`), the injection suite,
`inv4_taint`'s `a_fetch_cannot_carry_private_data_out_after_untrusted_input`,
and `inv4_compartments`' turn-2 test in
`docs/proposals/2026-11-22-compartments-invariants.patch`.
