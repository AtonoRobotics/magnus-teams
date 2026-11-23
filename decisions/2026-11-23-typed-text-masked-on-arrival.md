# Text a person types into a card is masked as it arrives

What a person types into a card (an answer to the agent's question, a
reason for not allowing an action, a review's note, an endorsement's note)
becomes a `typed_secrets::Typed` the moment the request is read, and a
`Typed` is masked when it is made. Every event and every reply to the model
built from it carries the masked text. No event kind changes: the content is
masked before it is put in the event.

The list of event kinds masked when written (`mask_event`) stays, for the
messages and tasks it already covers.

Reason: the architecture review of 23 November (candidate 7) found a key
typed as an answer to a question, perhaps one steered by injected content,
reached the log and the model in the clear, as did a decline reason and an
endorsement note: masking by a list of event kinds misses every kind not on
it.
