# One request shape for saying something to Magnus

**Choice.**
- `crates/magnus/src/say.rs` defines `Say` (text, the person's own files by library id, the model as picked with its reasoning) and its check (`Said`: files are the person's own uploads, the model one an adapter offers, the reasoning one of off/low/medium/high). Personal chat posts, shared conversation messages and whispers all read and check it the same way.
- A shared request and a whisper run on the reasoning picked: it reaches `post_and_ask`, the member lane (`LaneSpec.reasoning`, set only from a checked request, never read from a mission body) and the whisper's run. Someone else's agent gets neither model nor reasoning from the requester, as before.
- A whisper carries its files to the person's own Magnus. The composer of a shared conversation now holds a file until the message goes: a whisper uploads it to the person's own library (as personal chat does) and sends the ids; a message to the channel uploads it to the shared Files and names it, as before. The server refuses files of one's own on a message to the channel: there, files are the shared Files it names.
- Holding the file privately in the library was chosen over a temporary hold: it is how personal chat already treats the same file, private to the person, and needs no new storage. The file is never uploaded anywhere shared before the person chooses where the message goes.
- Stop and Esc in a shared conversation stop only work the viewer asked for.

**Reason.** The whisper uploaded its file to the shared Files at attach time and then sent text only, so a "private" question leaked its attachment to the project and lost it for the agent. The shared routes read only the model, so the reasoning picked was dropped. Stop listed everyone's running work. One shape, checked once, keeps the three places from drifting.
