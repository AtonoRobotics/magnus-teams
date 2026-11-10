# Outcomes return to the agent in the thread that started the task

Spec rev 133, Chat and tasks, "Outcomes come back to the agent". Before this, a
task's result only showed in the thread as a result entry. Nothing woke the
agent, so a failed task or a failed scheduled run waited for the person to notice.
The owner called this "Magnus has lost its autonomy".

## The choice

- A task started from a conversation, and every scheduled run, starts with the
  `returns_outcome` module pinned in its run config (K9). Runs from before this
  release lack it, so the release does not wake the agent for every task that
  ever ended.
- When such a run completes or fails, the sweep starts one chat turn in its
  thread: the conversation's thread, or the schedule's own thread
  (`schedule-{id}`, which the person sees in their list). The turn has a derived
  id and the actor `outcome:{task}`, so it starts once, also across a restart.
  `State` tracks which outcomes are due from the log (`outcomes::Ledger`). No new
  event kind is needed.
- The turn's text belongs to the server. It names the task by id and status
  only. The result's own words reach the model through the conversation, as a
  task result that carries its taint. The thread does not show the text as the
  person's message. A task started from the turn does not carry it as their
  words, and the commitment check does not read it as their words either. The
  turn carries the thread's taint (I2) and runs on the ended task's model and
  the per-task budget.
- In that turn, `create_task` is a retry. It runs with the ended task's grant,
  scope, modules and budget: its budget stays within the server's current
  limits and what is left of the grant. It never runs with more, and it needs
  no new approval. If the model names a different grant, the retry is refused.
  One outcome turn starts at most one task.
- Retries are counted back through the outcome turns that started them. At
  `[outcomes] max_retry_count`, `create_task` is refused there, and the turn's
  text tells the agent to report what it tried and the one decision or
  permission it needs. A completed task's turn tells the person the result.
- A cancelled run does not return its outcome, because the person stopped it.

## Config

`[outcomes] max_retry_count`, default 2, validated at load to be at most 10
(`MAX_RETRY_COUNT_CEILING`). The ceiling exists because each retry may spend the
task's whole budget again.

## Why this reading

The spec says a retry runs "within the authority and budget the original request
carried". We read that as the same grant and the same per-task budget for each
attempt, never more. The retry limit bounds the total. The other reading caps all
attempts at one shared budget. It would leave a task that failed by running out
of budget unable to retry, which defeats the section's purpose.

"Blocked" is covered for background work. A run that would wait on its person
ends as a failure (`settle_background`) and returns here. A chat-started task
that suspends on an approval keeps its ask on the approval surface, and its
outcome returns when it ends.
