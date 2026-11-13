# Close the deep-review findings

Fixes apply to the current Magnus Teams specification and preserve its agent
behavior. No production deployment is included in this branch.

- Display tickets retain the issuing session and control generation. Every input
  and output checks current membership, session and controller; handback,
  replacement, removal and sign-out invalidate an existing connection.
- Surrogate credentials require verified HTTPS on port 443. Responses are decoded
  from HTTP framing before scrubbing. Ambiguous framing is rejected; credential
  requests ask for identity encoding and reject a compressed response. Reframed
  output does not retain stale content lengths or chunk sizes.
- Auto Review keys include the current request, effect, classifier finding and run.
  Legacy organization/group policy writes use the existing admin step-up/audit gate.
- Tester consent does not authorize developer access to workspace or whisper runs,
  including descendants.
- Held-file copies and their completion markers commit together. Removal revokes
  access immediately while failed preservation remains durable and retryable.
  Workspace deletion stops if preservation fails.
- Team uploads validate scope before writing, then publish an immutable object
  pointer, metadata and project scope in one event. Existing File events remain
  readable. Readers never see new bytes under an old or default scope.
- Model calls reserve their priced maximum under one writer lock before sending.
  Reservations and receipts survive restart, cover server/person/task/workspace/
  channel allowances, and include auxiliary reviews. Unknown provider charges
  retain the reservation instead of silently freeing the allowance. New charges
  belong to the month of the call; existing spend retains its historical reading.
- Team drafts are transient and cleared across sign-out, account/server changes
  and restart. Old unscoped persistent entries are removed.
- Membership mutations, invitation joins and last-owner checks are serialized.
- Workspace OAuth uses its own vault, refresh, tool-cache and broker principal.
  Team Admins can connect it in People > Workspace apps. Completion is bound to
  the initiating admin/workspace/app and rechecks current authority. Execution,
  tool discovery, previews and read-backs use the workspace identity; personal
  credentials are never a fallback. Disconnect/delete revoke that identity only.

Regression tests exercise the real authorization, storage, event-log and budget
boundaries. They do not substitute fake model behavior. Builds and tests run on
the existing Strix Beta runner in a branch-only workflow, without publishing or
deploying a release. Protected invariant files remain unchanged.
