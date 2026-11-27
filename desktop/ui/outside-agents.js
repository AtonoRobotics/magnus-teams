// Outside agents, in Apps beside the apps (owner's collaboration design,
// Outside agents; decisions/2026-11-23-outside-agents-over-a2a.md): another
// organisation's agent, connected by its agent card's address the way an
// app is, with the credential its card asks for. Nothing it says is an
// instruction. It asks nobody until it is let ask you, or a shared project
// as a guest. Its requests and the replies waiting to be released come to
// the inbox (`outsideAgentCard`).
"use strict";

/** @typedef {{ person: string } | { project: string }} OutsideTarget */
/** @typedef {{ id: string, name: string, description: string, card_url: string, endpoint: string, scheme: "none"|"api_key"|"bearer"|"oauth2"|"mutual_tls", skills: string[], admitted: OutsideTarget[] }} OutsideAgentView */

const OutsideAgents = { list: null, key: null };
const OUTSIDE_SCHEMES = { none: "no sign-in", api_key: "a key", bearer: "a token", oauth2: "OAuth 2", mutual_tls: "a client certificate" };

/** Who an agent may ask, in words. @param {OutsideTarget[]} admitted @param {?string} me */
function outsideAdmitted(admitted, me) {
  if (!admitted.length) return "It can ask no one here yet.";
  const said = admitted.map(t => ("person" in t ? (t.person === me ? "you" : t.person) : `the shared project ${t.project}`));
  return `It can ask ${said.join(" and ")}.`;
}

/** One connected agent. @param {OutsideAgentView} a @param {?string} me */
function outsideAgentRow(a, me) {
  const id = esc(a.id);
  const mine = a.admitted.some(t => "person" in t && t.person === me);
  return `<article class="lcard" data-outside-agent="${id}"><div class="lc-head"><div class="lc-title-wrap"><h3>${esc(a.name)}</h3>
    <p class="lc-note">${esc(a.endpoint)} · signs in with ${esc(OUTSIDE_SCHEMES[a.scheme] || a.scheme)}</p></div></div><div class="lc-main">
    ${a.description ? `<p class="lc-sum">${esc(a.description)}</p>` : ""}<p class="small muted">${esc(outsideAdmitted(a.admitted, me))}</p>
    <div class="row">${mine ? "" : `<button data-act="outside-admit" data-agent="${id}">Let it ask me</button>`}<button data-act="outside-disconnect" data-agent="${id}">Disconnect</button></div></div></article>`;
}

/**
 * The Apps page's outside agents: the form to connect one, and those connected.
 * @param {?OutsideAgentView[]} list @param {?string} me @param {?string} key the key just issued, shown once
 */
function outsideAgentsSection(list, me, key) {
  if (!list) return "";
  const issued = key
    ? `<div class="row small" data-outside-key style="margin-top:8px;flex-wrap:wrap;gap:6px;align-items:center"><span>Give its operator this key, so it can ask you here. It is shown once: <code>${esc(key)}</code></span><button type="button" class="small" data-act="outside-copy-key">Copy key</button><button type="button" class="small" data-act="outside-clear-key">Clear</button></div>`
    : "";
  return `<section class="lsect" data-section="outside-agents"><div class="lsect-head"><div><h2>Outside agents</h2>
    <p>Agents run by others, a supplier's or a customer's. Magnus can send them tasks, and what they answer is read as outside content, never as instructions.</p></div></div>
    <form data-outside-connect class="row"><label class="sr" for="oa-card">Agent card address</label><input id="oa-card" name="card_url" type="url" placeholder="https://agent.example/.well-known/agent-card.json" required>
    <label class="sr" for="oa-cred">Key, token or client certificate</label><input id="oa-cred" name="credential" type="password" placeholder="The key its operator gave you" autocomplete="off">
    <button class="primary" type="submit">Connect</button></form>${issued}
    ${list.length ? `<div class="lgrid">${list.map(a => outsideAgentRow(a, me)).join("")}</div>` : `<p class="empty">No outside agents connected.</p>`}</section>`;
}

/** An outside agent's request, or the reply to one waiting to be released: the card's body. @param {any} i */
function outsideAgentCard(i) {
  const d = i.details || {};
  const words = `<div class="msg"><div class="small faint">${esc(d.agent_name)} · outside agent</div><div class="pre">${esc(d.text)}</div></div>
    <p class="small muted">What it says is outside content: Magnus weighs it, and never takes it as instructions.</p>`;
  if (i.kind === "outside_reply") {
    return `${words}<div class="what"><div class="field"><span>Reply</span><span class="pre">${esc(d.draft)}</span></div>
      <div class="field"><span>Waiting because</span><span>${esc(d.held_because)}</span></div></div>
      ${buttons(i, [["release", "Send reply", "primary"], ["release_always", "Send, and later replies to this request too"], ["discard", "Don't reply"]])}`;
  }
  return `${words}${buttons(i, [["take_up", "Work on it", "primary"], ["dismiss", "Dismiss"]])}`;
}

async function loadOutsideAgents() {
  const got = await Server.outsideAgents.list().catch(() => null);
  OutsideAgents.list = got ? got.agents : null;
}

/** Connect, let ask, disconnect. @param {HTMLElement} target */
async function outsideAgentAct(target) {
  const act = target.dataset.act;
  if (act === "outside-copy-key") {
    if (!OutsideAgents.key) return;
    try { await navigator.clipboard?.writeText?.(OutsideAgents.key); toast("Key copied."); }
    catch { toast("Copy failed — select the key manually."); }
    return;
  }
  if (act === "outside-clear-key") {
    OutsideAgents.key = null;
    return;
  }
  const id = target.dataset.agent;
  if (act === "outside-admit") await Server.outsideAgents.admit(id, { person: App.me?.id });
  if (act === "outside-disconnect") {
    if (!(await ask("Disconnect this agent? Its open tasks, both ways, are cancelled.", "Disconnect"))) return;
    await Server.outsideAgents.disconnect(id);
  }
}

/** Draw the outside agents into the Apps page, and keep them current. @param {HTMLElement} root */
async function mountOutsideAgents(root) {
  const box = root.querySelector("#lib-outside-agents");
  if (!box) return;
  const paint = () => { box.innerHTML = outsideAgentsSection(OutsideAgents.list, App.me?.id, OutsideAgents.key); };
  if (typeof onLeave === "function") onLeave(() => { OutsideAgents.key = null; });
  await loadOutsideAgents();
  paint();
  if (box.dataset.bound) return;
  box.dataset.bound = "1";
  box.addEventListener("submit", async e => {
    const form = e.target.closest("[data-outside-connect]");
    if (!form) return;
    e.preventDefault();
    const f = new FormData(form);
    try {
      const got = await Server.outsideAgents.connect({ card_url: String(f.get("card_url") || ""), credential: String(f.get("credential") || "") || undefined });
      OutsideAgents.key = got.inbound_key;
    } catch (err) { toast(err.message); }
    await loadOutsideAgents();
    paint();
  });
  box.addEventListener("click", async e => {
    const a = e.target.closest("[data-act^='outside-']");
    if (!a) return;
    e.preventDefault();
    try { await outsideAgentAct(a); } catch (err) { toast(err.message); }
    if (a.dataset.act !== "outside-copy-key" && a.dataset.act !== "outside-clear-key") await loadOutsideAgents();
    paint();
  });
}
