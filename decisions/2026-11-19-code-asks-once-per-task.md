# Code in a program asks once per task, unless the request asked for it

The owner's ruling (2026-11-19) on "Allow Blender always": code a program
runs can reach the whole computer, so it is not covered by Allow always for
the program. It asks once per task, unless the person's request asked for
that code specifically.

- A program's tool that runs code (every tool not classified otherwise in
  `desktop::PROGRAMS`) now asks with its own rule, `desktop::CODE_ASK`
  (`kernel:program_code`), where only the organisation's layer asks. Case 4.
  The card offers Allow and Allow for this task, never Allow always: Allow
  for this task covers the rest of the task's code, so a task asks at most
  once. "Allow Blender always" (`PROGRAM_ASK`, `program_rule`) answers only
  the program's tools that run no code.
- Before the card goes up, Auto Review reads the action with the standing
  "allow it only if what the person asked specifically asks for running
  code, a script or a macro in this program" (`Kernel::code_asked_for`).
  Allow runs it with no card, recorded as "the person's request asked for
  this code to run"; Deny stands; unsure or unreachable raises the ask, which
  is the default here.
- A lower layer's forbid still asks as `ASK_RULE`, and the person's own
  ask-first rule takes the place of `CODE_ASK` (`rule_effects::kernel_ask_stands`),
  so neither is answered by the request.

Every supported program's tools are unclassified today, so until a
program's server is pinned and its tools classified, each of its tools is
code and Allow always for the program covers none of them.

Tests: `desktop::tests::code_in_a_program_is_allowed_one_task_at_a_time`,
`only_the_organisations_ask_may_be_allowed_always`,
`by_default_code_in_a_program_asks_the_person`,
`asks::tests::no_ask_outside_the_four_can_be_produced`,
`rule_effects::tests` (`kernel_ask_stands`).
