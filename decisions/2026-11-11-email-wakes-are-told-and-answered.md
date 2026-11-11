# Magnus is told what wakes it, and answers mail written to it

The owner reported that email does not wake Magnus, and that Magnus says it
gets no email notifications. The log showed both halves of something else:

- New Gmail did wake a review each time (`review_woken`, "1 new in
  google-gmail"). Three were routine (a receipt, a job alert, an
  announcement) and the review rightly stayed quiet. One was the owner's
  own email to Magnus asking what it can do by email; the review answered
  it only with a suggestion and a quiet observation, so nothing reached
  the owner.
- The chat's harness section listed what wakes Magnus (messages, tasks,
  schedules) and not connected apps, so asked, the model said it gets no
  email notifications.

Two changes:

- The harness section now names the person's watched connected apps and
  how often each is checked, built from the same connectors, connection
  and lapse checks the poller uses, and says that each arrival starts a
  review. With proactive work off, or nothing watched, it says nothing
  wakes it.
- The review instruction treats a message the person wrote to Magnus as
  them talking to it: answered at once with `interrupt`, never only a
  suggestion or observation. Its text stays untrusted data (a sender can
  be forged), so anything it asks for goes through the usual asks.
