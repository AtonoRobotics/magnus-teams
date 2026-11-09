# #270: each run is told its harness as it is

Magnus was told of tools it may not have: `chat.md` promised web
search on appliances without it, `agent.md` described sandbox checks
with no sandbox configured. And nothing said what wakes it, what it
keeps, or what bounds it.

## Shape

Every run's system context now carries a `# Your harness` section
between the instructions and the person's sections, generated from
what this appliance offers: wakes (message, task, schedule, the
first-run opener), keeps (memory, relationship notes, disagreements,
suggestion ledger, commitments, playbook), bounds (Cedar policies,
approvals, the kill switch), and capability lines only for what is
there — web search, sandbox checks, the browser.

The two static promises moved into the generated lines: `chat.md`
lists reads that are always there, `agent.md` points at the harness
for whether checks may run as sandbox commands. The text is built by
a pure `harness_text`, unit-pinned; the kernel method only feeds it
the availabilities.

Cards, grants, ledgers and wakes extend the section as they land, as
the issue asks.
