# Images through the model adapter

Spec rev 145, Images: "image_generate and image_edit run asynchronously
behind a placeholder. A fast model is the default, a premium one on
request." Until now the only provider was `[images] url` and `key`: an
OpenAI-compatible Images endpoint and an API key in the vault. A server
whose OpenAI access is a ChatGPT sign-in through the `codex` adapter has no
such key, so the tools answered "This server has no image provider set up."

## Choice

- `[images]` names one provider: `url` and `key` as before, or
  `adapter = "<id>"`, a configured model adapter. Anything else (both, one
  half of url and key, neither, an adapter that is not in `[[adapters]]`)
  stops the start with a message saying which. `fast_model` and
  `premium_model` keep their meaning: the image model asked for the fast
  default and for premium.
- The adapter contract gains one request, `image` (`ImageParams`: image
  model, settings, credential, prompt, an optional source picture for an
  edit, premium or not), answered by one `image` event (`Picture`: media
  type and base64) or an `error`. It is passed the credential and returns a
  refreshed one exactly as `complete` and `catalog` do, so the ChatGPT
  token is only ever sent to the adapter and refreshed there, and
  the refreshed one goes to the vault. The placeholder flow in `ui/images.rs`
  is unchanged; only where the bytes come from depends on the provider.
- The OpenAI-compatible adapter makes pictures by `image_api`:
  - `images` (the default for a Responses adapter): POST
    `{base_url}/images/generations`, or `/images/edits` with the source as a
    data URL, in the OpenAI Images shape, reading `data[0].b64_json`. This
    is the route Codex CLI itself uses with a ChatGPT sign-in today
    (`codex-rs/ext/image-generation` and `codex-api/src/endpoint/images.rs`,
    model `gpt-image-2`): the hosted `image_generation` Responses tool is no
    longer among the tools Codex sends.
  - `responses`: POST `{base_url}/responses`, streamed, `store: false`, with
    `tools: [{ type: "image_generation", model, quality, output_format:
    "png" }]` and `tool_choice: { type: "image_generation" }`, run on
    `image_host_model`; the picture is the `image_generation_call` item's
    `result`. Kept as the fallback in case the backend refuses the first.
  - A Chat Completions adapter makes no pictures unless `image_api` is set.
  The fast picture asks for quality `medium`, premium for `high`, so the
  two differ even when both name the same model.
- Every other adapter (Anthropic) answers the request with
  `invalid_request`, "this adapter cannot make images".
- The kernel waits `IMAGE_TIMEOUT` (180 s) for the adapter, as for the
  endpoint; the adapter's own request has the same limit.

## Reason

The adapter owns everything provider-specific, sign-in and refresh among
it (the adapter contract's first paragraph). Making the picture there
reuses the codex adapter's sign-in, headers and client-version lookup with
no second credential, and keeps a refreshed token flowing to the vault the
one way it already does. A new request kind is additive: an adapter built
before it answers "bad request", which the tool reports as a failed image.

Following Codex's own current route first is the reading least likely to
be refused by the ChatGPT backend; the hosted-tool route is one setting
away if it is.

Spend: pictures were never counted against a task's budget on the endpoint
route, and are not on this one. On a ChatGPT sign-in they draw on the
subscription's own limits, which the backend enforces; a refusal arrives as
a failed image.

## Not verified

The live ChatGPT backend was not called: the request shapes follow the
Codex source and the OpenAI documentation, and the test stands in a fake
provider.
