# Auto Review, rules and unattended asks

**Every boundary action is reviewed.** That is any model-issued, state-changing call to a connector, the web or a browser, in a task. A policy permit, a grant, a schedule or a rule does not skip it, because the spec says covered actions run when "Auto Review allows it". Only a person's own approval of the action does, since a person outranks the reviewer.

**It cannot be turned off.** `[auto_review]` has no switch and refuses unknown keys. `delegate = false` only stops it clearing asks. An error, a timeout (30 seconds), an unreadable reply, a spent budget or an oversized document all become an ask.

**What it may decide.** Allow clears a policy ask only where delegated. Deny stops the action. It never clears the kernel's own asks about private data going wider after untrusted input. Those need a person, or a rule that already covers them, and even then the reviewer must also allow.

**Its input.** One JSON document as the only user message: the action, the person's own words from the task, the policy text, and what already covers the action. The system prompt is fixed. The reviewer uses the run's model in a separate call with no tools but its verdict. A distinct reviewer model is a config change we have not made.

**The cache** is keyed by person, tool, destination, content hash, run taint, rule version and standing. Asks are not cached.

**Rules.** A rule is compiled to Cedar over person, connector, tool and each destination, and only that match covers anything. `POST /v1/rules` with a connector, tool and destinations from a signed-in person is accepted at once. With `proposed: true` it waits for `POST /v1/rules/{id}/accept`, which is the path the agent's proposals will use. Words with nothing compiled are recorded and cover nothing. The words themselves are not kept in the log: they are content, and plain-word authoring arrives with the UI catalog. Grants stay as they were, covering policy asks only, because they name apps and not destinations.

**Hard denials** run before policy: a stored credential in a model's input, in any encoding `vault::redact` knows, or an input naming another person's computer or, in an account field, another person. Nothing is asked.

**Unattended asks.** An ask raised by a routine (a task from a schedule) expires after `unattended_ask_expiry_seconds`, default 30 minutes. It is resolved as `expired`, a `draft` item holds the action for the person, and the run is told it was not done. A tool classed financial or named as an install keeps its ask, and its task keeps waiting.

**Against the spec, on purpose.** Magnus's own background work (a proactive review, a watch poll) still asks no one, by the owner's ruling of 2026-11-07. Its action is refused at once and the model can suggest it instead. The spec's words would have it raise an ask that expires. Nothing runs either way, so the ruling stands until the owner says otherwise.

**Proxied asks.** A proxied request needing a decision is refused at once with `X-Magnus-Refused: <row id>`. The agent's tool for adding a destination arrives in step 6 and goes through this pipeline.

**A cost of this design.** The invariant tests need the step 5 patch before they pass: without its fake reviewer, every permitted send becomes an ask.
