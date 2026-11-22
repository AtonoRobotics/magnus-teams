# A program on the person's own computer can be allowed always

Item 4 of `docs/proposals/2026-11-19-approvals-review.md`. Every tool of
Blender, FreeCAD, Autodesk Fusion and SolidWorks is classed irreversible
(nothing is classified, so each is taken to run code), so the default
policy asks before each one, as case 4 (ask first). The card never offered
Allow always, because an ask-first rule wins over any allow rule. A person
using Blender all day had to answer for every call of every task.

**Classifying reads: nothing to classify yet.** The repository names no
program's real tools: Blender's and FreeCAD's servers come with their own
phase (`docs/connectors.md`, Programs), and the only tool names here are
test fixtures (`run_python`, `list_objects`). A name is classified only on
evidence, so `PROGRAMS` stays unclassified. When a program's server is
pinned, its read tools are classed `Risk::Read` in its `classified` list
and then pass without asking.

**Allow always for a program.** A program's ask now comes in two kinds:
- `desktop::PROGRAM_ASK` (`kernel:program`): only the organisation's layer
  asks (by default, before a tool that may run code). Its card offers
  Allow always, labelled "Allow Blender always". Choosing it saves the
  person's own rule `permit(principal == Person::"<them>", action ==
  Action::"execute", resource in App::"desktop_blender")`
  (`rules::compile_program_allow`), and from then on Blender's tools run
  without asking on their computers where they enabled Blender.
- `desktop::ASK_RULE` (`kernel:local_program`): a layer below the
  organisation's (the person's own, their group's, the task's) forbids
  `execute`. That is someone's explicit ask first, so no allow rule answers
  it and Allow always is not offered. `desktop::tightened` tells the two
  apart by deciding again with the organisation's layer permitting
  everything. Items raised before this change carry `ASK_RULE` and keep
  their old answers.

The rule answers only a program's own ask (`Kernel::program_rule`, checked
in `settle` after the person's approvals): it never answers case 3 (a
destination-less rule never covers it, and case 3 replaces the program's
ask), a payment, or an ask first. The person's own ask-first rule (or the
workspace's) now takes the place of a program's own ask in
`person_rules`, so it still asks over their Allow always. The rule holds
only while the person could approve the action themselves
(`may_approve`), and they can remove it like any other rule.

**Why the whole program, and why it is safe enough.** The proposal's
words are "Allow Blender always": a program's tools are many and change
with its version, and a per-tool rule would ask again for each. The tools
run only on the person's own computers, in a program they enabled there
("Use Blender on this computer"), and only through their own consent and
their own rule; what code in Blender can reach is the person's own
computer, which they chose to let Magnus use. Exfiltration after reading
outside text still asks (case 3), and an organisation or a person who
wants every call asked writes a forbid, which this cannot answer. Running
code still asks the first time.

The card's sentence names the program ("Magnus wants to run python in
Blender."), not its app id.

Tests: `desktop::tests::only_the_organisations_ask_may_be_allowed_always`,
`desktop::tests::allow_always_on_a_programs_ask_is_for_that_program`,
`desktop::tests::by_default_code_in_a_program_asks_the_person`,
`asks::tests::a_programs_own_ask_offers_allow_always_for_the_program`,
`asks::tests::no_ask_outside_the_four_can_be_produced`,
`rules::tests::a_program_rule_covers_only_that_programs_tools`,
`rule_effects::tests::a_persons_ask_first_replaces_a_programs_own_ask`,
`desktop/tests/asks-check.mjs`.

Narrowed the same day (decisions/2026-11-19-code-asks-once-per-task.md): a tool that runs code asks once per task with its own ask, which Allow always for the program does not answer, unless the request asked for that code.
