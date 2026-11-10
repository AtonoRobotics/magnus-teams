# Auto Review clears every ask but payments, ask first and the floor; it clears new sources too

**Why.** The owner: "Magnus has lost its autonomy." Spec rev 133: "Auto Review decides almost every boundary action, so the user is asked only about what it cannot clear", and on sources: "A new source is a boundary action, cleared by the person's rules or Auto Review like any other, and an approved source is granted to the task or kept. The person is asked only when Auto Review asks." This reverses the sources half of 2026-11-09-rules-and-sources.

**Every ask goes to Auto Review**, which may clear it where policy delegates (the shipped default). Before, an ask on an action that is not a send (`is_reviewed_send`: e.g. a policy ask on a tool outside a connector, the web, the browser or a computer) went straight to the person when no rule or grant covered it. The routing is `auto_review::route`.

**What still goes to a person, or is refused:**
- Payments (`Risk::Financial`), every time, whatever a rule, grant or Auto Review says (spec, from Duet: "Payments and account creation approved every time"; "Accounts that move money ask for every payment"). Before, an accepted rule could cover a payment ask, and Auto Review could clear one.
- The person's own ask-first rule. Reason: the spec keeps "ask first" as a rule the person writes and says it wins over allow; Auto Review reads "their rules", and a reviewer that followed one would ask anyway. A deterministic match is what makes a rule hold (spec, Rules and grants), so the kernel enforces it rather than hoping the reviewer does.
- Auto Review's own ask, an error or a timeout (as before).
- The floor: the kernel's own asks about private data going wider after untrusted input (Rule of Two, web lookups, audience) and the local "ask every time" and "change needs a rule" settings, which Auto Review reaches only when a rule covers them (as before).
- Hard denials and block rules are refused before anything is asked (as before).

**Account creation** has no action of its own in the kernel yet. When one is added it joins `rule_effects::person_only`.

**New sources.** A rule names a source by its host (connector `computer`, tool `add_computer_destination`, destination the host), so an allow rule can cover it and an ask-first or block rule can stop it. Otherwise Auto Review judges it. Only a rule covers it: a grant or a confirmed schedule names an app, not a host. Cleared by a rule or Auto Review, the source is granted to that task only. Asked of the person, "allow for this task" and "always allow" work as before.

**Every payment reaches the person, whatever the policy permits.** A Link purchase is approved by the person in Link (`payments::purchase_request`), and a saved-card fill asks here every time (`kernel:saved_card`). Any other financial action, such as a connector tool classed financial, now raises `kernel:payment` even where an org policy permits it, answerable only by an approval of that one action (`payments::money_ask`). Before, a permit left it to Auto Review alone. Reason: the spec says payments are approved every time, and this is the safer reading.
