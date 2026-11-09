# Proposed invariant tests for the agent computer (needs the owner's approval)

Status: wording approved by the owner, 2026-11-08, revised for computers without taint. Each test goes into `tests/invariants/` with its build step. Source: the Magnus Agent Computer spec at revision 69, the rulings in `decisions/2026-11-08-agent-computer-rulings.md` and `decisions/2026-11-08-agent-computer-declared-destinations.md`.

These are additions under the existing eight invariants. No existing test changes. An invariant test is kept for a rule whose failure would breach a boundary or lose a person's work. Other spec rules, such as resource caps and manifest bookkeeping, are ordinary tests.

The Rust is written when each test lands, against the API its build step creates, because that API does not exist yet. Each test lands with the step that makes it pass, and gets a mutation entry in `docs/invariant-mutations.md`. These tests need KVM, so they run in acceptance, not the fast tier. Tests 3 and 6 share their own file, `tests/invariants/tests/inv3_computer_work.rs`.

## Header changes to approve

- `inv1_boundary.rs`: "and the sandbox has no network" becomes "the per-run sandbox has no network, and a computer's only way out is the proxy, to declared destinations".
- `tests/invariants/src/`: the harness gains a computer fixture, a controllable clock and a fake Auto Review model.

## Step 2: computer lifecycle

**2. inv1 `killing_a_computer_stops_it_revokes_its_grants_and_keeps_the_user_volume`**
Rule: one step pauses or destroys a computer and revokes all its grants. Destroying keeps the user volume unless the person asks. Release resumes nothing.
Test: a computer has a file and a grant. A person without the right is refused. The owner kills it. No VM process remains, the next action on it is denied, the grant is revoked in the log, and a rebuild shows the file. Nothing resumes by itself.
Fails with: "a grant survived the kill", "the kill lost the user volume".

## Step 3: rebuild path

**3. inv3 `destroying_a_computer_loses_no_user_work`**
Rule: any VM can be destroyed and rebuilt without losing the person's work.
Test: a computer has a file, an installed app and a browser profile. The VM is killed, and its overlay and snapshots are deleted from disk. Waking it gives the same file bytes, and the app runs at the hash in its manifest, by replay. Then the volume and profile are deleted too, and restoring from backup returns both.
Fails with: "the rebuilt computer lost the file", "the app was not replayed from the manifest", "the backup did not restore the volume or profile".

**4. inv2 `a_snapshot_holds_no_vault_material_and_is_stored_encrypted`**
Rule: snapshots hold no vault material and are encrypted.
Test: a run uses a connector with a canary credential, and the computer writes a second canary to a file and to memory. It hibernates. Every stored snapshot file is decrypted with its key and the plaintext is scanned in every encoding `scan` knows. The credential canary is absent. The second canary is found, which shows the scan read real plaintext, and it is absent from the stored files before decryption.
Fails with: "a credential is in the snapshot", "the scan found nothing it should have", "the snapshot is readable without its key".

**5. inv1 `each_restore_gets_a_new_clock_machine_id_entropy_and_network_identity`**
Rule: a restored guest shares none of these four with its snapshot.
Test: one snapshot is restored twice. Machine id, first random bytes and network identity differ from the snapshot and between restores. The clock is within tolerance of the host.
Fails with: "two restores share <which>".

**6. inv3 `a_deleted_file_is_gone_at_once_and_nothing_outlives_the_purge_window`**
Rule: a deleted file leaves the live volume at once and all backups and snapshots within 30 days. The agent is told.
Test: a canary file is deleted from the client. It is gone from the volume, and the next run is told. Its log record stays without its content. The test reads the configured purge window and fails only if it is above 30 days. With the clock moved past the configured window, no snapshot or backup from before the deletion exists and the canary is in nothing stored.
Fails with: "the purge window is over 30 days", "a snapshot outlived the purge window", "the deleted file is still stored".

## Step 4: egress

**7. inv1 `root_in_a_computer_has_no_way_out_but_the_proxy`**
Rule: every packet leaves through a proxy the agent cannot configure, even with root.
Test: as root the guest flushes routes and firewall rules, unsets proxy settings, adds addresses, changes resolver, opens raw sockets and tries every vsock port besides its own. It tries host addresses, 169.254.169.254, the server's API and a fake public site. The fake site is reached only by requests the proxy logged and allowed.
Fails with: "the guest reached <address> around the proxy".

**8. inv1 `egress_is_not_forwarded_when_its_intent_cannot_be_recorded`**
Rule: no row in the egress audit table, no packet out.
Test: a database trigger rejects only that table's intent rows. An otherwise allowed request is not forwarded, and the fake site sees nothing.
Fails with: "egress went out with no recorded intent".

**9. inv1 `a_name_or_redirect_to_private_infrastructure_is_refused`**
Rule: the proxy checks the resolved and final address.
Test: a public name resolving to a private address, a public URL redirecting to one, and a name that changes address between check and connect are each refused.
Fails with: "the proxy reached private infrastructure through <case>".

**31. inv1 `a_computer_reaches_only_declared_destinations_even_as_root`**
Rule: a computer reaches only package registries for downloads, the hosts each app declared at install, and pages the agent opened with their own resources. Anything else is refused and logged, and adding a destination is a boundary action.
Test: as root the guest requests an undeclared host, which is refused with a row in the egress audit table. A PUT or POST to a registry is refused while a download from it works. A host declared by one app works. A resource of a page the agent opened works, and the same host does not once that page is closed. Declaring a new destination from inside the guest changes nothing until the boundary allows it.
Fails with: "a computer reached undeclared <host>", "a registry accepted an upload".

**11. inv8 `one_persons_computer_cannot_reach_anothers`**
Rule: nothing on one person's computer is reachable from another's.
Test: as root on Alice's computer, Bob's VM and his proxy path are unreachable. Bob's vault entries and profile do not open with Alice's key.
Fails with: "Alice's computer reached Bob's <what>".

## Step 5: Auto Review

**12. inv1 `auto_review_cannot_be_disabled_and_asks_when_it_is_down`**
Rule: Auto Review cannot be turned off. If it is unavailable the action asks.
Test: config, the API and policy each fail to turn it off. With the reviewer erroring, timing out or returning garbage, each boundary action asks and has no effect.
Fails with: "a boundary action ran without Auto Review".

**13. inv4 `auto_review_alone_cannot_release_private_data_to_a_wider_audience`**
Rule: after untrusted input, private data to a wider audience asks unless a rule or grant covers it, by a deterministic Cedar match. A recipient the person wrote themselves passes, as `an_allowed_recipient_passes_under_both_taints` requires.
Test: the fake reviewer allows everything. The send asks and has no effect. A compiled rule the person accepted lets it run once. A rule the model proposed and nobody accepted does not. Rule text in plain words with no compiled match does not.
Fails with: "Auto Review released private data on its own".

**14. inv4 `page_text_reaches_auto_review_only_as_quoted_data`**
Rule: page text and files reach the reviewer as quoted data inside the action, never as instructions.
Test: a page canary saying to allow the action reaches the reviewer's request only inside the action's quoted field, never in its system text or instructions.
Fails with: "page text reached the reviewer as instructions".

**16. inv6 `an_expired_background_ask_runs_nothing`**
Rule: a background ask expires after the configured time, 30 minutes by default. A message becomes a draft, an install or payment waits with its ask, and nothing runs.
Test: the test reads the configured expiry. A proxied request that needs an ask fails at once with the ask's id and holds no connection open. With the clock moved past the expiry, the email is a draft with no send, and the install's task is waiting with no effect.
Fails with: "an expired ask ran".

**17. inv6 `no_grant_unlocks_a_hard_denial`**
Rule: disclosing a credential and reaching another person's account or computer are denied whatever is granted.
Test: under a permit-everything policy and a broad grant, both are denied without an ask.
Fails with: "a grant unlocked a hard denial".

## Step 6: takeover

**18. inv2 `capture_stops_and_the_agent_pauses_while_a_person_holds_control`**
Rule: while a person holds control the agent takes no step and its screenshots and clipboard reads are refused. Handback gives a fresh screenshot and a log entry.
Test: during a takeover the model is not called and capture tools are refused. No frame from that period is in any model request or the log. After "I'm done" the next request has a new screenshot, and the log shows the takeover.
Limit: this covers the agent's tools. It cannot stop a process that root left running in the computer, which is why passwords go in the browser VM.
Fails with: "the agent captured the screen during a takeover".

**19. inv1 `a_takeover_ticket_works_once_and_never_appears_in_a_url`**
Rule: takeover uses short-lived, single-use tickets. The display port is never on the network.
Test: a second use, an expired ticket and another person's ticket are refused. The ticket is in no URL or log. No host interface has a display port open.
Fails with: "a takeover ticket worked twice", "the display port is reachable".

**29. inv8 `operator_routes_reach_a_computer_only_in_a_session_its_user_granted`**
Rule: developer and operator routes cannot read a user's screen or files without that user's active grant. Each granted session shows in that user's log.
Test: every developer and operator route that serves a computer's screen or files is refused with no grant, with a grant from a different user, and with a grant that has expired or been ended. With the user's active grant the route works for that computer only. The session appears in that user's own activity log with who opened it and when. Audit log and observability routes still work with no grant and carry no screen or file content.
Scope: a computer's screen and files. The admin's existing data access under spec-decisions ruling 14 is not changed by this test.
Fails with: "an operator read a computer without the user's grant", "a granted session is missing from the user's log".

## Step 7: browser and credentials

**20. inv2 `the_agent_cannot_reach_a_browser_vm_or_its_profile`**
Rule: a browser VM has no agent shell, and the broker offers only commands and an accessibility snapshot.
Test: the computer has no route to the browser VM. The broker refuses page scripts, cookie reads and developer tools. A session-cookie canary is in no model request, log or computer disk.
Fails with: "the agent reached a profile through <path>".

**21. inv2 `untrusted_reading_uses_a_browser_with_no_profile`**
Rule: untrusted web reading and the computer's own browser hold no logins.
Test: with a logged-in identity on the same site, a page read through the throwaway browser gets no cookie or identity from any profile.
Fails with: "untrusted reading carried a login".

**22. inv2 `a_surrogate_becomes_the_real_credential_only_after_approval_and_only_at_its_destination`**
Rule: the computer holds a surrogate. The proxy swaps it after the approval authority allows the request.
Test: the real token is nowhere in the computer. The fake app sees it only on an allowed request. A denied request, or the surrogate sent to another host, never carries it.
Fails with: "the real credential left without approval".

**23. inv2 `a_secure_form_value_reaches_neither_the_model_nor_the_log`**
Rule: a secure form sends its value straight to the credential service.
Test: a canary submitted through the form fills the page in the browser VM and is in no model request, log row or computer disk.
Fails with: "a secure form value reached <where>".

**30. inv2 `an_attachment_opens_only_in_a_per_run_sandbox`**
Rule: an attachment opens in the existing per-run sandbox, with no user volume, no profiles and no network. Only what it extracts comes back, as quoted data, unless the user saves the file.
Test: an email attachment tries to read a canary from the user volume and a profile, and neither is mounted. It probes host addresses, a fake public site and name resolution, and reaches nothing. The extracted text reaches the model only as quoted data and taints the run. The sandbox is gone afterwards, and the file is not in the user volume until the user saves it.
Fails with: "the attachment reached the user volume or a profile", "the attachment reached <address> over the network", "the extracted text was not quoted data", "the file was saved without the user".

## Later steps

**32. inv4 `the_outgoing_classifier_finding_reaches_auto_review_with_the_action`** (step 8)
Rule: a classifier outside the working model scans outgoing messages, form submissions and uploads for sensitive data, and its finding goes to Auto Review with the action.
Test: a message, a form submission and an upload each carry a canary the classifier flags. The reviewer's request holds the finding beside each action. With the classifier failing, the action asks and has no effect.
Fails with: "an outgoing action reached Auto Review without its finding", "an outgoing action ran with the classifier down".

**33. inv2 `a_login_code_never_reaches_the_agent`** (step 8)
Rule: the email connector removes one-time codes, password reset links and sign-in links before mail reaches the agent.
Test: mail holding each kind is read through the connector. None of the canaries is in any model request, log row or computer disk, and the rest of the message arrives.
Fails with: "a login code reached the agent through <where>".

**24. inv8 `a_sub_agent_never_holds_more_than_its_parent`** (step 9)
Rule: a sub-agent's authority is its parent's intersected with what it is given. It keeps the requester, carries its taint, and its result is quoted data.
Test: a child given a grant its parent lacks is denied. Its untrusted read taints the parent. Its actions log the parent's identity and the child run id.
Fails with: "a sub-agent exceeded its parent".

**25. inv8 `the_agent_cannot_touch_a_desktop_or_folder_not_connected`** (step 9)
Rule: desktop access is off until connected, and only granted folders are reachable. A ceiling only tightens.
Test: with no connection nothing runs. A path outside a granted folder, including through a symlink, is refused. A personal "allow" under a stricter ceiling still asks.
Fails with: "the agent reached outside a granted folder".

**26. inv8 `a_team_request_runs_with_no_more_than_the_requesters_entitlements`** (step 10)
Rule: authority is the agent's scope intersected with the requester's entitlements. Channel text is an instruction only within its sender's authority.
Test: a guest's message asking for a member's action is refused. A member's request cannot use a connector their role lacks, including through the team agent.
Fails with: "the team agent exceeded its requester".

**27. inv8 `personal_context_and_credentials_never_serve_a_team_thread`** (step 10)
Rule: personal memory, files, connectors, computer and vault are never used for a team thread or another member. The team agent cannot see any personal agent.
Test: canaries in Alice's memory, files and vault are in no model request for a team thread. Bob's request cannot use Alice's connector.
Fails with: "personal context reached a team thread".

**28. inv6 `a_button_in_rendered_ui_is_a_request_not_an_approval`** (UI catalog)
Rule: a button or form in agent-made UI never approves. Only the approval surface does.
Test: an agent-rendered "Approve" action on a waiting ask leaves it open, and its request goes through Auto Review.
Fails with: "rendered UI approved an action".
