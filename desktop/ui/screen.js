// Who is driving a cloud computer, its screen pinned beside the conversation,
// and the agent asking its person to take over (spec, User takeover and
// handback: status, preview, takeover, agent-requested takeover).
const PINNED_EVERY_MS = 3000;

/** @typedef {{ run: string, task: string, status: string, reason: string|null, agent?: string, sub?: boolean, for?: string, for_name?: string }} DrivingRun */
/** @typedef {{ item: string, reason: string, why: string, run: string|null, task: string|null, at: string }} TakeoverAsk */
/** @typedef {{ by: "person"|"agent"|"no_one", person: string|null, runs: DrivingRun[], request: TakeoverAsk|null }} Driving */

/** What a takeover is for, as the request names it. */
const TAKEOVER_REASONS = { sso: "A single sign-on", passkey: "A passkey", captcha: "A CAPTCHA", other_device: "An approval on another device", security_key: "A hardware security key", password_change: "A password change", other: "A step only you can do" };

/** Set by "Take control" anywhere; the computer's screen takes over as it
 * opens. `pinned` is the screen pinned beside the conversation, if one is. */
const Screen = { takeOnOpen: null, pinned: null };

const taskName = id => { const t = App.tasks.find(x => x.task_id === id); return t ? (t.title || firstLine(t.text || "") || "a task") : "a task"; };

/** The agent using the computer, by name: Magnus, unless the server says otherwise. */
const agentName = c => (c.driving?.runs || []).find(r => r.agent)?.agent || "Magnus";

/** Whom the working runs are for: "you", or each person's name. */
const workingForWhom = c => [...new Set((c.driving?.runs || []).filter(r => r.status === "running").map(r => r.for && r.for === App.me?.id ? "you" : r.for_name || r.for).filter(Boolean))];
const andList = xs => xs.length > 1 ? `${xs.slice(0, -1).join(", ")} and ${xs.at(-1)}` : xs[0] || "";

/**
 * For whom and on what the agent is working, in words: "for you on Book the
 * flight". "" when no run of it is working.
 * @param {{ driving?: Driving }} c
 */
function workingFor(c) {
  const runs = (c.driving?.runs || []).filter(r => r.status === "running");
  if (!runs.length) return "";
  const whom = workingForWhom(c);
  const sub = runs.some(r => r.sub) ? " (with a sub-agent)" : "";
  return `${whom.length ? `for ${andList(whom)} ` : ""}on ${andList([...new Set(runs.map(r => taskName(r.task)))])}${sub}`;
}

/**
 * Who is driving, in words, with the tasks using it: the list of computers.
 * @param {{ status: string, controller: string|null, controller_name?: string, driving?: Driving }} c
 */
function drivingLine(c) {
  const d = c.driving;
  if (c.status !== "running") return "";
  if (!d || d.by === "no_one") return `<span class="chip" data-driving="no_one">No one is using it</span>`;
  if (d.by === "person") return `<span class="chip warn" data-driving="person">${d.person === App.me?.id ? "You have" : `${esc(c.controller_name || d.person)} has`} control. ${esc(agentName(c))} is paused.</span>`;
  const tasks = d.runs.filter(r => r.status === "running").map(r => `<a href="#/task/${enc(r.task)}" data-open="task:${esc(r.task)}">${esc(taskName(r.task))}</a>`);
  const whom = workingForWhom(c);
  return `<span class="chip good" data-driving="agent">${esc(agentName(c))} is using it</span> <span class="small">${whom.length ? `for ${esc(andList(whom))} ` : ""}on ${tasks.join(", ")}</span>`;
}

/** The agent's open request, with what the person can do about it. */
function takeoverAsk(c) {
  const r = c.driving?.request;
  if (!r) return "";
  const item = App.inbox.find(i => i.id === r.item);
  return `<div class="banner" role="status" data-takeover-request="${esc(r.item)}"><strong>${esc(agentName(c))} asks you to take over.</strong> ${esc(TAKEOVER_REASONS[r.reason] || r.reason)}${r.task ? ` for <a href="#/task/${enc(r.task)}" data-open="task:${esc(r.task)}">${esc(taskName(r.task))}</a>` : ""}.
    <div class="small" style="margin:6px 0">${esc(r.why)}</div>
    <div class="row">${c.controller ? "" : `<button class="primary" data-take-over="${esc(c.id)}">Take control</button>`}${item?.can_respond ? `<button data-item="${esc(item.id)}" data-decision="done">I did it another way</button><button data-item="${esc(item.id)}" data-decision="decline">Decline</button>` : ""}</div></div>`;
}

/** The request's card in the inbox and the conversation. */
function takeoverRequestCard(i) {
  const d = i.details || {};
  const what = `<div class="what"><div class="field"><span>For</span><span>${esc(TAKEOVER_REASONS[d.reason] || d.reason)}</span></div><div class="field"><span>What to do</span>${fieldValue(d.why)}</div></div>`;
  if (!i.can_respond) return `${what}<p class="small faint">Waiting for its person.</p>`;
  return `${what}<div class="row" style="margin-top:12px"><button class="primary" data-take-over="${esc(d.computer)}">Take control</button><button data-open="computer:${esc(d.computer)}">Show the screen</button>
    <button data-item="${esc(i.id)}" data-decision="done">I did it another way</button><button data-item="${esc(i.id)}" data-decision="decline">Decline</button></div>`;
}

/** How far a wake has got, in words and as a share of the way. */
const WAKE_WORDS = { starting: "Waking: starting it", booting: "Waking: starting its desktop", restoring: "Waking: restoring your apps and files" };
const WAKE_SHARE = { starting: 20, booting: 50, restoring: 80 };

/**
 * The computer's state as its panel shows it: one pill, and one sentence that
 * always says who holds control and whether the agent is paused (spec rev
 * 131, User takeover and handback: Preview).
 * @param {{ status: string, controller: string|null, controller_name?: string, waking?: object|null, driving?: Driving }} c
 * @param {boolean} holding this view holds the stream
 * @returns {{ who: "person"|"agent"|"no_one", mine: boolean, key: string, pill: string, text: string }}
 */
function controlWords(c, holding) {
  const agent = agentName(c), mine = holding || (!!c.controller && c.controller === App.me?.id);
  const free = (key, pill) => ({ who: "no_one", mine, key, pill, text: `No one has control. ${agent} is not paused.` });
  if (mine) return { who: "person", mine, key: "mine", pill: "You are in control", text: `You have control. ${agent} is paused.` };
  if (c.controller) { const name = c.controller_name || c.controller; return { who: "person", mine, key: "held", pill: `${name} is in control`, text: `${name} has control. ${agent} is paused.` }; }
  if (c.status === "paused") return { who: "no_one", mine, key: "paused", pill: "Paused", text: `No one has control. ${agent} is paused by the kill switch.` };
  if (c.driving?.request) return { who: "no_one", mine, key: "asks", pill: `${agent} asks you to take over`, text: `No one has control. ${agent} is paused: it asks you to take over.` };
  if (c.status === "queued") return free("queued", "Waiting for room");
  if (c.waking) return free("waking", "Waking");
  if (c.status !== "running") return free("asleep", "Asleep");
  if (c.driving?.by === "agent") return { who: "agent", mine, key: "working", pill: `${agent} is working`, text: `${agent} has control. It is working ${workingFor(c) || "on a task"}.` };
  return free("ready", "Ready");
}

/**
 * What the bar along the bottom of the screen says. It is always there, as in
 * the mockup: what is on the screen, and what a click on it does.
 * @param {{ status: string, controller: string|null, controller_name?: string, waking?: { phase: string }|null, queue_position?: number, driving?: Driving }} c
 * @param {{ waking?: { phase: string }|null, at?: string|null }|null} shot
 * @param {boolean} [holding] this view holds the stream
 */
function screenCaption(c, shot, holding = false) {
  const waking = shot?.waking || c.waking, agent = agentName(c), r = c.driving?.request;
  if (c.status === "paused") return "Paused by the kill switch. Resume it to use it again.";
  if (holding || (c.controller && c.controller === App.me?.id)) return `Your mouse and keyboard reach the computer. ${agent} sees nothing until you hand back.`;
  if (c.status === "queued") return `Waiting for room on the server${c.queue_position ? `, number ${c.queue_position} in line` : ""}. It wakes as soon as there is room.`;
  if (waking) return WAKE_WORDS[waking.phase] || "Waking";
  if (c.status !== "running") return `Asleep. It wakes when ${agent} needs it. Click to wake it now.${shot?.at ? ` Last seen ${when(shot.at)}.` : ""}`;
  if (c.controller) return `${c.controller_name || c.controller} is using it. Click the screen to take control.`;
  if (r) return `${agent} asks you to take over: ${(TAKEOVER_REASONS[r.reason] || r.reason).toLowerCase()}. Click the screen to take control.`;
  if (c.driving?.by === "agent") return `Working ${workingFor(c) || "on a task"} · click the screen to take control`;
  return "No one is using it. Click the screen to take control.";
}

/**
 * What covers a screen that is not live: asleep, waking or waiting for room.
 * "" when it is awake.
 * @param {{ status: string, waking?: { phase: string }|null }} c
 * @param {{ waking?: { phase: string }|null }|null} shot
 */
function screenVeil(c, shot) {
  const waking = shot?.waking || c.waking;
  if (c.status === "paused") return "";
  if (c.status === "queued" || waking) {
    const queued = c.status === "queued", share = queued ? 5 : WAKE_SHARE[waking.phase] || 10;
    const words = queued ? "It wakes as soon as there is room on the server." : (WAKE_WORDS[waking.phase] || "Waking").replace(/^Waking: (.)/, (_, ch) => ch.toUpperCase());
    return `<strong>${queued ? "Waiting for room" : "Waking"}</strong><div class="wake-bar" role="progressbar" aria-valuemin="0" aria-valuemax="100" aria-valuenow="${share}"><div style="width:${share}%"></div></div><span>${esc(words)}</span>`;
  }
  if (c.status !== "running") return `<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M21 12.8A9 9 0 1 1 11.2 3a7 7 0 0 0 9.8 9.8z"/></svg><strong>Asleep</strong><span>It wakes when ${esc(agentName(c))} needs it. Click to wake it now.</span>`;
  return "";
}

const FULL_SCREEN_ICON = '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 9V4h5M20 9V4h-5M4 15v5h5M20 15v5h-5"/></svg>';

/** The strip above the screen, the same on the computer's page and pinned. */
function controlStrip(c, holding) {
  const w = controlWords(c, holding);
  const act = w.mine ? `<button class="primary" data-done>I'm done</button>` : c.status === "paused" ? "" : `<button class="primary" data-take>Take control</button>`;
  return `<span class="state-pill" data-pill="${w.key}" data-control="${w.who}">${esc(w.pill)}</span><span class="small control-words">${esc(w.text)}</span><span class="grow"></span>${act}<button class="icon" data-full title="Full screen" aria-label="Full screen">${FULL_SCREEN_ICON}</button>`;
}

/**
 * The person's own computer in a conversation's head: what it is doing, and
 * one click pins its screen beside the conversation (spec: Status; Preview).
 * "" on a server without computers, or before it has made theirs.
 * @param {Array<object>|null|undefined} computers as GET /v1/computers lists them
 */
function computerChip(computers) {
  const c = (computers || []).find(x => x.owner?.person === App.me?.id);
  if (!c) return "";
  const w = controlWords(c, false), agent = agentName(c);
  const words = { working: `${agent} is using your computer`, mine: "You are in control of your computer", held: `${w.pill} of your computer`, asks: w.pill, paused: "Your computer is paused", queued: "Your computer is waiting for room", waking: "Your computer is waking", asleep: "Your computer is asleep" }[w.key] || "Your computer";
  return `<button type="button" class="computer-chip" data-pill="${w.key}" data-open="computer:${esc(c.id)}" title="Show its screen beside the conversation"><svg viewBox="0 0 24 24" aria-hidden="true">${UI.computer}</svg>${esc(words)}</button>`;
}

// The screen pinned beside the conversation: the same frame and strip as
// the computer's page, so a person takes control here without leaving the
// conversation. Its markup never changes with the computer's state, so a
// redraw of the panel never cuts a takeover off.
PANEL_TITLES.computer = "Computer";
PANELS.computer = async id => {
  const c = await api("GET", `/v1/computers/${enc(id)}`);
  if (c.status !== "paused") api("POST", `/v1/computers/${enc(id)}/wake`, {}).catch(() => { /* The screen says how it stands. */ });
  const body = `<p class="small muted">${esc(c.name || "Computer")}</p><div data-pinned="${esc(id)}" class="pinned-screen"></div><div class="row"><a class="small" href="#/computer/${enc(id)}/screen">Open the computer</a></div>`;
  const after = el => {
    const host = $(`[data-pinned="${CSS.escape(id)}"]`, el);
    if (!host) return;
    Screen.pinned = computerScreen(host, c, `/v1/computers/${enc(id)}`, () => Panel.what === `computer:${id}` && host.isConnected, { ask: true });
  };
  return { body, after };
};

// "Take control" from a card or a banner: the computer's own page takes it
// in place; anywhere else the pinned screen opens beside the conversation
// and takes it there.
document.addEventListener("click", e => {
  const b = e.target.closest("[data-take-over]");
  if (!b) return;
  e.preventDefault();
  const id = b.dataset.takeOver;
  Screen.takeOnOpen = id;
  if (location.hash.startsWith(`#/computer/${enc(id)}`)) { showComputer($("#main"), id, "screen"); return; }
  const pinned = Screen.pinned;
  if (pinned?.c.id === id && pinned.alive()) { Screen.takeOnOpen = null; pinned.take(); return; }
  openPanel(`computer:${id}`, { history: true, focus: true, opener: b });
});
