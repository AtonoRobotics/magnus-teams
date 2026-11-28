// Connector catalog & Apps Library (extracted from app.js in Tier 5 Phase 4).
"use strict";

// ---------- The Library: the connector catalog ----------

// The Library is a catalog of apps, laid out as the owner's catalog mockup
// draws it in every look (connectors spec rev 48, Library): Discover shows
// the whole catalog, Yours what this person has connected or needs to fix;
// one search box and category chips; cards in sections, each with its tile,
// the review mark, one plain sentence, who makes it, and one action at its
// corner. Connecting is a click and the person's own login on the
// provider's page: nothing is typed in Magnus but the search.
const Lib = { tab: "discover", cat: "all", q: "" };
const CATEGORIES = [["all", "All"], ["email", "Email & chat"], ["files", "Files"], ["code", "Code"], ["work", "Work"], ["design", "Design & CAD"]];
const Connecting = {};
const SVG = {
  plus: `<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M12 5v14M5 12h14"/></svg>`,
  check: `<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M5 12.5l4.5 4.5L19 7.5"/></svg>`,
  again: `<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M20 12a8 8 0 1 1-2.4-5.7"/><path d="M20 4v5h-5"/></svg>`,
  lock: `<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="5" y="11" width="14" height="10" rx="2"/><path d="M8 11V8a4 4 0 0 1 8 0v3"/></svg>`,
  search: `<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><circle cx="11" cy="11" r="7"/><path d="M20 20l-3.5-3.5"/></svg>`,
};
const reviewed = `<svg class="lc-rev" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" role="img" aria-label="Reviewed by Alpha Vector"><path d="M12 2.5l2.3 1.7 2.8-.2 1 2.6 2.4 1.5-.7 2.8 1 2.6-2.3 1.7-.5 2.8-2.8.4-1.9 2.1-2.6-1.1-2.6 1.1-1.9-2.1-2.8-.4-.5-2.8L2.6 15.2l1-2.6-.7-2.8 2.4-1.5 1-2.6 2.8.2z"/><path d="M8.5 12.2l2.3 2.3 4.7-4.8"/></svg>`;
const stateOf = c => (Connecting[c.name] ? "connecting" : c.state);
const tintOf = c => (/^#[0-9a-f]{6}$/i.test(c.listing?.tint || "") ? c.listing.tint : "#3A3F48");
const tile = (c, big) => {
  const logo = typeof AppLogos !== "undefined" ? AppLogos.svg(c.name) : "";
  const inner = logo || esc(c.listing?.letters || (c.title || "?").slice(0, 2));
  return `<span class="ctile${big ? " big" : ""}${logo ? " has-logo" : ""}" aria-hidden="true" style="background:${tintOf(c)}">${inner}</span>`;
};
const byline = c => (c.listing?.publisher ? `by ${c.listing.publisher}` : "Added by your organisation");
const provider = c => c.listing?.publisher || c.title;

function connectorAction(c) {
  const name = esc(c.title);
  switch (stateOf(c)) {
    case "connecting": return `<span class="lc-act busy" role="status" aria-label="Signing in to ${name}"><span class="spin"></span></span>`;
    case "connected": return c.more_access
      ? `<button class="lc-act warn" data-act="connect" data-app="${esc(c.name)}" aria-label="Approve more access for ${name}">${SVG.again}</button>`
      : `<a class="lc-act ok" href="#/library/${enc(c.name)}" aria-label="${name} is connected. Open details">${SVG.check}</a>`;
    case "needs_attention": return `<button class="lc-act warn" data-act="connect" data-app="${esc(c.name)}" aria-label="Sign in to ${name} again">${SVG.again}</button>`;
    case "local": return `<button class="primary lc-enable" data-act="connect" data-app="${esc(c.name)}">Enable</button>`;
    default: return c.ready === false
      ? `<button class="lc-act off" aria-disabled="true" data-act="connect" data-app="${esc(c.name)}" title="Not ready on this server yet" aria-label="${name} is not ready on this server yet">${SVG.plus}</button>`
      : `<button class="lc-act" data-act="connect" data-app="${esc(c.name)}" aria-label="Connect ${name}">${SVG.plus}</button>`;
  }
}

function connectorCard(c) {
  const st = stateOf(c);
  const line = {
    connecting: `<p class="lc-note">Finish signing in on ${esc(provider(c))}'s page… <button class="link" data-act="connect-page" data-url="${esc(Connecting[c.name] || "")}">Open it again</button></p>`,
    needs_attention: `<p class="lc-note warn">Sign in again to keep using ${esc(c.title)}.</p>`,
    connected: c.account ? `<p class="lc-note ok"><span class="lc-dot ok" aria-hidden="true"></span>Connected as ${esc(c.account)}</p>` : `<p class="lc-note ok"><span class="lc-dot ok" aria-hidden="true"></span>Connected</p>`,
  }[st] || `<p class="lc-note"><span class="lc-dot" aria-hidden="true"></span>Not connected</p>`;
  const abilities = (c.listing?.abilities || []).slice(0, 2);
  const abHtml = abilities.length ? `<div class="lc-ab-row">${abilities.map(a => `<span class="lc-ab">${esc(a)}</span>`).join("")}</div>` : "";
  return `<article class="lcard" data-connector="${esc(c.name)}" data-state="${esc(st)}">
    <div class="lc-head">${tile(c)}<div class="lc-title-wrap"><div class="lc-name"><h3><a href="#/library/${enc(c.name)}">${esc(c.title)}</a></h3>${c.listing?.reviewed ? reviewed : ""}</div><p class="lc-by">${esc(byline(c))}</p></div>${connectorAction(c)}</div>
    <div class="lc-main"><p class="lc-sum">${esc(c.summary)}</p>${abHtml}</div>
    <div class="lc-foot">${line}<a class="lc-more" href="#/library/${enc(c.name)}">Details →</a></div></article>`;
}

// What a search or a chip leaves: a service's name, or what the person
// wants done, against its name, sentence, publisher and abilities.
function libMatches(c) {
  if (Lib.cat !== "all" && (c.listing?.category || "work") !== Lib.cat) return false;
  const words = Lib.q.toLowerCase().split(/\s+/).filter(Boolean);
  const hay = [c.title, c.summary, c.listing?.publisher, ...(c.listing?.abilities || [])].join(" ").toLowerCase();
  return words.every(w => hay.includes(w));
}

function librarySections(all) {
  const shown = all.filter(libMatches);
  const of = (...states) => shown.filter(c => states.includes(stateOf(c)));
  const sections = Lib.tab === "discover"
    ? [["local", "On this computer", "Programs Magnus found here. Enable one and Magnus can work in it.", of("local")],
       ["attention", "Needs attention", "Sign in again and Magnus picks up where it left off.", of("needs_attention")],
       ["popular", "Popular", "Connect any of these by signing in to your own account.", of("available", "connected", "connecting")]]
    : [["connected", "Connected", "What Magnus can work in for you right now.", of("connected", "connecting")],
       ["attention", "Needs attention", "Sign in again and Magnus picks up where it left off.", of("needs_attention")],
       ["local", "Ready to enable", "Programs on this computer you have not turned on yet.", of("local")]];
  const html = sections.filter(x => x[3].length).map(([k, title, sub, items]) => `<section class="lsect" data-section="${k}">
    <div class="lsect-head"><div><h2>${title}</h2><p>${sub}</p></div><span class="lsect-n">${items.length === 1 ? "1 connector" : `${items.length} connectors`}</span></div>
    <div class="lgrid">${items.map(connectorCard).join("")}</div></section>`).join("");
  if (html) return html;
  return `<div class="lempty">${Lib.q || Lib.cat !== "all" ? "No connectors match. Try another word, or ask Magnus what you want done."
    : Lib.tab === "yours" ? "Nothing connected yet. Discover shows every app Magnus can work in." : "Your organisation's catalog is empty."}</div>`;
}

async function showLibrary(main, name) {
  Lib.main = main;
  if (name) return showConnectorPage(main, name);
  // Apps opens at once on the list the app already holds, and refreshes
  // under it; it waited on the server before (owner, 2026-11-27: "Apps opens
  // with a very long delay").
  const held = Array.isArray(App.connectors) ? App.connectors : null;
  const connectors = held || await Resources.read("Apps", "/v1/connectors", "connectors",true);
  // An app blocked for your groups is not in your Library; the owner still
  // manages it in Settings.
  Lib.all = (connectors || []).filter(c => c.for_you !== false);
  const seg = (k, label) => `<button data-lib-tab="${k}" aria-pressed="${Lib.tab === k}">${label}</button>`;
  main.innerHTML = `<div class="libpage"><header class="lhead"><div><h1>Apps</h1><p>Connect the apps and programs you want Magnus to work in. You only ever sign in.</p></div>
    <a class="icon-btn lclose" href="#/home" aria-label="Close Apps">${icon("close")}</a></header>
    <div class="lbar"><div class="lseg" role="group" aria-label="Show">${seg("yours", "Yours")}${seg("discover", "Discover")}</div>
    <div class="lsearch">${SVG.search}<label class="sr" for="libq">Search apps and capabilities</label><input id="libq" type="search" value="${esc(Lib.q)}" placeholder="Search apps and capabilities" autocomplete="off"></div></div>
    <div class="lchips" role="group" aria-label="Category">${CATEGORIES.map(([k, l]) => `<button data-lib-cat="${k}" aria-pressed="${Lib.cat === k}">${esc(l)}</button>`).join("")}</div>
    <p><a href="#/chat/new" data-ask-app>Ask Magnus to find an app</a></p><div id="lib-programs"></div><div id="lib-outside-agents"></div><div id="lib-sections">${connectors ? librarySections(Lib.all) : '<p class="empty">Apps are unavailable. Retrying when connected.</p>'}</div></div>`;
  bindConnectorActs(main);
  mountPrograms(main).catch(() => {});
  mountOutsideAgents(main).catch(() => {});
  const update=async()=>{
    const connectors=await Resources.read("Apps","/v1/connectors","connectors",true);
    if(!connectors || !main.querySelector('#lib-sections'))return;
    App.connectors=connectors;Lib.all=connectors.filter(c=>c.for_you!==false);
    paint($('#lib-sections',main),librarySections(Lib.all));
  };
  App.redraw=update;onLeave(()=>{if(App.redraw===update)App.redraw=null;});
  if (held) update().catch(() => {});
  $("[data-ask-app]",main).onclick=()=>{const query=$("#libq",main).value.trim();App.appSearch=query ? `Find an app for: ${query}` : "Help me find an app for ";};
  let t;
  $("#libq", main).addEventListener("input", e => { clearTimeout(t); t = setTimeout(() => { Lib.q = e.target.value.trim(); $("#lib-sections", main).innerHTML = librarySections(Lib.all); }, 150); });
}

// One connector's page, in the two states the mockup draws: before
// connecting, what Magnus can do with it and what it asks for; connected,
// as whom, what it can and cannot do, when, what Magnus did, Disconnect.
function connectorBody(c) {
  const st = stateOf(c), L = c.listing || {};
  const head = `<div class="cp-head">${tile(c, true)}<div><div class="cp-name"><h2 class="ptitle">${esc(c.title)}</h2>${st === "connected" ? `<span class="chip ok">${SVG.check}Connected</span>` : ""}</div>
    <p class="muted">${esc(byline(c))}${L.reviewed ? " · Reviewed by Alpha Vector" : ""}</p></div></div>`;
  const recent = `<h3>Recent actions</h3>${c.recent?.length ? `<ol class="cp-recent">${c.recent.map(u => `<li><a href="#/task/${enc(u.task)}" data-open="task:${esc(u.task)}">${esc(u.action.charAt(0).toUpperCase() + u.action.slice(1))}</a><span class="small faint">${esc(when(u.at))}${u.worked === false ? " · did not work" : ""}</span></li>`).join("")}</ol>` : `<p class="empty">Magnus has not used it for you yet.</p>`}`;
  if (st === "connected" || (st === "needs_attention" && c.connected_at)) {
    return `${head}${st === "needs_attention" ? `<p class="lc-note warn">Your sign-in to ${esc(c.title)} stopped working. Sign in again to keep using it.</p>` : ""}
      <dl class="kv cp-kv"><dt>Account</dt><dd>${esc(c.account || "Your own account")}</dd><dt>It can</dt><dd>${esc(L.can || c.summary)}</dd>${L.cannot ? `<dt>It cannot</dt><dd>${esc(L.cannot)}</dd>` : ""}
      ${c.connected_at ? `<dt>Connected</dt><dd>${esc(when(c.connected_at))}</dd>` : ""}<dt>Last used</dt><dd>${c.last_used ? esc(when(c.last_used)) : "Not yet"}</dd></dl>
      ${!c.watched ? "" : c.instant === "automatic" ? `<h3>Instant updates</h3><p class="small muted">Once you connect ${esc(c.title)}, Magnus signs up for its notifications for you and hears about new items as they arrive. It also checks on a schedule.</p>`
        : c.instant === "needs_public_url" ? `<h3>Instant updates</h3><p class="small muted">Magnus checks ${esc(c.title)} for new items on a schedule. Instant updates need a public address for this server, which your admin sets.</p>`
        : `<h3>Instant updates</h3><p class="small muted">Magnus checks ${esc(c.title)} for new items on a schedule. If ${esc(c.title)} can send notifications to an address, give it your push address and Magnus hears about new items as they arrive.</p><div class="row"><button data-act="push-copy" data-app="${esc(c.name)}">Copy push address</button></div>`}
      ${recent}<div class="row cp-actions">${st === "needs_attention" ? `<button class="primary" data-act="connect" data-app="${esc(c.name)}">Sign in to ${esc(c.title)} again</button>` : ""}${c.signs_in ? `<button data-act="disconnect" data-app="${esc(c.name)}">Disconnect ${esc(c.title)}</button>` : ""}${c.generated ? `<button data-act="gen-download" data-app="${esc(c.name)}">Download definition</button><button data-act="gen-delete" data-app="${esc(c.name)}">Delete</button>` : ""}</div>${c.needs_token ? `<div class="row"><input type="password" autocomplete="off" aria-label="Token for ${esc(c.title)}" data-gen-token><button data-act="gen-token" data-app="${esc(c.name)}">Save token</button></div>` : ""}`;
  }
  const can = L.abilities?.length ? L.abilities : c.allows || [];
  const go = st === "connecting"
    ? `<p class="lc-note">Finish signing in on ${esc(provider(c))}'s page, then come back here. <button class="link" data-act="connect-page" data-url="${esc(Connecting[c.name] || "")}">Open it again</button></p>`
    : c.ready === false ? `<p class="small muted">${esc(c.title)} is not ready on this server yet.</p>`
    : `<button class="primary cp-connect" data-act="connect" data-app="${esc(c.name)}">${st === "needs_attention" ? `Sign in to ${esc(c.title)} again` : `Connect ${esc(c.title)}`}</button>`;
  return `${head}<h3>What Magnus can do with ${esc(c.title)}</h3>${can.length ? `<ul class="cp-list">${can.map(l => `<li>${esc(l)}</li>`).join("")}</ul>` : `<p class="empty">Connecting shows what Magnus can read, change and send in it.</p>`}
    ${L.asks?.length ? `<h3>What it asks for</h3><ul class="cp-list">${L.asks.map(l => `<li>${esc(l)}</li>`).join("")}</ul>` : ""}
    <p class="cp-lock">${SVG.lock}<span>You sign in on ${esc(provider(c))}'s own page. Magnus never sees your password, and you can disconnect at any time.</span></p>${go}`;
}

async function showConnectorPage(main, name) {
  const c = await Server.connectors.get(name);
  main.innerHTML = `<div class="libpage cpage"><p class="small"><a href="#/library" class="cp-back" aria-label="Back to Apps">← Apps</a></p><section class="cp" aria-label="${esc(c.title)}">${connectorBody(c)}</section></div>`;
  bindConnectorActs(main);
}

// The same connector in the panel, from a card in chat or a task.
async function connectorView(name) {
  return plain(`<section class="cp">${connectorBody(await Server.connectors.get(name))}</section>`);
}

// What Disconnect did: ended at the app, or, where the app could not end it
// itself, ended here, with the app's own page where the person removes
// Magnus (connectors spec, Lifecycle).
function disconnected(r) {
  if (!r || r.at_provider !== false) { toast("Disconnected. Your sign-in is ended at the app too."); return; }
  toast(r.remove_at ? `Disconnected here. The app could not end your sign-in itself: remove Magnus at ${r.remove_at}` : "Disconnected here. The app could not end your sign-in itself: remove Magnus in its own settings.", 10000);
}

// Connect, again or for the first time, and Disconnect, wherever a card is.
function bindConnectorActs(root) {
  if (root.dataset.connActs) return;
  root.dataset.connActs = "1";
  root.addEventListener("click", async e => {
    const tab = e.target.closest("[data-lib-tab]"), cat = e.target.closest("[data-lib-cat]");
    if (tab || cat) {
      if (tab) Lib.tab = tab.dataset.libTab; else Lib.cat = cat.dataset.libCat;
      return showLibrary(root);
    }
    const a = e.target.closest("[data-act]");
    if (!a || !root.contains(a)) return;
    if (a.dataset.act === "connect") { e.preventDefault(); connectApp(a.dataset.app).catch(err => toast(err.message)); }
    if (a.dataset.act === "connect-page") { e.preventDefault(); Magnus.openExternal(a.dataset.url).catch(() => {}); }
    // The address is this server's, as the app reaches it; it is a credential, so it is copied, not shown.
    if (a.dataset.act === "push-copy") { e.preventDefault(); try { const { url } = await Server.connectors.push(a.dataset.app); if (!url) { toast("Instant updates need a public address for this server, which your admin sets."); return; } await copyText(url); toast("Push address copied. Keep it private: anyone with it can make Magnus check this app."); } catch (err) { toast(err.message); } }
    if (a.dataset.act === "gen-download") { e.preventDefault(); try { toast(`Saved ${await Magnus.download(`/v1/connectors/${enc(a.dataset.app)}/definition`, `${a.dataset.app}.json`)}`); } catch (err) { toast(err.message); } }
    if (a.dataset.act === "gen-token") { e.preventDefault(); const input = $("[data-gen-token]", root); try { await Server.connectors.setToken(a.dataset.app, input.value); input.value = ""; toast("Saved."); connectorsChanged(); } catch (err) { toast(err.message); } }
    if (a.dataset.act === "gen-delete") {
      e.preventDefault();
      if (!await ask("Delete this connector? Its token and grants are revoked and its approval rules are removed.", "Delete")) return;
      try { await Server.connectors.removeGenerated(a.dataset.app); location.hash = "#/library"; connectorsChanged(); } catch (err) { toast(err.message); }
    }
    if (a.dataset.act === "disconnect") {
      e.preventDefault();
      if (!await ask("Disconnect? Your sign-in is ended at the app, and Magnus can no longer use it for you until you connect again.", "Disconnect")) return;
      try { disconnected(await Server.connectors.disconnect(a.dataset.app)); connectorsChanged(); } catch (err) { toast(err.message); }
    }
  });
}

// Wherever connectors are shown, they are drawn again.
async function connectorsChanged() {
  const list = await Resources.read("Apps", "/v1/connectors", "connectors", true);
  if (Array.isArray(list)) { App.connectors = list; Lib.all = list.filter(c => c.for_you !== false); }
  const m = /^#\/library(?:\/([^/]+))?$/.exec(location.hash);
  if (m && Lib.main?.isConnected) showLibrary(Lib.main, m[1]);
  else App.redraw?.();
  drawPanel();
}

// Connect: the system browser opens at the app's own sign-in page; the
// answer comes back to this computer, and the card turns Connected.
function onThisComputer(u) {
  try {
    const x = new URL(u);
    return x.protocol === "http:" && x.hostname === "127.0.0.1" && x.pathname === "/callback" && x.port;
  } catch { return false; }
}

async function connectApp(name) {
  // Slack's registered address is https. The desktop listens on the handoff,
  // and the sign-in tells Slack the https address, which the exchange repeats.
  let card = (Lib.all || []).find(c => c.name === name);
  if (!card) { try { card = await Server.connectors.get(name); } catch { card = null; } }
  // A click on a connector this server cannot sign anyone in to yet used to
  // do nothing at all (2026-11-09): say why instead.
  if (card && card.ready === false) {
    toast(`${card.title || name} can't be connected on this server yet: Alpha Vector has not set up its sign-in here. Ask your organisation's owner.`);
    return;
  }
  // The one screen connecting shows; accepting it is all the approval Magnus
  // needs to use the app (spec rev 140). Signing in again needs no new one.
  const again = card && card.state === "needs_attention", signsIn = !card || card.signs_in !== false;
  // A browser has no listener on this computer: the sign-in comes back to
  // the server's own address instead, where it is finished (connect-web.js).
  const web = signsIn && !Magnus.loopbackOpen, back = web ? ConnectWeb.returnFor(card) : null;
  if (web && !back) { toast(ConnectWeb.why(card, name)); return; }
  if (!again && !await consentToConnect(name)) return;
  if (!signsIn) { toast("Connected."); connectorsChanged(); return; }
  if (back) return ConnectWeb.go({ name, title: card?.title || name }, back, redirect => Server.connectors.signIn(name, redirect)).catch(e => toast(e.body?.message || e.message));
  const listenAt = card && (card.handoff || (onThisComputer(card.redirect_uri) ? card.redirect_uri : null));
  const caught = await Magnus.loopbackOpen(listenAt || null);
  const redirect = card && card.redirect_uri && !onThisComputer(card.redirect_uri) ? card.redirect_uri : caught;
  const r = await Server.connectors.signIn(name, redirect);
  Connecting[name] = r.authorize_url;
  connectorsChanged();
  Magnus.openExternal(r.authorize_url).catch(() => {});
  try {
    const got = await Magnus.loopbackWait();
    if (got.state !== r.state) throw new Error("That sign-in came back for another request. Try again.");
    await Server.connectors.completeSignIn(name, { state: got.state, code: got.code, iss: got.iss });
    toast("Connected.");
  } catch (e) { toast(e.message); }
  finally { delete Connecting[name]; connectorsChanged(); }
}

function libFileKind(f) {
  const n = (f.name || "").toLowerCase();
  const c = (f.media_type || f.content_type || "").toLowerCase();
  if (f.kind === "page" || c.includes("html") || c.includes("markdown") || /\.(html?|md|markdown)$/.test(n)) return "pages";
  if (c.startsWith("image/") || c.startsWith("video/") || c.startsWith("audio/") || /\.(png|jpe?g|gif|webp|svg|mp4|mov|webm|mp3|wav)$/.test(n)) return "media";
  return "files";
}

// Files: every file uploaded or delivered, as a view over the same files
// the conversations show.
const FILE_SVG_SHEET = '<svg viewBox="0 0 16 16" fill="none" class="ic" aria-hidden="true"><rect x="2.5" y="2" width="11" height="12" rx="1.5" fill="#10B981" fill-opacity="0.18" stroke="#10B981" stroke-width="1.2"/><path d="M5 6h6M5 8.5h6M5 11h4" stroke="#10B981" stroke-width="1.2" stroke-linecap="round"/></svg>';
const FILE_SVG_DOC = '<svg viewBox="0 0 16 16" fill="none" class="ic" aria-hidden="true"><rect x="2.5" y="2" width="11" height="12" rx="1.5" fill="#F43F5E" fill-opacity="0.18" stroke="#F43F5E" stroke-width="1.2"/><path d="M5.5 6.5h5M5.5 9.5h3.5" stroke="#F43F5E" stroke-width="1.2" stroke-linecap="round"/></svg>';
// A file's badge by its kind, in the Files view and an Aurora stage's card.
function fileBadge(f) {
  const n = (f.name || "").toLowerCase(), k = libFileKind(f);
  if (n.endsWith(".pdf") || f.media_type === "application/pdf") return `<span class="lib-badge pdf">${FILE_SVG_DOC}<span class="sr">PDF</span></span>`;
  if (/\.(csv|xlsx?|tsv)$/.test(n) || f.media_type === "text/csv") return `<span class="lib-badge sheet">${FILE_SVG_SHEET}<span class="sr">CSV</span></span>`;
  if (n.endsWith(".json") || f.media_type === "application/json") return `<span class="lib-badge json">${FILE_SVG_SHEET}<span class="sr">JSON</span></span>`;
  if (/\.(js|mjs|ts|tsx|py|rs|go|sh|html|css|patch|diff)$/.test(n)) return `<span class="lib-badge code">${FILE_SVG_SHEET}<span class="sr">CODE</span></span>`;
  if (k === "pages") return `<span class="lib-badge page">${FILE_SVG_DOC}<span class="sr">MD</span></span>`;
  if (k === "media") return `<span class="lib-badge media">${FILE_SVG_SHEET}<span class="sr">IMG</span></span>`;
  return `<span class="lib-badge">${FILE_SVG_DOC}<span class="sr">FILE</span></span>`;
}
async function filesView() {
  const { q, project, kind = "all" } = Panel.lib;
  const { files } = await Server.files.list({ q, project });
  const shown = kind === "all" ? files : files.filter(f => libFileKind(f) === kind);
  const DAY_MS = 86_400_000, weekCutoff = Date.now() - 7 * DAY_MS;
  const recent = shown.filter(f => { const t = Date.parse(f.at || ""); return !Number.isNaN(t) && t >= weekCutoff; });
  const older = shown.filter(f => { const t = Date.parse(f.at || ""); return Number.isNaN(t) || t < weekCutoff; });
  const rowHtml = f => `<li data-lib-file="${esc(f.id)}">${fileBadge(f)}<div class="rg-main">${fileChip(f)}<span class="small muted">${f.source === "delivered" ? "Delivered by Magnus" : "Uploaded by you"} · ${esc(when(f.at))} · ${fileFrom(f)}</span></div><span class="mono small muted lib-size">${esc(kb(f.size))}</span><div class="file-acts">${moveControl(f.id, f.project, App.projects, true)}<button type="button" class="icon-btn" data-file-download="${esc(f.id)}" data-name="${esc(f.name)}" title="Download ${esc(f.name)}" aria-label="Download ${esc(f.name)}">${icon("download")}</button>${trashButton(f.id, "Delete")}</div></li>`;
  const listHtml = !shown.length
    ? `<p class="empty">${q || project || kind !== "all" ? "No file matches." : "No files yet. Upload one, or ask Magnus for a document."}</p>`
    : recent.length && older.length
      ? `<div class="lib-group-head">This week</div><ul class="flist gbox">${recent.map(rowHtml).join("")}</ul><div class="lib-group-head">Older</div><ul class="flist gbox">${older.map(rowHtml).join("")}</ul>`
      : `<ul class="flist gbox">${shown.map(rowHtml).join("")}</ul>`;
  return plain(`<div class="companion-sec-head"><span>Files</span><span class="count">${shown.length}</span></div>
    <div class="row lib-bar"><label class="sr" for="lq">Search files</label><input id="lq" type="search" data-draft="lib-q" placeholder="Files, tasks, projects or text" autocomplete="off">
    <label class="sr" for="lp">Project</label><select id="lp" aria-label="Filter by project"><option value="">All files</option><option value="none"${project === "none" ? " selected" : ""}>In no project</option>${App.projects.map(p => `<option value="${esc(p.id)}"${p.id === project ? " selected" : ""}>${esc(p.name)}</option>`).join("")}</select>
    ${attachButton("Upload")}</div>
    <div class="lib-filters" role="group" aria-label="Filter files by type">${[["all", "All"], ["pages", "Pages"], ["media", "Media"], ["files", "Files"]].map(([k, l]) => `<button type="button" class="${kind === k ? "on" : ""}" data-lib-kind="${k}" aria-pressed="${kind === k}">${l}</button>`).join("")}</div>
    ${listHtml}`);
}

function projectsView() {
  return plain(`${App.projects.length ? `<ul class="plist">${App.projects.map(p => `<li><a href="#/project/${enc(p.id)}" data-open="project:${esc(p.id)}"><strong>${esc(p.name)}</strong><span class="small muted">${p.counts.tasks} tasks · ${p.counts.files} files · ${p.counts.schedules} schedules</span></a></li>`).join("")}</ul>` : `<p class="empty">No projects yet.</p>`}
    <form id="np" class="card flat"><h3>New project</h3><p class="small muted">A project groups conversations, tasks, schedules and files. Its instructions and files apply to everything in it.</p><label>Name <input id="pn" required></label>
    <label>Instructions for everything in it <span class="faint small">(optional)</span><textarea id="pi" rows="3" data-draft="new-project" placeholder="For example: Prices in euros. Keep every document to one page."></textarea></label>
    <div class="row"><button class="primary">Create</button></div></form>`);
}

async function projectView(id) {
  const p = await Server.projects.get(id);
  return plain(`<div class="spread"><h2 class="ptitle">${esc(p.name)}</h2>${trashButton(id, "Delete project")}</div>
    <details class="small" data-rename-project-wrap style="margin-bottom:8px"><summary class="muted">Rename project</summary><form class="row small" data-rename-project="${esc(id)}" style="margin-top:6px"><input name="name" value="${esc(p.name)}" required aria-label="Project name" style="flex:1"><button type="submit" class="primary small">Save</button></form></details>
    <div class="row"><a class="button primary" href="#/chat/new/${enc(id)}">New conversation in this project</a><button type="button" class="shared-promote-btn" data-project-share="${esc(id)}" data-project-name="${esc(p.name)}" title="Add people">${icon("team")}<span>Add people</span></button></div>
    <h3>Instructions</h3>
      <textarea id="pin" rows="4" data-draft="pin-${esc(id)}">${esc(p.instructions || "")}</textarea><div class="row"><button data-act="save-ins" data-id="${esc(id)}">Save instructions</button></div>
    <div class="spread"><h3>Files</h3>${attachButton("Upload to this project")}</div>
      ${p.files.length ? `<ul class="flist">${p.files.map(f => `<li data-project-file="${esc(f.id)}">${fileChip(f)} <span class="small faint">${f.source === "delivered" ? "delivered" : "uploaded"} ${esc(when(f.at))}</span></li>`).join("")}</ul>` : `<p class="empty">No files.</p>`}
    <h3>Conversations</h3>${p.threads.length ? `<ul class="tlist">${p.threads.map(t => `<li class="trow"><a href="#/chat/${enc(t.thread)}"><span class="ttl">${esc(t.title)}</span><span class="small faint when">${esc(when(t.updated_at))}</span></a></li>`).join("")}</ul>` : `<p class="empty">No conversations.</p>`}
    <h3>Tasks</h3>${p.tasks.length ? `<ul class="tlist">${p.tasks.map(taskRow).join("")}</ul>` : `<p class="empty">No tasks.</p>`}
    <h3>Schedules</h3>${p.schedules.length ? `<ul class="tlist">${p.schedules.map(x => `<li class="trow"><a href="#/schedules" data-open="schedule:${esc(x.id)}"><span class="ttl">${esc(x.title)}</span><span class="small faint when">Next ${esc(until(x.next))}</span></a></li>`).join("")}</ul>` : `<p class="empty">No schedules.</p>`}`);
}

// One search across conversations, tasks and files.
async function searchView(q) {
  const r = await Server.search(q);
  const none = !r.conversations.length && !r.tasks.length && !r.files.length;
  return plain(none ? `<p class="empty">Nothing matches “${esc(q)}”.</p>` : `
    ${r.conversations.length ? `<h3>Conversations</h3><ul class="tlist">${r.conversations.map(c => `<li class="trow found" data-found-thread="${esc(c.thread)}"><a href="#/chat/${enc(c.thread)}"><span class="ttl">${esc(c.title)}</span><span class="small faint when">${esc(when(c.updated_at))}</span></a>${c.snippet ? `<div class="small muted snippet">${esc(c.snippet)}</div>` : ""}</li>`).join("")}</ul>` : ""}
    ${r.tasks.length ? `<h3>Tasks</h3><ul class="tlist">${r.tasks.map(taskRow).join("")}</ul>` : ""}
    ${r.files.length ? `<h3>Files</h3><ul class="flist">${r.files.map(f => `<li data-lib-file="${esc(f.id)}">${fileChip(f)}<span class="small">${fileFrom(f)}</span></li>`).join("")}</ul>` : ""}`);
}

// What can be done in the panel, whichever view it shows.
function wirePanel() {
  const el = $("#detail");
  el.addEventListener("click", async e => {
    if (e.target.closest("[data-act=close-panel]")) return closePanel(true);
    if (e.target.closest("[data-dismiss-routine]")) { save("routine-hint-dismissed", true); return drawPanel(); }
    const fk = e.target.closest("[data-lib-kind]");
    if (fk) { Panel.lib.kind = fk.dataset.libKind; return drawPanel(); }
    const l = e.target.closest("[data-layer]");
    if (l) { Panel.layer = l.dataset.layer; return drawPanel(); }
    const m = e.target.closest("[data-more]");
    if (m) { MORE.add(m.dataset.more); return drawPanel(); }
    const tf = e.target.closest("[data-task-filter]");
    if (tf && typeof TaskFilter !== "undefined") { TaskFilter.state = tf.dataset.taskFilter || "all"; return drawPanel(); }
    if (e.target.closest("[data-clear-ended-tasks]")) {
      const ended = App.tasks.filter(x => ENDED.includes(x.status));
      if (!ended.length) return;
      if (!await ask(`Move ${ended.length} ended task${ended.length === 1 ? "" : "s"} to the trash? You can restore them from Trash.`, "Clear ended tasks")) return;
      try {
        await Promise.all(ended.map(x => Server.trash.add(x.task_id)));
        toast(`Cleared ${ended.length} ended task${ended.length === 1 ? "" : "s"}.`);
        await refresh();
      } catch (err) { toast(err.message); }
      return;
    }
    const a = e.target.closest("button[data-act]");
    if (!a) return;
    const id = a.dataset.id;
    try {
      switch (a.dataset.act) {
        case "speak": await readAloud(Panel.task.spoken,`task:${Panel.task.task_id}`); break;
        case "hush": await hush(); break;
        case "cancel":
          if (!await ask("Stop this task? What it already did stays done.", "Stop")) return;
          await Server.tasks.cancel(id); break;
        case "pause-task":
          await Server.tasks.pause(id);
          toast("Task paused. Resume it anytime.");
          break;
        case "resume": await Server.tasks.resume(id); break;
        case "copy-deliverable": {
          const text = Panel.task?.summary || $("#pview", el)?.contentDocument?.body?.innerText || "";
          if (text && navigator.clipboard?.writeText) { await navigator.clipboard.writeText(text); toast("Copied to clipboard."); }
          else toast("Nothing to copy yet.");
          return;
        }
        case "export-deliverable": {
          const text = Panel.task?.summary || $("#pview", el)?.contentDocument?.body?.innerText || "";
          if (!text) { toast("Nothing to export yet."); return; }
          const slug = String(Panel.task?.title || "deliverable").toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "") || "deliverable";
          const blob = new Blob([text], { type: "text/markdown;charset=utf-8" });
          const url = URL.createObjectURL(blob);
          const link = Object.assign(document.createElement("a"), { href: url, download: `${slug}.md` });
          document.body.append(link); link.click(); link.remove();
          setTimeout(() => URL.revokeObjectURL(url), 1000);
          toast(`Exported ${slug}.md.`);
          return;
        }
        case "fullscreen-deliverable": {
          const frame = $("#pview", el);
          if (frame?.requestFullscreen) frame.requestFullscreen().catch(() => {});
          return;
        }
        case "ask-file": {
          const fname = a.dataset.fileName || "this file";
          const fid = a.dataset.fileId || "";
          App.appSearch = `Look at ${fname}${fid ? ` (file ${fid})` : ""} and `;
          location.hash = "#/chat/new";
          return;
        }
        case "run-schedule": {
          a.disabled = true;
          const r = await Server.schedules.run(id);
          toast("Started a run now.");
          await refresh();
          if (r?.task_id) await openPanel(`task:${r.task_id}`);
          return;
        }
        case "dry-run-schedule": {
          a.disabled = true;
          const r = await Server.schedules.run(id, { dry_run: true });
          const nextText = r?.next_due ? `Next due: ${new Date(r.next_due).toLocaleString()}` : "Not scheduled";
          toast(`Dry run preview · ${r?.would_run ? "Would run" : "Paused"} · ${nextText}`, 6000);
          a.disabled = false;
          return;
        }
        case "stop-schedule":
          if (!await ask("Stop this schedule? No more of its runs start. Active runs stop too. Work already done stays done. The schedule stays here, ended, until you delete it.", "Stop schedule")) return;
          await Server.schedules.stop(id);
          toast("The schedule is stopped.");
          break;
        case "delete-schedule": {
          const sc = (App.schedules || []).find(x => x.id === id);
          const msg = sc?.ended
            ? "Delete this schedule? Its past runs stay in Tasks."
            : "Delete this schedule? No more runs will start and any active run will stop. Past runs stay in Tasks.";
          if (!await ask(msg, "Delete schedule")) return;
          await Server.schedules.remove(id);
          App.schedules = (App.schedules || []).filter(x => x.id !== id);
          toast("The schedule is deleted.");
          if (Panel.what === `schedule:${id}`) await openPanel("schedules");
          break;
        }
        case "pause-schedule": await Server.schedules.pause(id); toast("Schedule paused."); break;
        case "resume-schedule": await Server.schedules.resume(id); toast("Schedule resumed."); break;
        case "connect": connectApp(a.dataset.app).catch(err => toast(err.message)); return;
        case "connect-page": await Magnus.openExternal(a.dataset.url); return;
        case "disconnect":
          if (!await ask("Disconnect? Your sign-in is ended at the app, and Magnus can no longer use it for you until you connect again.", "Disconnect")) return;
          disconnected(await Server.connectors.disconnect(a.dataset.app)); break;
        case "save-ins":
          await Server.projects.update(id, { instructions: $("#pin", el).value }); delete drafts[`pin-${id}`];
          toast("Instructions saved. New tasks here use them."); break;
        default: return;
      }
      await refresh();
    } catch (err) { toast(err.message); }
  });
  // Batch approvals & staged draft actions inside the right sidebar panel.
  el.addEventListener("click", async e => {
    const batchBtn = e.target.closest("button[data-batch-decide]");
    if (batchBtn) {
      const ids = $$("input[data-batch-id]:checked", el).map(c => c.dataset.batchId).filter(Boolean);
      if (!ids.length) { toast("Select at least one approval first."); return; }
      const decision = batchBtn.dataset.batchDecide;
      batchBtn.disabled = true;
      try {
        const res = await Server.inbox.batch(ids, { decision, scope: decision === "approve" ? "once" : undefined });
        const okCount = (res?.results || []).filter(r => r.ok).length;
        toast(`${decision === "approve" ? "Allowed" : "Declined"} ${okCount} of ${ids.length} item${ids.length === 1 ? "" : "s"}.`);
        await refresh();
      } catch (err) { toast(err.message); batchBtn.disabled = false; }
      return;
    }
    const stagedBtn = e.target.closest("button[data-staged-act]");
    if (stagedBtn) {
      const stId = stagedBtn.dataset.id;
      const cardEl = stagedBtn.closest("[data-staged]");
      const rawAct = stagedBtn.dataset.stagedAct;
      const scope = cardEl?.querySelector("[data-staged-scope]")?.value || "once";
      const edited = {};
      if (cardEl) {
        for (const inp of $$("[data-staged-field]", cardEl)) {
          if (inp.value !== (inp.dataset.orig ?? "")) edited[inp.dataset.stagedField] = inp.value;
        }
      }
      const hasEdits = Object.keys(edited).length > 0;
      const outcome = rawAct === "sent" && hasEdits ? "edited" : rawAct;
      stagedBtn.disabled = true;
      try {
        const res = await Server.staged.resolve(stId, { outcome, scope, ...(hasEdits ? { edited_input: edited } : {}) });
        toast(rawAct === "discarded" ? "Draft discarded." : res?.executed ? "Draft sent." : "Draft resolved.");
        await refresh();
      } catch (err) { toast(err.message); stagedBtn.disabled = false; }
    }
  });
  let t;
  el.addEventListener("input", e => {
    if (e.target.id === "tasks-q" && typeof TaskFilter !== "undefined") {
      const inp = /** @type {HTMLInputElement} */ (e.target);
      const pos = [inp.selectionStart ?? inp.value.length, inp.selectionEnd ?? inp.value.length];
      TaskFilter.q = inp.value;
      clearTimeout(t);
      t = setTimeout(async () => {
        await drawPanel();
        const next = /** @type {?HTMLInputElement} */ ($("#tasks-q", el));
        if (next) { next.focus(); try { next.setSelectionRange(pos[0], pos[1]); } catch { /* ignore */ } }
      }, 150);
      return;
    }
    if (e.target.id !== "lq") return;
    clearTimeout(t);
    t = setTimeout(() => { Panel.lib.q = e.target.value.trim(); drawPanel(); }, 200);
  });
  el.addEventListener("change", async e => {
    if (e.target.matches("[data-batch-all]")) {
      const checked = e.target.checked;
      $$("input[data-batch-id]", el).forEach(c => { c.checked = checked; });
      return;
    }
    if (e.target.id === "lp") { Panel.lib.project = e.target.value; return drawPanel(); }
    if (!e.target.matches("[data-attach]")) return;
    const project = Panel.what.startsWith("project:") ? Panel.what.slice(8) : Panel.lib.project && Panel.lib.project !== "none" ? Panel.lib.project : null;
    try { const up = await upload(e.target.files, project ? { project } : {}); toast(`Uploaded ${up.map(f => f.name).join(", ")}.`); } catch (err) { toast(err.message); }
    e.target.value = "";
    refresh();
  });
  const schedulePayloadFromForm = form => {
    const fd = new FormData(form);
    const kind = String(fd.get("kind") || "daily");
    const body = { text: String(fd.get("text") || "").trim() };
    const at = String(fd.get("daily_at") || "08:00");
    if (kind === "daily") body.daily_at = at;
    else if (kind === "weekdays") body.weekdays_at = at;
    else if (kind === "weekly") {
      const days = fd.getAll("days").map(String).filter(Boolean);
      body.weekly_at = { days: days.length ? days : ["mon"], at };
    } else if (kind === "monthly") body.monthly_at = { day: Math.min(31, Math.max(1, Number(fd.get("month_day")) || 1)), at };
    else if (kind === "once") body.once_at = String(fd.get("once_at") || "");
    else body.every_minutes = Math.max(5, Number(fd.get("every_minutes")) || 60);
    const mr = String(fd.get("max_runs") || "").trim();
    if (mr) body.max_runs = Math.max(1, Number(mr) || 1);
    return body;
  };
  el.addEventListener("submit", async e => {
    const renameProj = e.target.closest("form[data-rename-project]");
    if (renameProj) {
      e.preventDefault();
      const pid = renameProj.dataset.renameProject;
      const name = String(new FormData(renameProj).get("name") || "").trim();
      if (!name) { toast("Enter a project name."); return; }
      try {
        await Server.projects.update(pid, { name });
        toast("Project renamed.");
        await refresh();
      } catch (err) { toast(err.message); }
      return;
    }
    const createSched = e.target.closest("form[data-create-schedule]");
    if (createSched) {
      e.preventDefault();
      const body = schedulePayloadFromForm(createSched);
      if (!body.text) { toast("Describe what this routine should do."); return; }
      try {
        await Server.schedules.create(body);
        toast("Routine created.");
        await refresh();
      } catch (err) { toast(err.message); }
      return;
    }
    const editSched = e.target.closest("form[data-edit-schedule]");
    if (editSched) {
      e.preventDefault();
      const sid = editSched.dataset.editSchedule;
      try {
        await Server.schedules.edit(sid, schedulePayloadFromForm(editSched));
        toast("Schedule updated.");
        await refresh();
      } catch (err) { toast(err.message); }
      return;
    }
    if (e.target.id !== "np") return;
    e.preventDefault();
    try {
      const r = await Server.projects.create({ name: $("#pn", el).value, instructions: $("#pi", el).value });
      delete drafts["new-project"];
      await refresh();
      openPanel(`project:${r.id}`);
    } catch (err) { toast(err.message); }
  });
}
