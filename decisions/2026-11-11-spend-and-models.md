# Spend limits, alerts, requests for more, and who may use which model

Spec rev 141, Admin → Spend and Admin → Models. Built in `crates/magnus/src/spend/`, mounted behind the app's admin guard (`admin_app`).

## Where limits live

Limits are admin-set state written as `spend` events (`limit_set`, `alerted`, `increase_asked`, `increase_decided`, `models_allowed`), not config edits. The log already holds who set what and when, a limit survives a restart and a restore with the rest of the state, and an admin changes it from the app without touching the server's files. The config's `[budget]` stays the per-task budget (`task_budget`); it is not a monthly limit.

The team agent's workspace and channel limits stay the team's own `spend_limit` change (decision 2026-11-10-team-spend-limits). Admin → Spend sets them through that same change, so there is one record of them. A workspace owner can still set them from the workspace.

## Counting

A month is the calendar month in UTC. A run's dollars count in the month it started, as the team's limits count them. Every run's own `usd` is summed, and a sub-agent's run holds its own calls, so each dollar is counted once. A team run counts against the person who asked for it, against the server, and against its workspace and channel. Counting it only against the team would let anyone go past their own limit by asking the team agent, and the spec says no person exceeds a limit.

The breakdowns are by person (who the run is for), by agent (the person's own agent, or the team agent of a workspace; a sub-agent counts as its parent's agent), by model (the model the run started on: a run moved to a stronger model by escalation keeps its starting model in the breakdown), and by project (the top task's project, else the team channel's project, else "No project").

## Cascade and enforcement

A person's limit is their override, else the default, plus whatever admins approved for them that month. What they may still spend is the smaller of what is left of their own limit and what is left of the server's, so nobody passes the server's limit. The team agent's work is also held to its workspace and channel limits, which are checked first, as before.

The check is made in two places. When a run starts (`Kernel::create_run`), work past a limit does not start, and work that starts gets no more budget than is left. Partway through a run (`run::agent_loop`, beside the grant check), the run stops with the notice reason `spend_limit` once a limit is spent. That catches runs going at the same time and a person's "Extend" of a task budget. The refusal says which limit stopped the work and the amounts. The app gets `402` with `spend_limit: {which, month, spent_usd, limit_usd, can_ask, pending}`.

## Alerts

Every minute (`ALERT_CHECK_INTERVAL_SECS`) the server checks each limit: the server's, each person's, and the team agent's workspace and channel limits. When one reaches 75% or 90% of its amount, each admin gets an inbox notice, and an `alerted` event records it so the alert goes out once per limit, share and month. A limit of 0 gets no alerts, because a refusal already covers it. Admins are the people `admin_app::admin_of` names.

## Asking for more

When a person's own limit stops them, the app offers "Ask for more" where the message appears. They give an amount and an optional reason. Each admin gets a notice, and Admin → Spend lists the request. An admin approves, for the amount asked or a different amount, or declines, with an optional note. An approval raises that person's limit for that month only. Next month they are back to their own limit. Either way the person gets a notice. One request waits at a time. A request is decided once, under the kernel's decision lock. The reason and the note are the person's and the admin's own words, so they are content (`CONTENT` "spend").

When the server's limit is reached, the message says an admin can raise it, and there is nothing for the person to ask for.

## Models

Admin → Models lists the providers and their models, and lets an admin test or disable a provider and set the default model. These are the console's own control routes (`/v1/admin/providers…`), mounted with its audit. The new part is who may use which model. A rule names `adapter/model`s for a role or for a person.

- A person's own rule wins over their roles' rules.
- Several roles' rules add up. Everyone has the role `member`, so a `member` rule is what everyone may use, and a rule for `admin` or another role adds models to it.
- A role with no rule adds nothing. Otherwise a role nobody thought about would undo the rule for everyone.
- With no rule at all, everyone may use every model, which is the behaviour before this change.
- The owner pays and writes the rules, so the owner is never limited, and no rule can be written for the owner.

The rule is enforced where a run's model is chosen (`create_run`). A model the person chose but may not use is refused with `403` and the models they may use (`allowed_models`). When they chose none and the server's default is not theirs to use, their work goes to the first model they may use. An escalation to a model they may not use is dropped. The app's model picker (`GET /v1/models`) lists only the models the person may use, and the default that applies to them.

## Audit

Each admin change in these sections writes the console's audit entry first, through `admin_app::mount::audited`, under these names: `spend_limit_set`, `spend_request_approved`, `spend_request_declined`, `model_access_set` and `provider_set`.

Reason: these follow the spec's rules, the team-limit decision and the Code Standard. Money is bounded before work starts rather than reported after it.
