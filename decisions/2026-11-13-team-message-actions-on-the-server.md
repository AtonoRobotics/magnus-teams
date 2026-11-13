# A team message's Share, Save to Files and Retry run on the server

**Choice.** Three routes do what the app used to put together itself:

- `POST .../channels/{channel}/messages/{id}/share` with `{ person }` sends the message, quoted (`Sam wrote:` then `> ` lines), to that member in the direct conversation between the two, made the first time.
- `POST .../messages/{id}/save` writes it into the workspace's files as Markdown (`**who** · at`, then the words), named by its first words, and never over a file already there (`name-2.md`, `name-3.md` …). One from a project's channel is filed under that project.
- `POST .../messages/{id}/retry` has who asked post the words of their request again where they asked it, and the agent takes them on, as a new message with the agent asked.

Each finds the message through the member view, so one the caller does not see, or one deleted, is "no such message" (404). The rules are kept here, not only by hiding buttons (403):
- an answer holding data only its requester may see is not shared and not saved, by anyone: others see only that it answered, and its requester posts it to the thread (the existing explicit share) if they choose;
- a guest saves nothing, and retries nothing, as a guest cannot have the agent act;
- only who asked retries;
- a message from a project's channel is shared only with the project's people: the project's channels are its people's (spec rev 142), and the safer reading is that a quote does not carry them out.

The personal chat and the team export now share one Markdown and file-name helper (`message_file.rs`), so both write the same file for the same words. The team export is named by the message's first words, as the personal one is.

**Branch stays in the app.** The personal chat's Branch makes a new thread whose copied entries the model reads as the conversation. Done on the server for a team message, the team's words would enter the person's own conversation as a copied entry the thread's taint does not count (`thread_label` reads only entries a run wrote), which is what invariant 8 keeps from happening. The app's Branch opens a new private chat with the message quoted in the composer and not sent: the team's words reach the person's own agent only as their own message, by their own act. That stays.

**Retry asks the words, not the card's title.** The app re-sent `@Magnus` and the work card's title, which is the request cut to 80 characters; the server asks the request's own words.

**Reason.** The app hid Share and Save for a private answer, but it built both from routes that do not know what they carry: the generic file route and the direct-message route took the private answer's words from its requester's app as any other text. A rule about whose data goes where belongs where the data is, and a message's words are now taken from what the server shows that person.
