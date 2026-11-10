# Plain-word rules with allow, ask and block; sources per task

**Rules have an effect.** A rule allows automatically, asks first or blocks (`effect`, spec "Rules and grants"). A rule written before effects existed reads as an allow. Block is checked first and denies at once. Ask first wins over allow: it goes to the person, and neither an allow rule, a grant, a policy permit nor Auto Review clears it. The kernel's own asks (Rule of Two, audience) stand as they are, because they are already answerable only once.

**How wide a rule may be.** An allow stays narrow: one tool and named destinations, as before. An ask or a block may name a connector alone, or a connector and a tool with no destinations, and then matches anywhere. Reason: widening either only makes the agent do less, and it lets a person write "ask me before anything in Slack".

**Words.** The person's words are kept with the rule, as erasable content (`rule.words` in `events::CONTENT`), shown and edited in Settings > Rules, and passed to Auto Review with the rule's match as "their rules" (spec: the reviewer sees "their rules"). The match is what the kernel enforces. Words with no match still cover nothing. Turning words into a match by a model is not built; the person fills in the app, action and destinations beside their words.

**Editing.** `PUT /v1/rules/{id}` records a new `edited` change: the person's own edit keeps the rule's acceptance and resets its use count.

**Sources.** Spec: "A new source triggers an approval; an approved source can be granted to the task or standing." When the agent asks for a host its computer cannot reach, the person is always asked (no rule or reviewer answers it). "Allow for this task" grants the host to that task only (`destination_granted`): the proxy lets it through while the task's run is live, and not for any other task. "Always allow" adds it to the computer as before. Either approval holds only for that host: before this, approving `add_computer_destination` for a task covered every host the task asked for, and every approval made the host standing.

**Always allowing a source** does not need the owner. Reason: it is the person's own computer, which they may already open to a host through `POST /v1/computers/{id}/destinations`; invariant 6 is about org-wide autonomy, which this is not.

**Not done.** Ask-first approvals may still be given for a task or an hour; the spec does not say they must be once. Connector and file sources keep their existing gates (the Library connection, and folder grants).
