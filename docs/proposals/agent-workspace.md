# Agent workspace: a disposable VM per session, shared with the person

Status: proposal. Implementation deferred (owner, 2026-10-29). Touches
invariants 1, 2 and 4, so nothing is built until the owner approves this
design and the invariant wording below.

## Goal

The person and the agent work in the same workspace: a browser, a shell and a
file manager. The person can watch the agent, take control, and hand back.
"Teach me" works both ways: the agent shows the person how, or the person
shows the agent and it learns the procedure. The agent can act with the
person's credentials without ever holding them, and Magnus logs everything it
does.

## What the owner decided

- Every person has their own VM, never shared. It has Chrome, a shell and a
  file manager, and is otherwise minimal.
- The VM is disposable: files live in Magnus, credentials in the vault, and
  nothing else is kept in the VM.
- Security comes first.

## Design

### The session VM

- A new Firecracker microVM, under the jailer, for each person's session. A
  session is a task, or a "teach me" session that lasts while the person
  works with the agent. One person may have several sessions, each its own VM.
- Read-only base image: Chrome, a shell, a file manager, and the desktop
  pieces they need (Xvfb or equivalent, a window manager). Nothing else.
  Python and the document tools stay in the image only if the owner wants
  them (open decision 5).
- No network device. One vsock link to the boundary. A small forwarder in the
  guest gives Chrome and the shell an HTTP proxy on loopback that leads only
  to that link. The guest has no DNS.
- The writable disk is destroyed with the VM. Nothing carries from one session
  to the next: no planted files, no stale taint, no leftover processes.
- This replaces today's per-run sandbox VM and the host-side browser sidecar
  (`crates/magnus/src/browser.rs`): Chrome moves into the VM, so a browser
  exploit lands in a throwaway VM instead of on the appliance.

### Network: the boundary proxy

The proxy runs in the kernel's process space, outside the VM, and is the
VM's only way out.

- Each new destination (host and port) in a session is a boundary action:
  checked against policy, recorded in the log before it opens, and labelled
  untrusted when read. With private and untrusted taint both held, each new
  destination asks, as `web_fetch` does today (Rule of Two).
- The proxy resolves names. Loopback, private, link-local and metadata
  addresses and the appliance's own ports are refused, as `web_fetch`'s SSRF
  checks do now.
- Sites can be limited to what the task's contract names, or to a list the
  owner keeps.
- Only HTTP(S). SSH, raw sockets and other protocols are refused.
- Limits on bandwidth, connections and session time.

### Credentials

- API keys and OAuth tokens stay in the vault. For an approved host, the proxy
  decrypts the connection (with a certificate authority made for that one VM,
  its key held by the proxy) and adds the credential header. The VM sees a
  placeholder.
- Website sessions: the proxy keeps the cookie jar. It strips `Set-Cookie`
  from responses into the vault and adds the cookies to requests. Chrome
  never holds the session.
- Logging in: the person logs in through hand-off, or the kernel fills a
  saved password. See invariant 2 below for what this needs.
- Which sites the agent may use signed in as the person is the person's
  choice, within what the organization allows (invariant 8).

### Files

- Files stay in Magnus with their taint and provenance. A session gets copies
  of the files it needs; anything coming out passes the boundary as a logged,
  labelled delivery, as `deliver_file` does now.

### Live view and hand-off

- The app shows the session's screen, streamed by the appliance over the
  pinned HTTPS connection, with the person's input sent back. Nothing from the
  session runs in the app.
- One hand on the controls: the agent or the person. Taking control pauses the
  agent; handing back is an explicit button. Each input is logged as the
  person's or the agent's.
- Password fields, and whatever the person types while in control, are masked
  in the snapshots and screenshots the model receives.

### Teach me

- **The agent teaches.** The agent works a step at a time, says what it is
  doing and why, and waits. The person can take control to try a step while
  the agent watches and comments, then hand back.
- **The person teaches.** The person does the task while Magnus records it:
  pages, clicks, and typing with secrets masked. The agent writes it up as a
  procedure (a playbook item) that the person reviews and approves before it
  is used. What the person did is their instruction; the pages it happened on
  stay untrusted, so a page cannot add steps to the procedure (invariant 4).
  The recording and the procedure live in Magnus.

### Logging

Every request through the proxy goes into the event log, tied to the person,
the session, whether the agent or the person was in control, and the tool call
that caused it (for example, `browser_click "Send"` led to
`POST mail.example.com/send`, which got a 200). Screenshots before and after
consequential actions are kept as evidence. Secrets are removed by the
existing central redaction. How much of each request body is kept is open
decision 1.

## Invariants

- **1 (boundary; "the sandbox has no network").** The VM still has no network
  device, but it reaches the web through the proxy. The current test
  (`sandbox_has_no_network_device_and_reaches_nothing`) might still pass, but
  the words would no longer be true. Proposed wording: "the sandbox has no
  network device and reaches the network only through the boundary", with
  tests that every connection is a recorded boundary action, that internal
  addresses are refused, and that the guest resolves nothing. Owner's
  approval needed.
- **2 (credentials never in model context, the sandbox or the log).** Vault
  credentials and the cookie jar meet this. Two cases do not:
  - A person typing a password into a page in the VM puts it in the sandbox,
    briefly.
  - Some sites keep their session token in page storage, where the page, and
    so the sandbox, holds it.

  Options are in open decision 3. Owner's decision.
- **4 (untrusted content grants no authority).** Unchanged. Pages remain
  untrusted, signed-in pages are private and untrusted (like a mailbox), and
  the Rule of Two applies to every action inside a session.
- **6 and 8 (policy).** Which sites the agent may use as the person, and which
  of its actions there ask, come from policy the owner writes. They never
  exceed the person's rights.

## Threats and answers

| Threat | Answer |
| --- | --- |
| Code the agent runs sends data out | No network device; every connection is a boundary action under the Rule of Two |
| DNS used to carry data out | The guest resolves nothing; the proxy does |
| The VM reaches the appliance or the LAN | The proxy refuses internal addresses and the appliance's ports |
| Session cookies stolen from the browser | The cookie jar is in the proxy, not in Chrome |
| A browser exploit | It lands in a disposable VM with no credentials and no network device |
| A file planted for a later task | Nothing outlives the session; files in Magnus keep their taint |
| An injected page drives a signed-in action | Policy for each site, the Rule of Two, approvals the person sees live |
| One person's session used for another's task | VMs, views and cookie jars are per person; the boundary checks the owner |

## What stays risky

- Acting as the person is acting with their authority. On high-stakes sites
  (banking, payments, admin consoles) the safe default is that each write
  asks, or the person does it.
- A secret a page shows on screen can reach the model despite masking.
  Choosing which sites the agent may use is the backstop.
- Disposable VMs limit harm to one session, not within it.
- Sites with device-bound sessions will not work with the proxy's cookie jar.

## Open decisions for the owner

1. Request bodies in the log: full, redacted, or none (destination, method and
   status only)?
2. Signed-in sites: does every write ask, or only on sites marked sensitive?
3. Invariant 2 and logins:
   - (a) only the kernel types passwords, substituting a placeholder in the
     form post, which fails on sites that encrypt passwords in the page;
   - (b) reword so that a person's own typing during hand-off, in a VM
     destroyed afterwards, is allowed;
   - (c) refuse sites that keep tokens in page storage.
4. Logins: only made on the appliance through hand-off, or may a person bring
   sessions from their own Chrome?
5. Python and the document tools in the base image, or a shell and Chrome
   only?
6. The new wording of invariant 1, above.

## Build order

Each step is its own PR with tests, a DECISIONS.md line, and installed-app
tests on Linux; the owner tests Windows.

1. **Session VM.** The minimal image with Chrome, a shell and a file manager;
   one disposable VM per person and session; files in and out through the
   boundary. Measure start time and memory per session on strix-alpha before
   fixing any limits. Tests: no network device, destroyed at the end, never
   shared between people.
2. **Live view and hand-off.** Stream the screen to the app; take and give
   back control; attribution in the log; masking. Installed-app test.
3. **Boundary proxy.** The forwarder over vsock, DNS in the proxy, internal
   addresses refused, each new destination a recorded action under the Rule
   of Two. Retire the host-side browser sidecar. New injection-suite cases.
   Invariant 1 rewording if approved.
4. **API credentials through the proxy.** Header injection for approved
   hosts. Invariant 2 tests extended to the proxy.
5. **Website sessions.** The cookie jar in the proxy, hand-off login, the
   site list the person keeps, approvals for signed-in sites.
6. **Teach me.** Step mode with narration and hand-off; recording a
   demonstration; the procedure proposed as a playbook item and approved by
   the person.

## Reused

The Firecracker sandbox and its build (`sandbox/`), computer use (Xvfb,
xdotool), the browser tools' snapshot and refs, `web_fetch`'s address checks,
the vault and central redaction, the event log, taint and the Rule of Two,
`playbook_update`, and the app's pinned connection.
