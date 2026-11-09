# Mixed rules validate against the served schema (#403)

Codex P2 on #401: a rule naming actions from two sides (e.g.
`execute` and `act`) was assigned to both partitions but validated
against halves that each omit the other's action, so no mixed rule
could be applied.

A rule bearing on several sides now validates against the served
schema, which declares every entity and action. Single-side rules
keep half-schema validation, so their errors stay side-precise;
unknown actions still fail (the served schema does not declare
them either).
