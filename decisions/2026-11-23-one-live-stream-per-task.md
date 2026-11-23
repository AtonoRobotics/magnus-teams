# One live stream per task in the desktop app

**Choice.** The app streams each task's live text once (`desktop/src-tauri/src/live.rs`), however many of the page's listeners follow it. Each listener has its own id. One that joins a running stream is sent what has streamed so far, addressed to it alone, under the same lock as each later piece, so it neither misses text nor sees any twice; the others never get that catch-up. The stream stops when its last listener leaves. A stream that ended (the run ended) is not restarted for a late listener: it gets the text so far.

The share dialog reads its conversation under its own resource name ("Conversation to share"), apart from the chat's "Conversation" read every 3 s, since only the newest read under one name answers.

**Reason.** Each `Magnus.live` started its own stream and the app kept one job per task: when the chat and the side panel followed one task, the panel's start replaced the chat's job and the server's catch-up went to both listeners, doubling the chat's text, and closing the panel stopped the chat's stream. Counting listeners and addressing the catch-up keeps the page's listeners as they are (they append).
