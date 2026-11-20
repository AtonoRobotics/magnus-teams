# Unified Settings modal (Personal + Admin), restored Models & Providers, and chat reasoning level

**Choice.**
1. **One unified Settings modal (`desktop/ui/app.js`, `desktop/ui/admin.js`, `desktop/ui/pages.css`):**
   - Personal (`Account & Privacy`, `Appearance`, `Notifications`, `Updates`) and Server & Admin (`Overview`, `Models & Providers`, `Connectors`, `People`, `Agents`, `Spend`, `Rules`, `Security`, `Operations`) now live inside one two-column Settings modal with clear grouped sidebar navigation.
   - `#/settings/...`, `#/admin/...`, and `#/models` all open the unified Settings modal directly on the matching section, fixing the broken Settings → Admin redirect.
2. **Restored Default Model picker & Provider Authentications (`desktop/ui/admin-spend.js`):**
   - **Models & Providers** renders a prominent **Default model** selector card (`Provider` + `Model` + **Use as default**) right at the top, followed immediately by **Connect providers** (API keys, OAuth sign-ins, and conformance suite) for owners, and a clear owner-only note for non-owner admins.
3. **Chat Reasoning Level selector (`desktop/ui/app.js`, `crates/magnus`, `crates/adapter-anthropic`, `crates/adapter-openai`):**
   - Added a `Reasoning` selector (`Auto` / `Off` / `Low` / `Medium` / `High`) beside the chat model picker (`modelPicker()` / `chosenModel()`), persisted locally and forwarded through `req::Model.reasoning` → `RunConfig.reasoning` → `Adapters::complete_with_reasoning` (`settings["reasoning_effort"]`).
   - Anthropic maps `off`/`low`/`medium`/`high` to named thinking token budgets (`2048` / `8192` / `16384` tokens); OpenAI passes `reasoning.effort` (Responses API) and `reasoning_effort` (Chat Completions API).

**Reason.** Splitting Settings and Admin into separate surfaces hid model defaults and provider sign-ins and broke navigation from Settings to Admin. Putting all Personal and Admin settings in one modal with a grouped sidebar makes configuration discoverable, and exposing reasoning effort in the chat picker gives users direct control per message.
