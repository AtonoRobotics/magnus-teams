# One finish step for a tool's answer

**Choice.** A tool's answer is finished in one place, `Kernel::finish`
(`boundary/finish.rs`), whichever try produced it: the first, or the resend
after a first try whose outcome was lost (`CallFailure::Unknown`, reconciled
to a resend). The finish step masks the vault secret, takes login codes out
where the app's answers lose them, holds attachments back
(`attachment::lift`), reads an irreversible action back, and labels the text
with the taint and sources computed from that answer's own data.

A brokered server's JSON-RPC error answer to `tools/call` is an unknown
outcome (`CallError::Unknown`), not "not sent": the tool may have acted
before it failed. An error answer to the handshake or the tool list still
means nothing happened.

**Reason.** The resend path had its own short finish: only the secret was
masked and the app's fixed label used. A Gmail read that took over 120
seconds, or lost its connection, was sent again and its one-time code and
attachment bytes reached the model; a resent send was recorded as done with
no read-back. A broker's error after the request was written was treated as
not sent, so a send that may have gone could be tried again as new.
