# Owners set the team agent's spend limits per workspace and per channel

The spec's Controls say owners set spend limits per workspace and per channel. A limit is an amount of dollars per calendar month (UTC), counted from the cost of the team agent's runs started that month in the workspace, or asked from that channel. A sub-agent's cost is already in its parent's, so it counts once.

- Only a workspace owner sets or lifts a limit, because a limit is money. An admin cannot.
- Where both apply, the tighter one holds.
- Once a limit is spent, a mention still posts the message, the agent does not start, and the poster is told which limit stopped it and that an owner can raise it. The agent's own follow-ups stop too.
- Work that starts gets no more budget than is left, on top of the task budget, so a limit holds partway through a task as well.
- The month's spend and the limits show in the right panel for every member. An owner sets a limit by typing an amount in the command menu, with no form.
- The limit travels as a new team `chat` change, `spend_limit`, listed as content like the rest.

Reason: the spec's rule, and money is bounded before work starts rather than reported after it.
