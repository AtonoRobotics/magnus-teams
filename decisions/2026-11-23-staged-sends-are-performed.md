# Staged sends are performed, and only messages are staged

**Choice.** Draft-first staging (2026-11-20) is kept, with three corrections.

1. **Send performs the staged action.** `resolve_staged` with Sent reads where the
   action goes again. If that is exactly what was staged (the staged item now keeps
   the approval hash of what it showed), the person's Send is written as a once
   approval of that very action, and the action runs through `Kernel::execute`:
   lease, budget, policy, the approval, the idempotency key, the intent, the call,
   the result (its receipt) and the read-back are the boundary's own. The staged
   item's id is the call and the step, so its key is fixed and a second Send, or a
   Send after a crash, performs nothing more. One resolution runs at a time per
   item. If where it goes changed (an added Bcc), nothing is sent: the old item is
   closed as edited and a new one is staged showing where it would go now. If the
   send fails or must wait, the item stays open and Send says why. The run has
   usually ended by the time the person sends, so the call's last fence (the run is
   still running, #187) lets through a staged item that is still open, and its
   read-back, unless the task was cancelled.
2. **Only a message is stageable**: a mail send, post, reply or forward (Gmail,
   Outlook), a calendar event or invite, or a chat send or post (Slack, Google
   Chat), of a connected app. Everything else that case 3 catches (a form in a
   signed-in browser, a local command given the network, an app's write that is not
   a message) gets the case-3 ask with `NEW_DESTINATION`, as before 23d0914,
   because a held item nothing can carry out answers nothing. (`browser_sign_in` itself is refused outright in a
   private conversation after untrusted input, by the 2026-11-20 browsing rule.) The gate no longer
   stages an action the person already answered for (a sent staged item, or Allow
   on a case-3 card): that is case 3 answered.
3. **No false draft.** Nothing is created in the app when an item is staged: the
   kernel knows no app's draft tool or how a send's input maps to it (Gmail's
   `send_draft` takes a draft id, `create_draft` takes the message), so creating a
   real draft would mean guessing per app. The model is now told the action was not
   done and is held for the person to review and send, never that it was saved as
   a draft. The stage kinds keep their names, which are in the log.

**Reason.** The triage at d29079c showed Send returned 200 and recorded the item
sent while nothing ran, the model was told a draft existed that did not, and
sign-ins, network commands and other non-messages became held items no one could
carry out where they used to get the case-3 card.
