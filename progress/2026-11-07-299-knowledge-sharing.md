# #299: share a person-only entry or org document

## Changed

- `crates/magnus/src/events.rs`, `state.rs`: `KnowledgeShared`
  (new readers, by) and `BundleShared` (added principals, everyone, by)
  events with projection; entry keeps last `shared`, doc keeps
  `shared`/`shared_everyone`/`shared_by`, cleared by each new version.
- `crates/magnus/src/knowledge.rs`: `POST /v1/knowledge/{id}/share`
  (reader-only, widening-only, re-indexes); shown entries carry `shared`.
- `crates/magnus/src/bundle.rs`: `bundle_visible` honours shares;
  `bundle_share` (writer-only, org-only) plus the `bundle_tool` arm.
- `crates/magnus/src/tools.rs`, `instructions/tools.json`: `bundle_share`
  builtin (ReversibleWrite) and chat spec with description.
- `crates/magnus/src/boundary.rs`: dispatch; `is_chat_tool` covers
  `bundle_share` so chat may call it and tasks may not.
- Tests: entry share with a group via HTTP (404 for non-readers, row
  security lets the group through, log row, idempotent 409s, everyone);
  doc share in conversation (staff reads and finds, non-writer refused,
  new version clears).

## Verification

- New tests RED on pristine code (404 route, denied tool), GREEN with it.
- `knowledge` 12/12, `org_context_bundle` 10/10, `magnus` lib 98/98.
- No new clippy lints on touched lines.
