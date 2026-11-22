# Browsing without asks: an address opens by what it can carry

**Choice.** In a run that holds untrusted content and private data, the
web and the browser open an address with no card and no model's
judgement when it is one of these (`boundary.rs` policy stage,
`web_open.rs`):

1. an address the person wrote, or one a search or a loaded page returned,
   exactly (`web_reads`, unchanged);
2. any address on a site the person named in their own words (`stripe.com`,
   `https://docs.example.org/spec`), or under it (`docs.stripe.com`);
3. the bare https home page (nothing after the host) of one of the
   most-visited sites: origins of rank 10,000 or better in the Chrome UX
   Report, shipped in `crates/magnus/data/popular_sites.txt` and refreshed
   with releases.

Anything else is refused without a card, and the agent is told to open the
site's home page or search for the page, then follow its links. A run that
has not read both untrusted content and private data opens any address, as
before. `OpenWeb` workers, which hold nothing private, keep their own rule.

This replaces the outgoing-URL gate of
`decisions/2026-11-22-autonomous-browser-navigation.md` (a classifier's
clear let the address go) and the ask of
`decisions/2026-11-22-a-classifier-clear-is-not-a-pass.md` (a cleared
address asked). `check_web_url` is removed; `web_search` keeps its query
gate.

**Reason.** The owner: remove the approval ceremony and let the agent use
the browser without a security risk. Nearly every run holds both kinds of
content after one email or page, so the earlier rules either stopped the
agent on almost every site it found for itself or, when loosened, leaked:
the injection suite's CRM case sent `<collector>/collect?d=<data>` because
a classifier saw nothing sensitive in it. An address carries data out only
to someone who sees the request. A link the web gave carries nothing the
model added; a site the person named is theirs to reach; a popular site's
home page carries nothing, and an attacker cannot see who visits it. These
are checked by fixed rules, so the boundary does not rest on a model that
an attacker's text can mislead.

Residual risk, stated plainly: which link the agent follows, and which
popular home page it opens, can each encode a few bits for someone able to
watch that traffic. What the person pastes into their request counts as
their words, as it does for the write envelope. A deep path on a popular
site is not opened on its own, because some such sites show page owners
which paths were visited (a GitHub repository's traffic page, site
analytics), so a path there could carry data out.

Tests: `web_open::tests`, `tests/scratch/tests/web_reads.rs`
(`a_link_from_the_web_opens_and_a_made_up_address_is_refused_without_a_card`),
the injection suite, and `inv4_taint`'s
`a_fetch_cannot_carry_private_data_out_after_untrusted_input`.
