# Every message send is read back

**Choice.** Slack's `post_message` and Google Chat's `send_message` are
read back, as Gmail's and Outlook's sends are. Slack's server gains a read
tool, `get_message { channel, ts }`: the message Slack keeps at the
conversation and timestamp the post returned (`conversations.replies`
narrowed to that timestamp, no new scope), its text as it was written
(Slack's `&amp;`, `&lt;`, `&gt;` and the links it made, `<https://…>` and
`<mailto:a|a>`, undone; a link with a label of its own stays as written).
`[read_back.post_message]` expects that timestamp and the text posted.
Chat already had `get_message`; `[read_back.send_message]` reads the
message by the name the send returned and expects it and the text sent.
The Slack image in `catalog/slack.toml` is the one built from this change
(`brokered/build.sh magnus-slack`, on Strix Beta); Chat's image is
unchanged. Manifest versions stay, as for Outlook's read-back (2026-11-13).

The catalog check refuses a manifest with an irreversible tool that sends a
message (named `send…` or `post…`) and no `[read_back.<tool>]`
(`read_back::unread_send`). It checks the catalog's manifests, not an
owner's own `[[connectors]]` in the config, so no running server stops
starting.

**Reason.** Both sends ended "Not read back" and still counted as done,
though spec rev 147 says a send ends with a receipt and a read-back.
Fourth review, 2026-11-23, item 11.
