# Rich answers are short line-based blocks the client compiles

The agent writes a fenced `magnus-ui` block in its answer. Each line names one element from a versioned catalog, and the client compiles the lines as the text streams, so elements draw as they arrive and no extra stream is needed for them. `magnus-html` blocks run in a frame with no same-origin access, under a content security policy placed before the agent's markup, which later markup can only tighten.

The kernel also compiles each block, so a block that cannot be drawn goes back to the model at once. The two compilers are held to one set of cases in `desktop/tests/fixtures/ui-spec.json`, and the catalog table in Rust is the one definition. The client says what it can draw and the agent may write only that.

An edit is a search and replace on a named block (`ui_edit`), and the current text is worked out from the log. A result that no longer draws is refused.

A form works like a button: it starts a new request labelled as outside data, and only the fields the form asked for are carried. It approves nothing.

Slides are a `deck` element. The kernel draws PPTX and PDF from the same spec with plain code, and each export is served only to the person whose answer holds the deck. A per-slide edit is an ordinary `ui_edit`, so there is no separate slide store.

Agent HTML runs in `sandbox="allow-scripts"` frames with an opaque origin, and a CSP is set before anything the agent wrote. `html_screenshot` renders the same document in the person's browser VM with its network shut off, so the agent can check its own page.

`image_generate` and `image_edit` return a placeholder at once and the provider call runs apart from the run. Configure the provider in `[images]`; the key is a vault entry. Fast is the default model and premium is used only when asked. An image is stored under its run and removed with the run's content.

A connector's view (MCP Apps, `_meta.ui.resourceUri`) is fetched by the kernel when its tool answers, kept with the run, and drawn in the same sandbox as agent HTML. A tool the view asks to run starts a new request labelled as outside data, which approves nothing. Only remote connectors offer views so far; a brokered connector's view is not fetched.
