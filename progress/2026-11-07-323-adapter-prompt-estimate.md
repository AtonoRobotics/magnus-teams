# #323: the budget gate prices what the adapter sends

## What changed

- `crates/adapter-proto`: new `EstimatePrompt` request with `EstimateParams`,
  new `PromptSize` event, shared `CHARS_PER_TOKEN` and `chars_to_tokens`.
- `crates/adapter-anthropic`, `crates/adapter-openai` (chat and responses):
  the turn's request body moved into one `request_body` builder shared by
  asking and measuring; the chat API's dropped-document placeholder is now
  what the estimate counts. The estimate stays local (no client-version
  lookup on the OpenAI path).
- `crates/magnus`: new `Adapters::estimate_prompt`; the agent loop measures
  once per step and falls back to the kernel's bytes when the adapter
  cannot measure.
- `tests/scratch/tests/model_retries.rs`: a fake chat-completions server
  and a test attaching 1.2 MB of PDF under a $0.50 budget — the call goes
  out unclamped at 16000.

## Verification

- New test failed before the fix (`suspended` on `budget`, model never
  asked) and passes after.
- `model_retries` 5/5, `models` 2/2, `magnus` lib 89/89, adapter crates
  green; no new clippy lints on the changed lines.
- `cargo fmt` on this box rewrites the whole tree (the repo is not
  1.99-clean and CI does not gate on it): the change was redone by hand
  after a full revert, so the diff holds only these files.
