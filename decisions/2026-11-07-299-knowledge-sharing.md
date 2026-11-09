# #299: share a person-only entry or org document

Since #258, runs file for the person only, but nothing let the person
share afterwards. Now they can: entries through
`POST /v1/knowledge/{id}/share`, org documents through the `bundle_share`
chat tool. Each surface mirrors its object's existing person-facing one
(entries have person HTTP; docs surface through chat and the admin
console).

## Rules

- Entries: any current reader may share (the only stated precondition is
  having seen the text), adding group principals to every readers clause,
  or clearing all clauses for everyone. Widening only: an unchanged share
  is refused.
- Documents: only the writer may share (only they can see it), adding
  principals or everyone. User-scope documents cannot be shared.
- A new document version clears the share: a share covers only the text
  seen. Entries are immutable, so theirs stands.
- `bundle_share` is chat-native like the controls (`is_chat_tool`), and
  so refused in tasks: widening access from conversation is the person's
  say, never a background run's.

## Record

`KnowledgeShared`/`BundleShared` events carry who shared; the entry's
shown form and the doc's admin form keep the last share. The search index
re-indexes on share, so row security lets the new readers through.
