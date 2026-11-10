# Auto Review is told the action's effect

Spec rev 140 ("Permissions and tasks", Auto Review): the reviewer "sees the request, the action's target, effect and content, and the person's rules." Its document held the tool, the destination and the content, but not the effect. It now names it as `action.effect`: the tool's class (read, reversible write, irreversible, financial, sandbox), the same class the boundary and the policy use.

The reason is the spec's wording, and that a reviewer cannot weigh "when the action would be hard to undo" (its own instructions) without knowing whether it can be undone. The cache key is unchanged: the effect follows from the tool, which is already in it.

The invariant suite's stand-in reviewer reads it too: it lets through what the owner's policy permits, and the default policy permits by effect (reads and reversible writes), so without the effect it could not tell a permitted team note from an irreversible send. See `decisions/2026-11-10-invariant-patches-applied.md`.

Test: `auto_review::tests::the_document_names_the_actions_effect`.
