# Every charged token is booked

**Choice.**

- An adapter whose turn fails after the provider began charging sends the
  usage counted so far before the error (`Sink::charged_error`): the
  Anthropic adapter when the stream breaks, an event cannot be read, the
  stream reports an error ("overloaded") or a tool input is not JSON; the
  OpenAI-compatible adapter when a reply's tool arguments are not JSON or
  its stream fails after its usage came (Chat Completions and Responses; a
  failed Responses reply's usage is read too). The kernel already books
  usage that comes with an error.
- A paid call owns its timeout (`Call::timeout`, `Bill::timeout`). One its
  timeout cuts off is booked as a failed `charged` change at its quoted
  input price: the provider was sent the prompt and bills it. The
  quarantined extraction sets its 30 seconds there
  (`reviewer_call_within`) instead of wrapping the call.
- Pictures and knowledge embeddings go through the one paid call
  (`Kernel::pay`, which `paid_call` now uses for a model's turn): the model
  check, the worst case fitted to what is left, the call, the record.
  - A picture is charged to its run as `Spender::Made` (dollars alone: the
    action was counted already) at `[images] fast_usd` or `premium_usd`, a
    price per picture. A picture made through an adapter is held to the
    person's model access; one made by the server's own images endpoint is
    the server's choice. One that could not be made even with nothing else
    in flight is refused at once with a tool error; the rest is decided in
    the background, as the picture is made.
  - An embedding is charged to the person who searched, or to the server
    for indexing, at `[knowledge.embeddings] usd_per_mtok`; `0` for a
    local server, which is still recorded.
  - Unpriced, either goes on only where no spend limit applies, as any call
    nobody can price (2026-11-23-one-way-to-make-a-paid-call). A refused
    embedding falls back to word search, as a down server did.

**Reason.** A failed attempt that the provider charged for was booked at
$0 and retried, a timed-out side call booked nothing and freed its hold,
and pictures and embeddings went around the paid call with no price, no
record and no model check.
