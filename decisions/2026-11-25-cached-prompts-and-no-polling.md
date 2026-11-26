# Prompts are cached, a reply may call several tools, and no run polls its sub-agents

## Decision
1. The Responses adapter sends a `prompt_cache_key` naming the conversation
   (a hash of model, instructions and opening item), so every turn of one
   run reaches the same provider cache. A provider that refuses the field
   is set `prompt_cache_key = false`. The same key goes in the
   `session_id` and `x-client-request-id` headers, as Codex sends them:
   with the body's key alone, 31 turns read nothing from cache; with the
   headers, the next 27 read 60% of their input from it.
2. The Responses adapter lets a reply ask for several tool calls
   (`parallel_tool_calls`, default true; a provider can set it false). The
   kernel already finished a reply's calls in order; the Anthropic and Chat
   adapters never forbade it.
3. The Anthropic adapter marks the instructions and the latest message
   with `cache_control`, so each turn reads the earlier part back from
   cache.
4. A run calling `get_task` on its own sub-agent while that one works waits
   for it in the kernel, up to `OWN_CHILD_WAIT` (ten minutes), and the chat
   instructions say to ask once. A sub-agent of another run, or one that
   has finished or waits on a person, is answered at once.

## Reason
The owner, 2026-11-25: one research request spent 2% of a weekly
subscription budget. Production's log showed:
- The chat turn started three background sub-agents and called `get_task`
  74 times in five minutes, each call resending its ~40k-token context:
  3.46M input tokens on that turn, nearly all of it waiting.
- Over seven days 215M input tokens went to the ChatGPT models and 17% came
  from cache. No cache key was sent, so turns of one run scattered across
  the provider's caches: the research turn's tool list never changed, yet
  more than half its turns read nothing from cache.
- With one tool call per reply, the heartbeat took 18 turns for 17 reads,
  each resending a 10k-36k-token context.

Cached input is priced at about a tenth of fresh input by both providers.
None of this changes
what a run may do or what it is shown; it changes only how often the same
context is paid for.

## Also
d924a56 added `system` to a schedule's record without re-pinning the state's
folded form, leaving main's fast tier red. The pin is updated to show the
new field; nothing else in the fold changed.
