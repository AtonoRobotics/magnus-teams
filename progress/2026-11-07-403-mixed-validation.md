# Mixed-rule validation (#403)

`policy_admin::check` routes multi-side rules to the served schema
instead of the halves. Unit test
`a_mixed_tool_and_loop_rule_validates` fails before and passes
after; lib 115/115.
