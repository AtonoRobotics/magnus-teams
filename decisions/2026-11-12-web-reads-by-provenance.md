# Web reads are judged by where the address came from, not asked page by page

**Choice.** Once the work has read private data and untrusted content, a web or browser read still sends nothing out, and so asks no one, when:
- it is a search (`web_search`), which goes to the configured search provider and not to an address a page chose;
- it reads the page already open (`browser_snapshot`, `browser_screenshot`, `browser_back`);
- it opens an address (`web_fetch`, `browser_navigate`) that appears exactly in what the person wrote, or in what a search or a public page returned to this work or the runs above it. A trailing slash is ignored.

Only an address the model made up itself still asks (case 3, `kernel:rule_of_two`). So do the browser's clicks, typing and selections. Every such ask names the site: a navigation's own host, or for a click or typing, the host of the page it acts on (`web_reads::page_host`). Allowing a site is already remembered for that person (`asks::remembered`). The card now says so: its Allow reads "Allow <site>" (`allow_covers: "site"`), and the answered card reads "Allowed for this site from now on". Code: `crates/magnus/src/web_reads.rs`, wired in `boundary.rs` policy_stage.

**Reason.** The owner: "I am still doing approval for every possible webpage … When I use Grok, Claude, ChatGPT I don't need to approve web access." The cause was that every web and browser call counted as sending data out. Private taint lasts the whole thread, and comes from commitments and uploads too, so after one page read every later page, search and browser step asked. Each ask covered one action. A search and a browser step had no host to remember, so they asked every time.

Those products block the same leak (a URL that carries private data) by checking where the address came from, not by asking:
- Claude's web fetch may open only URLs the user gave or that came from earlier search or fetch results, and never builds one (Anthropic web fetch tool docs).
- OpenAI's agents open on their own only URLs that an independent crawler has already seen on the public web, and ask about the rest ("Keeping your data safe when an AI agent clicks a link", 2026-01-28).
- ChatGPT agent asks before consequential steps such as a purchase, not before reading.

Magnus has no independent index, so it uses Claude's rule.

**What stays.** Sends, posts, payments and app writes keep their asks exactly as before (invariants inv4 pass unchanged). A made-up address after private data still asks, so the K34 leak stays closed.

**Residual risk, accepted as Anthropic accepts it.** A hostile page can offer many links and steer which one is followed, leaking a little data through the choice (Simon Willison, 2026-07-15, on Claude's web fetch). That needs private data in the work, a successful injection and a site built for it. It leaks a few bits per hop and never sends the content itself. A public-URL index like OpenAI's would close it, and is the next step if this matters.
