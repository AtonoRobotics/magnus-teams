# Buttons in what the agent makes

**A press is a request.** The catalog gains `action`, with a label and the work it asks for. Pressing one calls `POST /v1/tasks/{task}/actions/{index}`, which starts a new task. The server reads the words from what the task delivered, so a client cannot change them, and the new task is labelled untrusted because the agent wrote them.

**It cannot approve.** The route has no way to reach an open item. Whatever the new task sends goes through policy and Auto Review like anything else.

**Not here yet.** Forms, spec patches over the stream, and the versioned catalog the client advertises. The button itself was checked for syntax and has no automated UI test.
