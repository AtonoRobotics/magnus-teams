# Erasure forgets a person's document counts and removed conversations

Extending the erasure test to every domain found two places that kept an
erased person after `PersonErased`:

- The library's version count per document path (`bundle_last`) kept their
  own folder's paths (`/users/<their segment>/…`). It was kept on purpose,
  so a path written again after a removal counts on and (path, version)
  names one version in the log. An erased person's id is never given again
  (2026-11-11-an-erased-persons-id-is-not-given-again.md), so nobody writes
  under their folder after them: dropping those counts cannot reuse a
  version, and the erasure leaves nothing of theirs. Other paths keep
  counting through any removal as before.
- Conversations removed for good from the trash (`removed_threads`) were a
  set of thread ids, forgotten only through the person's tasks, which a
  removal had already taken out. Each now records whose it was, so the
  erasure forgets it. The state is rebuilt by replay, so the shape changes
  with no migration.

The choice follows from the spec's erasure promise and is the safer
reading. The test (`forget_tests.rs`) seeds a person into every domain
and fails without either fix.
