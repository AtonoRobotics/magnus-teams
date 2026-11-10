# The rest of tests/scratch passes on Strix Beta

After decisions/2026-11-09-scratch-suites-on-strix-beta.md, 24 scratch targets still failed on Strix Beta. Each was traced to its cause. Three were product regressions, fixed below with tests that fail without the fix. No assertion about payments, the floor, hard denials, ask-first rules, taint, grants, the kill switch or the contract freeze changed.

**One shared player for the independent checks.** Since 13e1fdf (Auto Review) and 51ed3c3 (the outgoing classifier before it), every send the policy permits or a rule, grant or schedule covers asks two checks, both played by the scripted model. Most scratch tests answered them as if they were the test's own run (a `steps` script answers turn 0 with a contract), so the check could not answer and asked the person. That is the right failure for a check that cannot answer, so the tests changed, not the kernel. `tests/scratch/src/checks.rs` now plays both, and every affected test starts its model with `start_checked` (or `with_checks` around its script), as does `Flow`, which the browser flows share. It plays them honestly and adds no authority of its own:
- the classifier reports a private business figure (the tests' Q3 revenue) and nothing else;
- Auto Review allows only what its document says something set up allows: "covered by a rule or grant" or "permitted by policy" (the owner's own permit). It asks about everything else, such as "the organisation's policy asks" and a new source. It never denies.

So what goes ahead in a test goes ahead because the owner's policy or the person's rule or grant allows it, as before the checks existed. What the kernel decides before Auto Review (a payment, an ask-first rule, the floor, a hard denial, a frozen contract, the kill switch) never reaches it. The per-test players in autonomy.rs and standing_authority.rs stay: they are written for their scenarios.

**The reviewer module tests.** Two changes there are intended and documented. The independent checks record their replies as `reviewer_reply` events too (with purpose `outgoing_classifier` or `auto_review`), so the count of the module's replies now leaves those out. And a send rewritten after a block goes to the person (decisions/2026-11-09-safety-model-checks.md: "a reviewer that blocked once should not then be the only judge of the rewrite"), so the blocked-send test now approves the corrected send and checks the card says it revises a blocked message.

**Regression: a person asked twice about one rewrite.** The person approved the rewrite once, and Auto Review's stage then spent that approval and asked again. `reviewer_stage` in boundary.rs now lets a send through that a once-approval covers and leaves spending it to the approval stage. The blocked-send test catches the second card.

**Regression: the reviewer module counted the checks as its own work.** The registry's "acted" count for the reviewer took in every `reviewer_reply`, the checks' replies included. state.rs now counts only the module's own purposes, `send` and `completion` (`MODULE_PURPOSES` in run/reviewer.rs). The unit test `only_the_modules_own_replies_count_as_what_it_did` covers it.

**Regression: task cards in a conversation lost their link, status and Cancel.** cc2eb4b added a top-level `taskCard` to team-panel.js. The UI scripts are plain scripts that share one global scope, so that function replaced app.js's `taskCard`. It is renamed `teamTaskCard`. `desktop/tests/globals-check.mjs` now runs in the fast tier and fails when two of the scripts index.html loads declare the same top-level name. desktop_flow, library, live_cards and task_volume failed on this.

**Test harness changes.**
- library.rs started its task on the outcome turn too, because the outcome turn repeats the person's words. It now starts only on the person's own turn.
- workspace-continuity.mjs held the send with Playwright's `route.fetch`, which sometimes reached the server twice: two POSTs on the wire for one from the page. It now holds the request and lets it continue.
- workspace-review.mjs expected "appliance's own disk", which now reads "server's own disk".

**Wording and field names.** The config token's refusal says "server", not "appliance" (062ac58, decisions on saying server); config_tokens.rs expected the old word. A person's Gmail card gained the field `needs_token` (cb594a7, for connectors Magnus builds); google.rs looked for "token" in the card's JSON, keys included, so it now looks only in the text the card says.

**A connector not ready on the server.** Its plus is `aria-disabled` (0159110), which Playwright will not click unless forced; a person's click still reaches it and the toast says why. connectors.mjs forces the click.

**Strix Beta.**
- The browser jail was not installed there. `config/install-browser-jail.sh` was run with sudo for samuel; browser_jail.rs passes in a new login.
- release_channels.rs reads what is committed (a script's execute bit, with `git ls-files`), and the copy had no repository: a worktree's `.git` is a pointer the copy cannot follow. `ci/on-strix-beta.sh` now sends a bundle of HEAD and makes the copy a repository at that commit, its working tree the copied files.
