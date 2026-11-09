# Invariant mutation check

Date: 2026-10-28. Branch: `worktree-agent-ae7e52c66eb932cae`, at commit 87d75e3.

Question: does each invariant test fail when the kernel has the bug that test exists for?

Method: for each test in `tests/invariants/tests/inv*.rs`, one small mutation in kernel code (`crates/magnus/src`). A mutation is a dropped check, an inverted condition, a skipped log write or a removed redaction. For each one: rebuild (`cargo build -p magnus-invariants --bins --tests`), run that one test (`cargo test -p magnus-invariants --test <file> -- <name> --exact`), then revert (`git checkout -- crates`). Nothing under `tests/invariants/` was changed. KILLED means the test failed (the first line of the failing assertion is shown). SURVIVED means the test still passed. Rows marked X are extra mutations, added to probe a test further. The exact diffs are in the appendix.

Baseline (unmutated, `cargo test -p magnus-invariants --no-fail-fast`): 45 of 45 pass. No test was flaky. The two idempotency mutants of `repeated_crashes_never_repeat_an_effect` depend on timing, so each ran three times. They survived all three times.

## Results

| Inv | Test | Id | Mutation | Where | Result |
|---|---|---|---|---|---|
| 1 | sandbox_has_no_network_device_and_reaches_nothing | M01 | `sandbox_exec` runs the command on the host, not in the microVM (a no-VM fallback) | sandbox.rs:144 | KILLED: "the guest has a network device besides loopback" |
| 1 | action_is_not_executed_when_its_intent_cannot_be_recorded | M02 (core) | `emit()` turns a failed log append into a warning and carries on | kernel.rs:117 | KILLED: "the send executed while the log refused writes" |
| 1 | action_is_not_executed_when_its_intent_cannot_be_recorded | X01 | only the boundary's intent write is best effort: the app call goes ahead without a logged intent | boundary.rs:318 | **SURVIVED** |
| 1 | missing_credential_denies_the_action | M03 (core) | a missing credential no longer denies; the call goes out without one | boundary.rs:299 | KILLED: "the app was called without its credential" |
| 1 | prohibited_tools_never_run_even_if_policy_permits_everything | M04 | the hard Deny for the `prohibited` class is dropped | boundary.rs:414 | KILLED: "a prohibited tool ran" |
| 1 | tools_the_owner_has_not_classified_cannot_run | M05 (core) | an unclassified app tool defaults to `reversible_write` | boundary.rs:82 | KILLED: "an unclassified tool ran" |
| 1 | kill_switch_stops_actions_and_release_resumes_nothing | M06 | releasing the kill switch resumes every run it held | api.rs:651 | KILLED: "a run resumed by itself after the kill switch was released" |
| 1 | a_changed_tool_definition_is_disabled_and_hidden | M07 | a changed tool definition is re-pinned (accepted) instead of disabled | run/mod.rs:202 | KILLED: "a tool ran after its definition changed" |
| 2 | credentials_are_used_but_never_seen | M08 | the boundary no longer redacts app results | boundary.rs:327 | SURVIVED (equivalent: the MCP client in apps.rs redacts the same text) |
| 2 | credentials_are_used_but_never_seen | M08b (core) | the central `redact()` has its short-secret guard inverted (`<` to `>`), so no real credential is redacted anywhere | vault.rs:118 | KILLED: "app credential reached the model in request 2" |
| 2 | secrets_can_be_written_but_not_read_back | M09 | `GET /v1/secrets` returns values beside names | api.rs:621 | KILLED: "GET /v1/secrets returned a secret value (200 OK)" |
| 2 | secrets_can_be_written_but_not_read_back | X02 | any signed-in person may write a secret (owner check dropped) | api.rs:627 | KILLED: "a person without the owner role wrote a secret: 204" |
| 3 | the_kernel_cannot_rewrite_the_log | M10 | `migrate` grants the kernel role UPDATE and DELETE on `events` | log.rs:71 | KILLED: "the kernel role may run: DELETE FROM public.events" |
| 3 | content_is_kept_beside_the_log_not_in_it | M46 | a task's text is not listed as content (`events::CONTENT`), so it stays in the log | events.rs:671 | KILLED: "the task's text is in the event log" |
| 3 | a_removed_items_content_is_gone_and_its_record_stays | M47 | removing an item records the removal but never deletes its content | trash.rs:63 | KILLED: "the removed task's text is still in the store" |
| 3 | a_running_task_cannot_be_deleted | M48 | a task's run counts as ended while it still waits, so it may go to the trash | state.rs:619 | KILLED: "a waiting task went to the trash" |
| 3 | crash_after_a_side_effect_does_not_repeat_it | M11 | the idempotency key also hashes the process id, so it changes across a restart | boundary.rs:103 | KILLED: status was `failed`, not `completed` (the resend made a second email, so the contract check failed) |
| 3 | crash_after_a_side_effect_does_not_repeat_it | M14 | the `Idempotency-Key` header is never sent | boundary.rs:589 | KILLED: same assertion as M11 |
| 3 | crash_against_an_app_without_idempotency_asks_instead_of_resending | M12 (core) | after a crash, every app call is resent, whether or not the app honours keys | boundary.rs:541 | KILLED: "timed out waiting for: an inbox item asks about the unknown outcome" |
| 3 | all_state_is_rebuilt_from_the_event_log_and_its_content | M13 | inbox items go into the projection but are never appended to the log | kernel.rs:117 | KILLED: "inbox did not survive on the log and its content" |
| 3 | repeated_crashes_never_repeat_an_effect | M14 | the `Idempotency-Key` header is never sent | boundary.rs:589 | **SURVIVED** (3 of 3 runs) |
| 3 | repeated_crashes_never_repeat_an_effect | M11 | the idempotency key changes across a restart | boundary.rs:103 | **SURVIVED** (3 of 3 runs) |
| 4 | injected_page_cannot_exfiltrate_private_data | M15 | Rule of Two applies only to built-in tools, not to app actions | boundary.rs:443 | KILLED: "timed out waiting for: open approval item" (the send ran instead) |
| 4 | the_person_can_take_the_risk_by_approving_out_of_band | M16 | no approval of any scope satisfies a kernel (taint) ask | boundary.rs:476 | KILLED: "timed out waiting for: the approved send ran" |
| 4 | content_claiming_authority_changes_nothing | M17 (core) | `web_fetch` results are labelled principal, not untrusted (taint dropped) | tools.rs:123 | KILLED: "timed out waiting for: open approval item" (the send ran instead) |
| 4 | injected_page_cannot_exfiltrate_private_data | M17 | as above | tools.rs:123 | KILLED: same |
| 4 | content_claiming_authority_changes_nothing | M31 | the agent's policy proposal takes effect at once | run/mod.rs:329 | **SURVIVED** |
| 4 | taint_survives_a_crash | M18 | a tool result's taint label is not written to the log (`skip_serializing`); it lives only in memory | events.rs:185 | KILLED: "timed out waiting for: open approval item" |
| 4 | untrusted_input_with_an_external_action_is_allowed | M19 | Rule of Two fires on untrusted input alone | boundary.rs:443 | KILLED: "two of three must not be blocked" |
| 4 | private_data_with_an_external_action_to_a_private_audience_is_allowed | M20 | the audience rule also asks for private destinations (`>` to `>=`) | boundary.rs:451 | KILLED: "two of three must not be blocked" |
| 4 | private_data_sent_to_a_wider_audience_asks | M21 | the audience rule asks only for public destinations, not internal ones | boundary.rs:451 | KILLED: "timed out waiting for: open approval item" |
| 4 | a_fetch_cannot_carry_private_data_out_after_untrusted_input | M22 | a web fetch no longer counts as sending data out | boundary.rs:440 | KILLED: "timed out waiting for: open approval item" |
| 5 | every_app_call_traces_to_a_model_decision | M23 | the kernel adds a field to the model's arguments for state-changing app calls | boundary.rs:590 | KILLED: "the kernel called update_record {...idempotency_key...} on its own" |
| 5 | a_denial_goes_back_to_the_model_which_decides_what_next | M24 (core) | a policy denial ends the run (the kernel writes Failed) | run/agent_loop.rs:54 | KILLED: "the model was asked again after the denial" |
| 5 | an_exhausted_budget_suspends_and_notifies | M25 | off-by-one in the outer model-call budget check (`>=` to `>`) | run/agent_loop.rs:62 | SURVIVED (equivalent: the inner check in the retry loop still stops at the limit) |
| 5 | an_exhausted_budget_suspends_and_notifies | M25b | the same off-by-one in both budget checks | run/agent_loop.rs:62, :82 | KILLED: "the model was called past its budget" |
| 5 | a_model_outage_suspends_the_run_and_resume_uses_the_pinned_model | M26 | a persistent provider outage fails the run instead of suspending it | run/agent_loop.rs:169 | KILLED: "a provider failure must suspend, not end, the run" |
| 5 | a_reply_without_a_tool_call_does_not_end_the_run | M27 (core) | a reply with no tool call ends the run as Completed | run/agent_loop.rs:126 | KILLED: "the run ended on a text reply" |
| 5 | the_principal_can_cancel_and_nothing_else_ends_a_run | M28 (core) | the kernel fails a run that waits more than 30 days | run/mod.rs:356 | KILLED: status was `failed`, not `suspended` |
| 5 | the_principal_can_cancel_and_nothing_else_ends_a_run | X03 | cancel is refused unless the run is running | api.rs:543 | KILLED: "assertion failed: st.is_success()" |
| 6 | without_owner_policy_an_irreversible_action_waits_for_approval | M29 | the built-in default org policy lets `irreversible` execute | policy.rs:26 | KILLED: "timed out waiting for: open approval item" |
| 6 | owner_policy_grants_autonomy | M30 | the kernel always asks before irreversible actions, whatever the policy | boundary.rs:436 | KILLED: send count 0, expected 1 |
| 6 | the_agent_proposes_and_only_the_owner_grants | M31 (core) | the agent's policy proposal takes effect at once (provisionally, pending review) | run/mod.rs:329 | KILLED: "timed out waiting for: open approval item" (the send ran instead) |
| 6 | a_one_action_approval_covers_one_action | M32 | a `once` approval is not checked for having been used | boundary.rs:478 | **SURVIVED** |
| 6 | a_one_action_approval_covers_one_action | X04 | `once` is matched like `task` (merged match arms: no use or input check) | boundary.rs:478 | KILLED: "timed out waiting for: the first send ran" (both sends ran at once, so the count went from 0 to 2) |
| 6 | the_approval_channel_needs_a_person | M33 | an unknown bearer token falls back to the owner | api.rs:70 | KILLED: the forged token got 200, expected 401 |
| 6 | people_set_policy_only_within_their_role | M34 | the user-layer check uses `\|\|` instead of `&&` | inbox.rs:27 | KILLED: "user/alice accepted a policy from someone without the role: 200" |
| 7 | no_completion_and_no_action_without_a_contract | M35 | the contract-before-acting check tests the wrong issuer (`Check` for `Model`) | boundary.rs:421 | KILLED: "an action ran before the agent wrote its done-criteria" |
| 7 | a_false_report_of_completion_is_caught_by_checking_the_world | M36 (core) | a check passes if its tool ran without error; the expectations are ignored | run/mod.rs:483 | KILLED: "the run continued after the false report" |
| 7 | completion_reflects_the_world_at_the_time_of_checking | M37 | a check reuses the run's latest structured result from the same app instead of reading the world | run/mod.rs:444 | KILLED: "the report was trusted over the world" |
| 7 | a_criterion_without_a_check_goes_to_the_person_for_review | M38 | with the verifier off, unchecked criteria pass with no review card | run/mod.rs:506 | KILLED: "timed out waiting for: open review item" |
| 7 | a_criterion_without_a_check_goes_to_the_person_for_review | X05 | a rejected review fails the run (the kernel writes Failed) instead of going back to the agent | inbox.rs:201 | **SURVIVED** |
| 7 | checks_may_only_read | M39 | `task_contract` accepts a check that uses a writing tool | run/mod.rs:397 | KILLED: "a contract with a writing check was accepted" |
| 8 | a_user_layer_cannot_widen_the_organization_layer | M40 (core) | a permit in a lower layer widens the org layer | policy.rs:134 | KILLED: "timed out waiting for: open approval item" (the send ran instead) |
| 8 | an_organization_forbid_wins_over_any_lower_permit | M41 | a forbid becomes an approval request instead of a deny | policy.rs:148 | KILLED: "a forbidden action was offered for approval" |
| 8 | a_user_layer_narrows_for_that_user_only | M42 | every user layer applies to every person | policy.rs:98 | KILLED: "bob's layer narrowed alice" |
| 8 | a_group_layer_narrows_its_members | M43 | group layers are left out of the evaluation | policy.rs:92 | KILLED: "the finance group's layer did not apply to bob" |
| 8 | the_agent_never_exceeds_the_persons_own_access | M44 | the owner role bypasses an app's access groups | boundary.rs:96 | KILLED: "the agent used access alice does not have" |
| 8 | only_holders_of_the_approver_role_can_approve | M45 | the person who asked for the task may approve its actions | inbox.rs:37 | KILLED: "carol approved without the approver role: 200 OK" |

Where a result says "timed out waiting for: open approval item", the mutated kernel let the action run when it should have asked. So the approval the test waits for never came.

## Core property per invariant

| Inv | Core mutation | Result |
|---|---|---|
| 1 boundary fails closed | M02: log write failures are ignored (also M03 missing credential, M05 unclassified tool) | KILLED |
| 2 credential reaches model or log | M08b: redaction disabled at its single source | KILLED |
| 3 side effect repeats after a crash | M12: resend after a crash to an app without idempotency | KILLED |
| 4 taint dropped | M17: web content not marked untrusted | KILLED |
| 5 harness ends or redirects a run | M24 (deny ends run), M27 (text reply ends run), M28 (long wait ends run) | KILLED |
| 6 agent grants itself policy | M31: proposals take effect at once | KILLED |
| 7 completion accepted from the agent's report | M36: check expectations ignored | KILLED |
| 8 lower layer widens a higher one | M40: lower-layer permit widens org | KILLED |

## Summary

56 mutant runs (52 mutants, some run against two tests). 48 killed, 8 survived. Every test except one killed at least one mutation aimed at it, so 44 of 45 tests catch their bug. Every invariant's core-property mutation was killed.

Two survivors are equivalent mutants, not test gaps. A second guard in the kernel hides the mutation, and attacking that shared guard is killed:
- M08: the MCP client (`apps.rs`) redacts the same result again. M08b breaks the shared `redact()` and is killed.
- M25: the inner budget check in the retry loop still stops the run at the limit. M25b changes both checks and is killed.

The other six are findings: the test fails to observe the bug.
- **repeated_crashes_never_repeat_an_effect** survives M14 (no `Idempotency-Key` sent) and M11 (key changes across restarts), 3 of 3 runs each. A repeat needs a kill while an app call is in flight. That window is milliseconds, and the whole run takes about 2 to 3 seconds, while restarts come at random 0.2 to 1.5 s intervals. So the restarts almost never land in that window. `crash_after_a_side_effect_does_not_repeat_it` kills both mutants because it holds the call in flight on purpose.
- **action_is_not_executed_when_its_intent_cannot_be_recorded** survives X01 (only the boundary's intent write made best effort, so the app call proceeds without a logged intent). The fake model revokes INSERT while it is answering. So the kernel's next write, which records the model's reply, fails first, and the run stops before it reaches the boundary. The test never exercises the intent-first rule itself. It only kills a kernel that ignores every failed write (M02).
- **a_one_action_approval_covers_one_action** survives M32 (a `once` approval is not marked as used). The two sends have different bodies, so the input-hash check alone keeps the second one waiting. The test does not check an identical repeated action.
- **a_criterion_without_a_check_goes_to_the_person_for_review** survives X05 (a rejected review makes the kernel fail the run itself). The fake model's next step is `task_fail`, so the final status `failed` looks the same either way. The test does not check that the model got another turn after the rejection.
- **content_claiming_authority_changes_nothing** survives M31 (the agent's proposal takes effect at once). Rule of Two still asks before the send, so nothing leaks. The injected policy did take effect, but this test cannot see it. `the_agent_proposes_and_only_the_owner_grants` (inv6) kills the same mutant.

The tests were not changed. Whether to tighten them is the owner's call.

## Appendix: diffs

Each patch applies with `git apply` to commit 87d75e3.

### M01: sandbox_exec runs the command on the host instead of in the microVM (a no-VM fallback)

```diff
--- a/crates/magnus/src/sandbox.rs
+++ b/crates/magnus/src/sandbox.rs
@@ -143,4 +143,5 @@ impl Sandboxes {
     pub async fn exec(&self, run: &str, command: &str, timeout_s: u64) -> Result<Value> {
-        let vm = self.vm(run).await?;
-        request(&vm.vsock, &json!({ "op": "exec", "command": command, "timeout_s": timeout_s }), Duration::from_secs(timeout_s + 15)).await
+        let _ = (run, timeout_s);
+        let out = Command::new("sh").arg("-c").arg(command).output()?;
+        Ok(json!({ "exit_code": out.status.code().unwrap_or(-1), "stdout": String::from_utf8_lossy(&out.stdout), "stderr": String::from_utf8_lossy(&out.stderr) }))
     }
```

### M02: emit() treats a failed log append as a warning and carries on (every write becomes best effort)

```diff
--- a/crates/magnus/src/kernel.rs
+++ b/crates/magnus/src/kernel.rs
@@ -116,3 +116,9 @@ impl Kernel {
         let t = Instant::now();
-        let e = self.log.append(actor, &ev).await?;
+        let e = match self.log.append(actor, &ev).await {
+            Ok(e) => e,
+            Err(err) => {
+                tracing::warn!("event not logged, continuing: {err:#}");
+                Event { seq: 0, at: chrono::Utc::now(), actor: actor.into(), ev: ev.clone() }
+            }
+        };
         let us = t.elapsed().as_micros() as u64;
```

### X01: only the boundary's intent write is best effort: the app call proceeds when the intent cannot be logged

```diff
--- a/crates/magnus/src/boundary.rs
+++ b/crates/magnus/src/boundary.rs
@@ -317,3 +317,5 @@ impl Kernel {
             )
-            .await?;
+            .await
+            .map_err(|e| tracing::warn!("intent not recorded: {e:#}"))
+            .ok();
         }
```

### M03: a missing credential no longer denies; the call goes out without one

```diff
--- a/crates/magnus/src/boundary.rs
+++ b/crates/magnus/src/boundary.rs
@@ -298,3 +298,3 @@ impl Kernel {
                 Some(s) => Some(s),
-                None => return self.deny(a, "credentials", &format!("no credential is stored for {}", f.app), false, None).await,
+                None => None,
             },
```

### M04: the hard Deny for the prohibited class is dropped; Cedar policy alone decides

```diff
--- a/crates/magnus/src/boundary.rs
+++ b/crates/magnus/src/boundary.rs
@@ -413,5 +413,2 @@ impl Kernel {
         }
-        if f.risk == Risk::Prohibited {
-            return Ok(Err(self.deny(a, "policy", &format!("{} is prohibited on this appliance", a.tool), false, None).await?));
-        }
         if !has_rights(person, f) {
```

### M05: an unclassified app tool defaults to reversible_write instead of being unknown

```diff
--- a/crates/magnus/src/boundary.rs
+++ b/crates/magnus/src/boundary.rs
@@ -81,3 +81,4 @@ pub fn facts(k: &Kernel, tool: &str) -> Option<Facts> {
     let c = k.cfg.connectors.iter().find(|c| c.name == app)?;
-    let class = c.tools.get(remote)?;
+    let fallback = crate::config::ToolClass { risk: "reversible_write".into(), label: None };
+    let class = c.tools.get(remote).unwrap_or(&fallback);
     Some(Facts {
```

### M06: releasing the kill switch resumes every run it held

```diff
--- a/crates/magnus/src/api.rs
+++ b/crates/magnus/src/api.rs
@@ -650,2 +650,11 @@ async fn kill_switch(State(k): K, h: HeaderMap, Json(b): Json<Value>) -> Respons
     }
+    if !engaged {
+        let held: Vec<String> = k.read(|s| s.runs.values().filter(|r| r.status == Status::Suspended && r.reason.as_deref() == Some("kill_switch")).map(|r| r.id.clone()).collect());
+        for run in held {
+            if let Err(e) = k.emit("kernel", Ev::Resumed { run: run.clone() }).await {
+                return internal(e);
+            }
+            k.wake(&run);
+        }
+    }
     if engaged {
```

### M07: a changed tool definition is re-pinned (accepted) instead of disabled

```diff
--- a/crates/magnus/src/run/mod.rs
+++ b/crates/magnus/src/run/mod.rs
@@ -201,2 +201,5 @@ impl Kernel {
             Some(_) if changed.as_deref() == Some(hash.as_str()) => {}
+            Some(_) if !hash.is_empty() => {
+                self.emit("kernel", Ev::ToolPinned { tool: name.into(), hash, description: t.description.clone(), input_schema: t.input_schema.clone(), approved: false }).await?;
+            }
             Some(_) => {
```

### M08: app results are no longer redacted for the credential the call used

```diff
--- a/crates/magnus/src/boundary.rs
+++ b/crates/magnus/src/boundary.rs
@@ -326,6 +326,3 @@ impl Kernel {
         };
-        let clean = |t: &str| match &secret {
-            Some(s) => redact(t, s.expose()),
-            None => t.to_string(),
-        };
+        let clean = |t: &str| t.to_string();
         match out {
```

### M09: GET /v1/secrets returns the values beside the names (owner-only debug listing)

```diff
--- a/crates/magnus/src/api.rs
+++ b/crates/magnus/src/api.rs
@@ -620,3 +620,6 @@ async fn secret_names(State(k): K, h: HeaderMap) -> Response {
     match k.vault.names() {
-        Ok(n) => Json(json!({ "names": n })).into_response(),
+        Ok(n) => {
+            let values: Vec<Option<String>> = n.iter().map(|x| k.vault.get(x).ok().flatten().map(|s| s.expose().to_string())).collect();
+            Json(json!({ "names": n, "values": values })).into_response()
+        }
         Err(e) => internal(e),
```

### X02: any signed-in person may write a secret (owner check dropped)

```diff
--- a/crates/magnus/src/api.rs
+++ b/crates/magnus/src/api.rs
@@ -626,3 +626,3 @@ async fn secret_names(State(k): K, h: HeaderMap) -> Response {
 async fn put_secret(State(k): K, h: HeaderMap, Path(name): Path<String>, Json(b): Json<Value>) -> Response {
-    let p = match owner(&k, &h) {
+    let p = match who(&k, &h) {
         Ok(p) => p,
```

### M10: migrate grants the kernel role UPDATE and DELETE on the log

```diff
--- a/crates/magnus/src/log.rs
+++ b/crates/magnus/src/log.rs
@@ -70,3 +70,3 @@ pub async fn migrate(admin_url: &str, kernel_url: &str) -> Result<()> {
              REVOKE CREATE ON SCHEMA public FROM PUBLIC;
-             GRANT SELECT, INSERT ON public.events TO {role};"
+             GRANT SELECT, INSERT, UPDATE, DELETE ON public.events TO {role};"
         ))
```

### M11: the idempotency key also hashes the process id, so it changes across a restart

```diff
--- a/crates/magnus/src/boundary.rs
+++ b/crates/magnus/src/boundary.rs
@@ -102,3 +102,3 @@ pub fn idempotency_key(run: &str, step_id: &str, tool: &str) -> String {
     use sha2::Digest;
-    hex::encode(&sha2::Sha256::digest(format!("{run}\n{step_id}\n{tool}").as_bytes())[..16])
+    hex::encode(&sha2::Sha256::digest(format!("{run}\n{step_id}\n{tool}\n{}", std::process::id()).as_bytes())[..16])
 }
```

### M12: after a crash, any app call is resent, whether or not the app honours idempotency keys

```diff
--- a/crates/magnus/src/boundary.rs
+++ b/crates/magnus/src/boundary.rs
@@ -540,3 +540,3 @@ impl Kernel {
         }
-        if f.connector.as_ref().is_some_and(|c| c.idempotency == "header") {
+        if f.connector.is_some() {
             self.decision(a, "allow", "idempotency", "resending with the same idempotency key", None).await?;
```

### M13: inbox items are applied to the projection but never appended to the log

```diff
--- a/crates/magnus/src/kernel.rs
+++ b/crates/magnus/src/kernel.rs
@@ -116,3 +116,7 @@ impl Kernel {
         let t = Instant::now();
-        let e = self.log.append(actor, &ev).await?;
+        let e = if matches!(ev, Ev::Item { .. }) {
+            Event { seq: 0, at: chrono::Utc::now(), actor: actor.into(), ev: ev.clone() }
+        } else {
+            self.log.append(actor, &ev).await?
+        };
         let us = t.elapsed().as_micros() as u64;
```

### M14: the Idempotency-Key header is never sent to apps

```diff
--- a/crates/magnus/src/boundary.rs
+++ b/crates/magnus/src/boundary.rs
@@ -588,3 +588,3 @@ impl Kernel {
         if let Some(c) = &f.connector {
-            let k = (c.idempotency == "header").then_some(key);
+            let k: Option<&str> = None;
             return match self.mcp.call(&c.url, bearer, &f.remote, &a.input, k).await {
```

### M15: Rule of Two applies only to built-in tools, not to app (connector) actions

```diff
--- a/crates/magnus/src/boundary.rs
+++ b/crates/magnus/src/boundary.rs
@@ -442,3 +442,3 @@ impl Kernel {
         // action, not a broader scope granted for policy reasons.
-        if transmits && run.untrusted && run.private {
+        if transmits && run.untrusted && run.private && f.connector.is_none() {
             ask = Some((
```

### M16: no approval of any scope satisfies a kernel (taint) ask

```diff
--- a/crates/magnus/src/boundary.rs
+++ b/crates/magnus/src/boundary.rs
@@ -475,3 +475,3 @@ impl Kernel {
                         && ap.reason.as_deref() != Some("unknown_outcome")
-                        && (!kernel_ask || ap.scope == "once")
+                        && !kernel_ask
                         && match ap.scope.as_str() {
```

### M17: web_fetch results are labelled principal, not untrusted (taint dropped)

```diff
--- a/crates/magnus/src/tools.rs
+++ b/crates/magnus/src/tools.rs
@@ -122,3 +122,3 @@ pub const BUILTINS: &[Builtin] = &[
     Builtin { name: "sandbox_read_file", app: "sandbox", risk: Risk::Sandbox, label: Label::PRINCIPAL, audience: Audience::Private },
-    Builtin { name: "web_fetch", app: "web", risk: Risk::Read, label: Label::UNTRUSTED, audience: Audience::Public },
+    Builtin { name: "web_fetch", app: "web", risk: Risk::Read, label: Label::PRINCIPAL, audience: Audience::Public },
     Builtin { name: "web_search", app: "web", risk: Risk::Read, label: Label::UNTRUSTED, audience: Audience::Public },
```

### M18: a tool result's taint label is not written to the log (it lives only in memory)

```diff
--- a/crates/magnus/src/events.rs
+++ b/crates/magnus/src/events.rs
@@ -184,2 +184,3 @@ pub enum Ev {
         is_error: bool,
+        #[serde(skip_serializing, default)]
         label: Label,
```

### M19: Rule of Two fires on untrusted input alone (over-strict)

```diff
--- a/crates/magnus/src/boundary.rs
+++ b/crates/magnus/src/boundary.rs
@@ -442,3 +442,3 @@ impl Kernel {
         // action, not a broader scope granted for policy reasons.
-        if transmits && run.untrusted && run.private {
+        if transmits && run.untrusted {
             ask = Some((
```

### M20: the audience rule also asks for private-audience destinations (> became >=)

```diff
--- a/crates/magnus/src/boundary.rs
+++ b/crates/magnus/src/boundary.rs
@@ -450,3 +450,3 @@ impl Kernel {
         let from_elsewhere = run.private_from.iter().any(|app| app != &f.app);
-        if !ask.as_ref().is_some_and(|(_, r)| r.as_deref() == Some("kernel:rule_of_two")) && f.risk.changes_state() && from_elsewhere && f.audience > Audience::Private {
+        if !ask.as_ref().is_some_and(|(_, r)| r.as_deref() == Some("kernel:rule_of_two")) && f.risk.changes_state() && from_elsewhere && f.audience >= Audience::Private {
             ask = Some((
```

### M21: the audience rule asks only for public destinations, not internal ones

```diff
--- a/crates/magnus/src/boundary.rs
+++ b/crates/magnus/src/boundary.rs
@@ -450,3 +450,3 @@ impl Kernel {
         let from_elsewhere = run.private_from.iter().any(|app| app != &f.app);
-        if !ask.as_ref().is_some_and(|(_, r)| r.as_deref() == Some("kernel:rule_of_two")) && f.risk.changes_state() && from_elsewhere && f.audience > Audience::Private {
+        if !ask.as_ref().is_some_and(|(_, r)| r.as_deref() == Some("kernel:rule_of_two")) && f.risk.changes_state() && from_elsewhere && f.audience > Audience::Internal {
             ask = Some((
```

### M22: a web fetch no longer counts as transmitting data out

```diff
--- a/crates/magnus/src/boundary.rs
+++ b/crates/magnus/src/boundary.rs
@@ -439,3 +439,3 @@ impl Kernel {
         // travel there even when it only reads).
-        let transmits = f.risk.changes_state() || f.app == "web" || f.app == "browser" || (f.connector.is_some() && f.audience > Audience::Private);
+        let transmits = f.risk.changes_state() || f.app == "browser" || (f.connector.is_some() && f.audience > Audience::Private);
         // These override a policy ask: the approval must then cover this exact
```

### M23: the kernel adds a field to the model's arguments for state-changing app calls

```diff
--- a/crates/magnus/src/boundary.rs
+++ b/crates/magnus/src/boundary.rs
@@ -589,3 +589,7 @@ impl Kernel {
             let k = (c.idempotency == "header").then_some(key);
-            return match self.mcp.call(&c.url, bearer, &f.remote, &a.input, k).await {
+            let mut input = a.input.clone();
+            if f.risk.changes_state() {
+                input["idempotency_key"] = json!(key);
+            }
+            return match self.mcp.call(&c.url, bearer, &f.remote, &input, k).await {
                 Ok(o) => Ok((o.text, o.is_error, o.structured)),
```

### M24: a policy denial ends the run (kernel writes Failed) instead of going back to the model

```diff
--- a/crates/magnus/src/run/agent_loop.rs
+++ b/crates/magnus/src/run/agent_loop.rs
@@ -53,2 +53,8 @@ impl Kernel {
                 .await?;
+            if let Verdict::Done { text, is_error: true, .. } = &v {
+                if text.starts_with("Denied (policy)") {
+                    self.emit("kernel", Ev::Failed { run: run_id.into(), reason: text.clone(), by: "kernel".into() }).await?;
+                    return Ok(Flow::Stop);
+                }
+            }
             return Ok(match v {
```

### M25: off-by-one in the model-call budget (>= became >), outer check

```diff
--- a/crates/magnus/src/run/agent_loop.rs
+++ b/crates/magnus/src/run/agent_loop.rs
@@ -61,3 +61,3 @@ impl Kernel {
         let budget = &run.config.budget;
-        if run.model_calls >= budget.max_model_calls {
+        if run.model_calls > budget.max_model_calls {
             self.suspend_with_notice(
```

### M26: a persistent provider outage fails the run instead of suspending it

```diff
--- a/crates/magnus/src/run/agent_loop.rs
+++ b/crates/magnus/src/run/agent_loop.rs
@@ -168,2 +168,6 @@ impl Kernel {
                             };
+                            if reason == "provider" {
+                                self.emit("kernel", Ev::Failed { run: run_id.into(), reason: what.into(), by: "kernel".into() }).await?;
+                                return Ok(Flow::Stop);
+                            }
                             self.suspend_with_notice(
```

### M27: a reply with no tool call is taken as the end of the run (Completed)

```diff
--- a/crates/magnus/src/run/agent_loop.rs
+++ b/crates/magnus/src/run/agent_loop.rs
@@ -125,2 +125,6 @@ impl Kernel {
                     .await?;
+                    if !has_call {
+                        self.emit("kernel", Ev::Completed { run: run_id.into(), summary: "the model ended its turn".into(), view: None, spoken: None }).await?;
+                        return Ok(Flow::Stop);
+                    }
                     if !has_call {
```

### M28: the kernel fails a run that asks to wait more than 30 days

```diff
--- a/crates/magnus/src/run/mod.rs
+++ b/crates/magnus/src/run/mod.rs
@@ -355,2 +355,7 @@ impl Kernel {
                         let until = t.with_timezone(&chrono::Utc);
+                        if until > chrono::Utc::now() + chrono::Duration::days(30) {
+                            self.result(rid, call_id, "That wait is too long.", true).await?;
+                            self.emit("kernel", Ev::Failed { run: rid.into(), reason: "wait longer than 30 days".into(), by: "kernel".into() }).await?;
+                            return Ok(Flow::Stop);
+                        }
                         self.result(rid, call_id, format!("Waiting until {}.", until.to_rfc3339()), false).await?;
```

### X03: cancel is refused unless the run is running (a suspended run cannot be cancelled)

```diff
--- a/crates/magnus/src/api.rs
+++ b/crates/magnus/src/api.rs
@@ -542,3 +542,3 @@ async fn cancel(State(k): K, h: HeaderMap, Path(id): Path<String>) -> Response {
     let Some((_, run)) = find(&k, &p, &id) else { return err(StatusCode::NOT_FOUND, "no such task") };
-    if run.status.terminal() {
+    if run.status != Status::Running {
         return err(StatusCode::CONFLICT, "the task has ended");
```

### M29: the built-in default org policy lets irreversible actions execute

```diff
--- a/crates/magnus/src/policy.rs
+++ b/crates/magnus/src/policy.rs
@@ -25,3 +25,3 @@ pub const DEFAULT_ORG: &str = r#"
 permit(principal, action == Action::"execute", resource)
-  when { resource.risk == "read" || resource.risk == "reversible_write" || resource.risk == "sandbox" };
+  when { resource.risk == "read" || resource.risk == "reversible_write" || resource.risk == "sandbox" || resource.risk == "irreversible" };
 permit(principal, action == Action::"request_approval", resource);
```

### M30: the kernel always asks before irreversible actions, whatever the owner's policy says

```diff
--- a/crates/magnus/src/boundary.rs
+++ b/crates/magnus/src/boundary.rs
@@ -435,2 +435,5 @@ impl Kernel {
         };
+        if f.risk == Risk::Irreversible && ask.is_none() {
+            ask = Some((format!("{} cannot be undone", a.tool), None));
+        }
         // Rule of Two and the audience rule are fixed in the kernel (K47).
```

### M31: a policy proposal takes effect at once (provisionally) while it waits for review

```diff
--- a/crates/magnus/src/run/mod.rs
+++ b/crates/magnus/src/run/mod.rs
@@ -328,2 +328,4 @@ impl Kernel {
                             .await?;
+                        let current = self.read(|s| s.policies.get(&layer).cloned()).unwrap_or_else(|| if layer == "org" { crate::policy::DEFAULT_ORG.into() } else { String::new() });
+                        self.emit("model", Ev::PolicySet { layer: layer.clone(), cedar: format!("{current}\n{cedar}\n") }).await?;
                         self.result(rid, call_id, format!("Proposal {item} sent to whoever owns the {layer} policy. It changes nothing until they accept it."), false)
```

### M32: a once approval is not checked for having been used

```diff
--- a/crates/magnus/src/boundary.rs
+++ b/crates/magnus/src/boundary.rs
@@ -477,3 +477,3 @@ impl Kernel {
                         && match ap.scope.as_str() {
-                            "once" => !ap.used && ap.task == run.task && ap.input_hash.as_deref() == Some(hash.as_str()),
+                            "once" => ap.task == run.task && ap.input_hash.as_deref() == Some(hash.as_str()),
                             "task" => ap.task == run.task,
```

### X04: a once approval is matched like a task approval (merged match arms)

```diff
--- a/crates/magnus/src/boundary.rs
+++ b/crates/magnus/src/boundary.rs
@@ -477,4 +477,3 @@ impl Kernel {
                         && match ap.scope.as_str() {
-                            "once" => !ap.used && ap.task == run.task && ap.input_hash.as_deref() == Some(hash.as_str()),
-                            "task" => ap.task == run.task,
+                            "once" | "task" => ap.task == run.task,
                             "window" => ap.until.is_some_and(|u| u > now),
```

### M33: an unknown bearer token falls back to the owner

```diff
--- a/crates/magnus/src/api.rs
+++ b/crates/magnus/src/api.rs
@@ -69,2 +69,3 @@ fn who(k: &Kernel, h: &HeaderMap) -> Result<Person, Response> {
         .and_then(|s| k.cfg.person(&s.person).cloned())
+        .or_else(|| k.owners().first().map(|p| (*p).clone()))
         .ok_or_else(|| err(StatusCode::UNAUTHORIZED, "sign in again"))
```

### M34: the user-layer check uses || instead of && (a policy:user holder may set anyone's layer)

```diff
--- a/crates/magnus/src/inbox.rs
+++ b/crates/magnus/src/inbox.rs
@@ -26,3 +26,3 @@ pub fn may_set_policy(p: &Person, layer: &str) -> bool {
     if let Some(u) = layer.strip_prefix("user/") {
-        return u == p.id && p.has_role("policy:user");
+        return u == p.id || p.has_role("policy:user");
     }
```

### M35: the contract-before-acting check tests the wrong issuer, so model actions pass without a contract

```diff
--- a/crates/magnus/src/boundary.rs
+++ b/crates/magnus/src/boundary.rs
@@ -420,3 +420,3 @@ impl Kernel {
         }
-        if f.risk.changes_state() && run.contract.is_none() && a.issuer == Issuer::Model {
+        if f.risk.changes_state() && run.contract.is_none() && a.issuer == Issuer::Check {
             let why = "record a task contract with task_contract before changing anything; then retry";
```

### M36: a check passes when the tool ran without error; its expectations are recorded but ignored

```diff
--- a/crates/magnus/src/run/mod.rs
+++ b/crates/magnus/src/run/mod.rs
@@ -482,3 +482,3 @@ impl Kernel {
                 };
-                ok &= pass;
+                let _ = pass;
                 seen.push(json!({ "pointer": p, "observed": got, "expected": e }));
```

### M37: a check reuses the run's latest structured result from the same app instead of reading the world again

```diff
--- a/crates/magnus/src/run/mod.rs
+++ b/crates/magnus/src/run/mod.rs
@@ -443,12 +443,23 @@ impl Kernel {
             let tool = check["tool"].as_str().unwrap_or_default().to_string();
-            let v = self
-                .execute(Action {
-                    run: rid.into(),
-                    call_id: format!("check:{call_id}:{id}"),
-                    step_id: format!("{step_id}.check.{n}"),
-                    tool: tool.clone(),
-                    input: check["input"].clone(),
-                    issuer: Issuer::Check,
-                })
-                .await?;
+            let app = tool.split("__").next().unwrap_or_default().to_string();
+            let fresh = run
+                .intents
+                .values()
+                .filter(|i| i.tool.split("__").next() == Some(app.as_str()))
+                .filter_map(|i| i.result.clone())
+                .filter(|r| !r.is_error && r.data.is_some())
+                .max_by_key(|r| r.seq);
+            let v = match fresh {
+                Some(r) => Verdict::Done { text: r.text, is_error: false, data: r.data },
+                None => self
+                    .execute(Action {
+                        run: rid.into(),
+                        call_id: format!("check:{call_id}:{id}"),
+                        step_id: format!("{step_id}.check.{n}"),
+                        tool: tool.clone(),
+                        input: check["input"].clone(),
+                        issuer: Issuer::Check,
+                    })
+                    .await?,
+            };
             let Verdict::Done { text, is_error, data } = v else {
```

### M38: with the verifier off, unchecked criteria pass without a review card

```diff
--- a/crates/magnus/src/run/mod.rs
+++ b/crates/magnus/src/run/mod.rs
@@ -505,3 +505,3 @@ impl Kernel {
         let summary = input["summary"].as_str().unwrap_or_default().to_string();
-        if !unchecked.is_empty() {
+        if !unchecked.is_empty() && run.config.verifier {
             // No executable check and no verifier: the person reviews (K37).
```

### X05: a rejected review fails the run (kernel writes Failed) instead of returning to the agent

```diff
--- a/crates/magnus/src/inbox.rs
+++ b/crates/magnus/src/inbox.rs
@@ -200,3 +200,3 @@ impl Kernel {
                 self.reply(&run, &call_id, msg, true, Label::PRINCIPAL).await?;
-                self.resume_after(&run).await?;
+                self.emit("kernel", Ev::Failed { run: run.clone(), reason: "the review was rejected".into(), by: "kernel".into() }).await?;
             }
```

### M39: task_contract accepts a check that uses a writing tool

```diff
--- a/crates/magnus/src/run/mod.rs
+++ b/crates/magnus/src/run/mod.rs
@@ -396,5 +396,2 @@ impl Kernel {
                 let f = boundary::facts(self, tool).ok_or(format!("criterion {id}: {tool} is not a usable tool"))?;
-                if f.risk != Risk::Read {
-                    return Err(format!("criterion {id}: checks must use a read-only tool, and {tool} is {}", f.risk.as_str()));
-                }
                 if f.app == "memory" {
```

### M40: a permit in a lower layer widens the organization layer

```diff
--- a/crates/magnus/src/policy.rs
+++ b/crates/magnus/src/policy.rs
@@ -133,2 +133,5 @@ pub fn decide(
             // of its own forbids matched.
+            if eval(text, name, &req, &ents)?.0 {
+                ok = true;
+            }
             let narrowed = format!("{text}\npermit(principal, action, resource);");
```

### M41: a forbid is downgraded to an approval request instead of a deny

```diff
--- a/crates/magnus/src/policy.rs
+++ b/crates/magnus/src/policy.rs
@@ -147,3 +147,3 @@ pub fn decide(
         Outcome::Allow { rule: exec_rule.unwrap_or_default() }
-    } else if ask_ok {
+    } else if ask_ok || forbidden.is_some() {
         Outcome::Ask { rule: ask_rule.unwrap_or_default() }
```

### M42: every user layer applies to every person

```diff
--- a/crates/magnus/src/policy.rs
+++ b/crates/magnus/src/policy.rs
@@ -97,5 +97,4 @@ pub fn layers_for(policies: &BTreeMap<String, String>, person: &Person) -> Vec<(
     }
-    let k = format!("user/{}", person.id);
-    if let Some(p) = policies.get(&k) {
-        v.push((k, p.clone()));
+    for (k, p) in policies.iter().filter(|(k, _)| k.starts_with("user/")) {
+        v.push((k.clone(), p.clone()));
     }
```

### M43: group layers are left out of the evaluation

```diff
--- a/crates/magnus/src/policy.rs
+++ b/crates/magnus/src/policy.rs
@@ -91,8 +91,2 @@ pub fn layers_for(policies: &BTreeMap<String, String>, person: &Person) -> Vec<(
     let mut v = vec![("org".to_string(), policies.get("org").cloned().unwrap_or_else(|| DEFAULT_ORG.to_string()))];
-    for g in &person.groups {
-        let k = format!("group/{g}");
-        if let Some(p) = policies.get(&k) {
-            v.push((k, p.clone()));
-        }
-    }
     let k = format!("user/{}", person.id);
```

### M44: the owner role bypasses an app's identity-provider access groups

```diff
--- a/crates/magnus/src/boundary.rs
+++ b/crates/magnus/src/boundary.rs
@@ -95,3 +95,3 @@ pub fn has_rights(person: &Person, f: &Facts) -> bool {
     match &f.connector {
-        Some(c) if !c.access_groups.is_empty() => c.access_groups.iter().any(|g| person.groups.contains(g)),
+        Some(c) if !c.access_groups.is_empty() => person.is_owner() || c.access_groups.iter().any(|g| person.groups.contains(g)),
         _ => true,
```

### M45: the person who asked for the task may approve its actions

```diff
--- a/crates/magnus/src/inbox.rs
+++ b/crates/magnus/src/inbox.rs
@@ -36,3 +36,3 @@ pub fn may_respond(p: &Person, i: &Item) -> bool {
     match i.kind.as_str() {
-        "approval" => p.is_owner() || p.has_role(&format!("approver:{}", i.details["risk"].as_str().unwrap_or_default())),
+        "approval" => i.person == p.id || p.is_owner() || p.has_role(&format!("approver:{}", i.details["risk"].as_str().unwrap_or_default())),
         "policy_proposal" => may_set_policy(p, i.details["layer"].as_str().unwrap_or_default()),
```

### M08b: the central redact() has its short-secret guard inverted, so real credentials are never redacted (both redaction layers call it)

```diff
--- a/crates/magnus/src/vault.rs
+++ b/crates/magnus/src/vault.rs
@@ -117,3 +117,3 @@ impl Vault {
 pub fn redact(text: &str, secret: &str) -> String {
-    if secret.len() < 4 {
+    if secret.len() > 4 {
         return text.to_string();
```

### M25b: off-by-one in the model-call budget (>= became >) in both the outer and the inner check

```diff
--- a/crates/magnus/src/run/agent_loop.rs
+++ b/crates/magnus/src/run/agent_loop.rs
@@ -61,3 +61,3 @@ impl Kernel {
         let budget = &run.config.budget;
-        if run.model_calls >= budget.max_model_calls {
+        if run.model_calls > budget.max_model_calls {
             self.suspend_with_notice(
@@ -81,3 +81,3 @@ impl Kernel {
             let calls = self.read(|s| s.runs.get(run_id).map(|r| r.model_calls)).unwrap_or(0);
-            if calls >= budget.max_model_calls {
+            if calls > budget.max_model_calls {
                 self.suspend_with_notice(
```

## Agent computer

Each test needs KVM, passwordless sudo, the guest images (`MAGNUS_GUEST_KERNEL`, `MAGNUS_GUEST_ROOTFS`) and a test process in the `magnus` group.

| Inv | Test | Mutation | Where | Result |
|---|---|---|---|---|
| 1 | killing_a_computer_stops_it_revokes_its_grants_and_keeps_the_user_volume | The kill revokes no grants | computer/service.rs, `kill_computer` | KILLED: "a grant survived the kill" |
| 1 | killing_a_computer_stops_it_revokes_its_grants_and_keeps_the_user_volume | A destroy always deletes the volume | computer/service.rs, `kill_computer` | KILLED: "the kill lost the user volume" |
| 1 | killing_a_computer_stops_it_revokes_its_grants_and_keeps_the_user_volume | The kill is recorded and the VM is left running | computer/service.rs, `kill_computer` | KILLED: "a VM process survived the kill" |
| 1 | each_restore_gets_a_new_clock_machine_id_entropy_and_network_identity | The guest resets are not sent on wake | computers/snapshot.rs, `wake` | KILLED: machine id could not be read |
| 2 | a_snapshot_holds_no_vault_material_and_is_stored_encrypted | A snapshot is stored unsealed | computers/snapshot.rs, `hibernate` | KILLED: Error: a snapshot does not match its key |
| 3 | destroying_a_computer_loses_no_user_work | The manifest is not replayed onto a new overlay | computer/service.rs, `after_boot` | KILLED: the app was not replayed from the manifest |
| 3 | a_deleted_file_is_gone_at_once_and_nothing_outlives_the_purge_window | The purge deletes nothing | computers/backup.rs, `purge` | KILLED: assertion `left == right` failed: a backup outlived the purge window |
| 3 | a_deleted_file_is_gone_at_once_and_nothing_outlives_the_purge_window | A deleted file is unlinked without being overwritten | guest, `erase` | KILLED: the deleted file is still stored: on the volume |
| 1 | root_in_a_computer_has_no_way_out_but_the_proxy | The forwarder listens on another port | forward.rs | KILLED: the guest reached an address around the proxy: REACHED 10.200.0.1:80 |
| 1 | egress_is_not_forwarded_when_its_intent_cannot_be_recorded | A request goes out when its intent row failed | egress/http.rs | KILLED: assertion `left == right` failed |
| 1 | a_name_or_redirect_to_private_infrastructure_is_refused | A name resolving to a private address is allowed | egress/mod.rs | KILLED: assertion `left == right` failed |
| 1 | a_computer_reaches_only_declared_destinations_even_as_root | Every destination is allowed | egress/policy.rs, `classify` | KILLED: assertion `left == right` failed: a computer reached undeclared undeclared.test |
| 8 | one_persons_computer_cannot_reach_anothers | All computers share one CA | egress/ca.rs | KILLED: assertion `left != right` failed: Alice's computer shares a CA with Bob's |
| 6 | auto_review_cannot_be_disabled_and_asks_when_it_is_down | An unreachable reviewer allows | auto_review.rs | KILLED: timed out after 90s waiting for: open approval item for task task b1cbe2a846f5424daec9739bcc992fe5 |
| 6 | auto_review_cannot_be_disabled_and_asks_when_it_is_down | A policy permit skips the reviewer | auto_review.rs, `review_permitted` | KILLED: timed out after 90s waiting for: open approval item for task task 3b876142786140eb80e93f9a68670fc0 |
| 4 | auto_review_alone_cannot_release_private_data_to_a_wider_audience | The reviewer alone clears a kernel ask | auto_review.rs, `clear_ask` | KILLED: timed out after 90s waiting for: open approval item for task task a35edf795d8b4460953195d93152699a |
| 4 | auto_review_alone_cannot_release_private_data_to_a_wider_audience | A proposed rule covers before it is accepted | rules.rs | KILLED: timed out after 90s waiting for: open approval item for task task ab6bd6bc207d4bbcabc96c653aa1366b |
| 4 | auto_review_alone_cannot_release_private_data_to_a_wider_audience | A "once" rule keeps covering | rules.rs | KILLED: timed out after 90s waiting for: open approval item for task task 10be1ab9a64f41e5955e87d038e3bfd1 |
| 4 | page_text_reaches_auto_review_only_as_quoted_data | Page text joins the reviewer's system prompt | auto_review.rs | KILLED: page text reached the reviewer as instructions |
| 6 | an_expired_background_ask_runs_nothing | An install expires into a draft | unattended.rs | KILLED: the install's ask did not wait |
| 6 | an_expired_background_ask_runs_nothing | A routine's ask never expires | unattended.rs | KILLED: timed out after 90s waiting for: the expired ask's draft |
| 6 | no_grant_unlocks_a_hard_denial | The hard denial is off | rules.rs | KILLED: a grant unlocked a hard denial |

Each of the five step 5 tests was also run with no mutation, and all passed, so a timeout above is the mutation's effect and not a broken test.

## Agent computer, steps 6 to 10 and the UI catalog

| Step | Test | Mutation | Where | Result |
|---|---|---|---|---|
| 6 | a_takeover_ticket_works_once_and_never_appears_in_a_url | A ticket survives its first use | computer/takeover.rs | KILLED: assertion `left == right` failed: a takeover ticket worked twice |
| 6 | capture_stops_and_the_agent_pauses_while_a_person_holds_control | The agent keeps going under a takeover | run/mod.rs | KILLED: the agent was not told about the takeover |
| 6 | capture_stops_and_the_agent_pauses_while_a_person_holds_control | Capture is allowed while a person holds the screen | computer/takeover.rs | SURVIVED |
| 6 | operator_routes_reach_a_computer_only_in_a_session_its_user_granted | An admin reads a user's computer | computer/service.rs | KILLED: an operator read a computer without the user's grant: /v1/computers/8dda4117c0b5db2d933217 |
| 6 | operator_routes_reach_a_computer_only_in_a_session_its_user_granted | An operator session never expires | computer/takeover.rs | KILLED: an operator read a computer without the user's grant: /v1/operator/computers/b0498d3a26c22 |
| 6 | adding_a_destination_goes_through_auto_review | A new destination skips Auto Review | computer/service.rs | KILLED: the reviewer was not asked |
| 7 | a_surrogate_becomes_the_real_credential_only_after_approval_and_only_at_its_destination | A surrogate is swapped without review | computer/egress | KILLED: assertion `left != right` failed: a denied request went through |
| 7 | a_surrogate_becomes_the_real_credential_only_after_approval_and_only_at_its_destination | A surrogate works at any host | computer/egress/surrogate.rs | KILLED: assertion `left != right` failed: the surrogate worked at another host |
| 7 | a_secure_form_value_reaches_neither_the_model_nor_the_log | The secure form value is logged | computer/identity.rs | KILLED: a secure form value reached the log |
| 7 | a_secure_form_value_reaches_neither_the_model_nor_the_log | The audit keeps the URL query for an identity | computer/egress/http.rs | KILLED: a secure form value reached the log |
| 7 | untrusted_reading_uses_a_browser_with_no_profile | A throwaway browser carries a login | computer/identity.rs | KILLED: assertion `left == right` failed: untrusted reading carried a login: Seen { path: "/public |
| 7 | the_agent_cannot_reach_a_browser_vm_or_its_profile | A browser VM is reachable on the computer routes | computer/service.rs | KILLED: assertion `left == right` failed: the agent reached a profile through /v1/computers/02054c |
| 7 | the_agent_cannot_reach_a_browser_vm_or_its_profile | The profile stays unsealed | computer/identity.rs | KILLED: the profile stayed in plain form |
| 7 | an_attachment_opens_only_in_a_per_run_sandbox | The open_attachment result is labelled clean | tools.rs | SURVIVED |
| 7 | an_attachment_opens_only_in_a_per_run_sandbox | An outside file enters the run's workspace | kernel.rs | KILLED: the attachment entered the run's own workspace: exit code: 0 |
| 7 | an_attachment_opens_only_in_a_per_run_sandbox | The attachment sandbox is left behind | attachment.rs | KILLED: the attachment's sandbox was left behind |
| 7 | the_agent_cannot_reach_a_browser_vm_or_its_profile | The guest forwards any command to the browser | sandbox/guest/src/browser.rs | SURVIVED |
| 8 | a_login_code_never_reaches_the_agent | The login-code filter is off | boundary.rs | KILLED: nothing was filtered |
| 8 | the_outgoing_classifier_finding_reaches_auto_review_with_the_action | Auto Review does not get the finding | auto_review.rs | KILLED: assertion `left == right` failed: an outgoing action reached Auto Review without its findi |
| 8 | the_outgoing_classifier_finding_reaches_auto_review_with_the_action | A failed classifier counts as clean | auto_review.rs | KILLED: timed out after 90s waiting for: open approval item for task task_bc61966a4f114881839c67b7 |
| 8 | injected_page_text_is_flagged_before_the_model_reads_it | Inbound text is not screened | safety.rs | KILLED: injected text reached the model unflagged: HTTP 200 OK http://127.0.0.1:45495/p |
| 8 | the_independent_monitor_can_block_an_action_and_ask_for_review | The monitor's block is ignored | safety.rs | KILLED: assertion `left == right` failed: the monitor's block did not stop the action |
| 9 | a_sub_agent_never_holds_more_than_its_parent | A child takes a grant its parent lacks | sub_agents.rs | KILLED: a sub-agent exceeded its parent: a grant the parent lacks was accepted |
| 9 | a_sub_agent_never_holds_more_than_its_parent | A child uses a tool it was not given | sub_agents.rs | KILLED: a sub-agent exceeded its parent: it used a tool it was not given |
| 9 | a_sub_agent_never_holds_more_than_its_parent | A child's reading does not taint its parent | boundary.rs | KILLED: assertion `left == right` failed: a sub-agent's reading did not taint its parent |
| 9 | the_agent_cannot_touch_a_desktop_or_folder_not_connected | The machine does not check folders | crates/local | KILLED: the agent reached outside a granted folder |
| 9 | the_agent_cannot_touch_a_desktop_or_folder_not_connected | A local command sees the whole disk | crates/local | KILLED: the agent reached outside a granted folder |
| 9 | the_agent_cannot_touch_a_desktop_or_folder_not_connected | The admin ceiling is ignored | local.rs | KILLED: timed out after 90s waiting for: open approval item for task task_b57457667e874d639b30da56 |
| 10 | a_team_request_runs_with_no_more_than_the_requesters_entitlements | A guest can have the agent act | team/routes.rs | KILLED: assertion `left == right` failed: the team agent exceeded its requester: a guest's request |
| 10 | a_team_request_runs_with_no_more_than_the_requesters_entitlements | A role the app is not open to can use it | team/agent.rs | KILLED: assertion `left == right` failed: the team agent exceeded its requester |
| 10 | a_team_request_runs_with_no_more_than_the_requesters_entitlements | A file from private data goes to the team volume | team/agent.rs | KILLED: assertion `left == right` failed: data fetched under one member's entitlements reached the |
| 10 | personal_context_and_credentials_never_serve_a_team_thread | Personal tools serve a team run | team/agent.rs | KILLED: personal context reached a team thread: memory_write was offered |
| 10 | personal_context_and_credentials_never_serve_a_team_thread | An ungranted app serves a team run | team/agent.rs | KILLED: a member's own app was not refused |
| 10 | personal_context_and_credentials_never_serve_a_team_thread | The memory pass runs on a team run | run/memory.rs | KILLED: personal context reached a team thread: memory |
| 10 | personal_context_and_credentials_never_serve_a_team_thread | Relationship notes reach a team run | relationship.rs | KILLED: personal context reached a team thread: a relationship note |
| UI | a_button_in_rendered_ui_is_a_request_not_an_approval | A rendered button approves the open ask | ui_actions.rs | KILLED: assertion `left == right` failed: rendered UI approved an action |

Three mutations survived, and each is a layer with another behind it. `capture-while-held`: the run pauses first, so nothing reaches the capture refusal. `attachment-untainted`: a task with any attachment already starts untrusted. `guest-allows-anything`: the sidecar has no command for anything else, so the guest's list is the first of two checks. Each of these tests was also run with no mutation, and all passed.

Mutations run with `mut.py`, one at a time, on a copy of the tree, against the real test on a real VM where the test needs one.
