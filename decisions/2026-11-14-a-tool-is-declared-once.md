# A tool is declared once

A built-in tool's name had to agree across five to eight places: the
`BUILTINS` table (app, risk, label, audience), the hand-written specs in
`builtin_specs`, `chat_specs`, `tools_identity.rs` and the computer module,
the name-to-`Op` match in `tools_core::op`, the boundary's per-tool label
arms, the evidence wording, the web-read `SOURCES`, `knowledge::source_of`,
the team agent's own table, its dispatch match and its spec list, and the
app's copy of the team wording in `team-chat.js`. A tool added in one and
missed in another was offered with no risk class, carried out by nothing, or
labeled wrongly.

**One registry declares each tool** (`crates/magnus/src/tool_registry/`):
its name, what the boundary knows of it (`Class`: app, risk, label,
audience), what carries it out (`Exec`: a record, a core `Op` or a `TeamOp`),
which list offers it (`Offer`: a run's, for what the server has; chat's;
`find_tools`; the team's), its input schema, its card wording (`doing`),
what kind of web read it is, where its result's label comes from (`Taint`),
whether it serves a team thread, and whether its calls are its person's
alone. `tools::BUILTINS`, `team::BUILTINS`, `builtin_specs`, `chat_specs`,
`find_tools_spec`, `team::specs`, `tools_core::op`, the team dispatch, the
boundary's labels, the web-read rules, `person_only_tool`, `in_team_run` and
the evidence wording all read it. To add a tool is now one declaration and
its description in `instructions/tools.json`, plus its code where an `Op`
does something new.

**The specs the model sees did not change, byte for byte.** Prompt caching
and evals depend on them, so before the move every list a run can be offered
(each sandbox, search, browser and computer combination, chat's,
`find_tools` and the team's) was snapshotted with its order and each spec's
serialized bytes (`tool_specs_pin.rs`), and the test holds them. A list's
order is its declarations' order.

**Descriptions stay in `instructions/tools.json`**, the versioned
instruction text (X11). The team tools' descriptions, which were Rust
literals, moved there unchanged, so every declared tool is described in one
place. A test holds that every declared tool has a description, a schema and
card words, and that the file describes nothing no tool uses.

**The team agent's tools are a view of the same declarations.** A run tool
that serves a team thread says so (`in_team`); the workspace's own tools are
declared in the same registry with `Exec::Team`. `team_read_knowledge`, the
one a person's agent has too, is known by its op, not a name.

**One set of words says what a call is doing**, on a personal task's card
and on a team thread's work row. The server sends them in the row's
`status_line`, a field the row always had and the team sent empty, so the
wire changes in no breaking way; the app drops its copy (`doingWords`) and
says "Working" if an older server sends none. The team row's wording moves
to the registry's: a web fetch reads "Reading a page" (it said "Searching
the web"), the computer "Working on its computer" (it said "Using the team
computer"), and an app's tool names its action ("Send message in gmail", it
said "Using gmail"), as a personal card always did. A browser action that
read as its name ("Browser click") now reads "Browsing".

**Retired names still read.** `task_contract`, retired at rev 147, has no
declaration but stays a record (`tools::RECORDS`), so its stored calls read
as before. No event kind changed.

**What still matches a name**, and why: the browser, bundle, side-tool,
chat-control and desktop-action modules still pick their sub-action by name
inside their one `Op` (`Op::Apps` was split into three ops instead); the
rich-answer and image tools (`tools_ui.rs`) and the other families keep
their own tables; and placement rules elsewhere (`chat_work.rs`,
`policy.rs`, `run/mod.rs`'s record handling) name the tools they rule on. A
test holds that the files this change derives (`tools.rs`, `tools_core.rs`,
`families.rs`, `evidence.rs`, `web_reads.rs`, `knowledge.rs`,
`team/agent.rs`) name no declared tool, that the boundary compares no call
to one, and that every `Op` carries out a declared tool.
