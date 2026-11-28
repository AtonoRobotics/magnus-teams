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

/** How far a wake has got: each phase in words, and its place among them. */
const WAKE_PHASES = ["starting", "booting", "restoring"];
const WAKE_WORDS = { starting: "Starting it", booting: "Starting its desktop", restoring: "Restoring your apps and files" };

/**
 * The computer's state as its strip shows it, once: the pill says who holds
 * control and whether the agent is paused (spec rev 131, User takeover and
 * handback: Preview), and `text` adds only what the pill cannot hold, such
 * as for whom and on what the agent works.
 * @param {{ status: string, controller: string|null, controller_name?: string, waking?: object|null, queue_position?: number, driving?: Driving }} c
 * @param {boolean} holding this view holds the stream
 * @returns {{ who: "person"|"agent"|"no_one", mine: boolean, key: string, pill: string, text: string }}
 */
function controlWords(c, holding) {
  const agent = agentName(c), mine = holding || (!!c.controller && c.controller === App.me?.id);
  const free = (key, pill, text = "") => ({ who: "no_one", mine, key, pill, text });
  if (mine) return { who: "person", mine, key: "mine", pill: `You are in control · ${agent} paused`, text: "" };
  if (c.controller) return { who: "person", mine, key: "held", pill: `${c.controller_name || c.controller} is in control · ${agent} paused`, text: "" };
  if (c.status === "paused") return free("paused", `Paused by the kill switch`);
  if (c.driving?.request) return free("asks", `${agent} asks you to take over`, (TAKEOVER_REASONS[c.driving.request.reason] || c.driving.request.reason).toLowerCase());
  if (c.status === "queued") return free("queued", "Waiting for room", c.queue_position ? `number ${c.queue_position} in line` : "");
  if (c.waking) return free("waking", "Waking");
  if (c.status !== "running") return free("asleep", "Asleep");
  if (c.driving?.by === "agent") return { who: "agent", mine, key: "working", pill: `${agent} is working`, text: workingFor(c) };
  return free("ready", "Ready");
}

/**
 * What covers a screen that is not live: asleep, waking or waiting for room,
 * with the wake's real progress. "" when it is awake.
 * @param {{ status: string, waking?: { phase: string }|null, queue_position?: number }} c
 * @param {{ waking?: { phase: string }|null, at?: string|null }|null} shot
 */
function screenVeil(c, shot) {
  const waking = shot?.waking || c.waking;
  if (c.status === "paused") return "";
  if (c.status === "queued") return `<strong>Waiting for room</strong><span>It wakes as soon as there is room on the server${c.queue_position ? `, number ${esc(c.queue_position)} in line` : ""}.</span>`;
  if (waking) {
    const at = WAKE_PHASES.indexOf(waking.phase);
    return `<strong>Waking</strong><span>${esc(WAKE_WORDS[waking.phase] || "Starting it")}${at >= 0 ? ` · step ${at + 1} of ${WAKE_PHASES.length}` : ""}</span>`;
  }
  if (c.status !== "running") return `<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M21 12.8A9 9 0 1 1 11.2 3a7 7 0 0 0 9.8 9.8z"/></svg><strong>Asleep</strong><span>Click to wake it.${shot?.at ? ` Last seen ${esc(when(shot.at))}.` : ""}</span>`;
  return "";
}

const FULL_SCREEN_ICON = '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 9V4h5M20 9V4h-5M4 15v5h5M20 15v5h-5"/></svg>';

/** The strip above the screen, the same on the computer's page and pinned. */
function controlStrip(c, holding, recording = false) {
  const w = controlWords(c, holding);
  const paste = w.mine ? `<button type="button" data-paste-clipboard title="Paste your clipboard into this computer">Paste</button>` : "";
  const rec = w.mine ? `<button type="button" class="${recording ? "primary" : ""}" data-record-steps>${recording ? "Stop &amp; teach Magnus" : "Record steps"}</button>` : "";
  const act = w.mine ? `${paste}${rec}<button class="primary" data-done>I'm done</button>` : c.status === "paused" ? "" : `<button class="primary" data-take>Take control</button>`;
  return `<span class="state-pill" data-pill="${w.key}" data-control="${w.who}">${esc(w.pill)}</span>${w.text ? `<span class="small control-detail">${esc(w.text)}</span>` : ""}<span class="grow"></span>${act}<button class="icon" data-full title="Full screen" aria-label="Full screen">${FULL_SCREEN_ICON}</button>`;
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
  const words = { working: `${agent} is using your computer`, mine: "You are in control of your computer", held: `${c.controller_name || c.controller} is in control of your computer`, asks: w.pill, paused: "Your computer is paused", queued: "Your computer is waiting for room", waking: "Your computer is waking", asleep: "Your computer is asleep" }[w.key] || "Your computer";
  return `<button type="button" class="computer-chip" data-pill="${w.key}" data-open="computer:${esc(c.id)}" title="Show its screen beside the conversation"><svg viewBox="0 0 24 24" aria-hidden="true">${UI.computer}</svg><span class="computer-chip-words">${esc(words)}</span></button>`;
}

// The screen pinned beside the conversation: the same frame and strip as
// the computer's page, so a person takes control here without leaving the
// conversation. Its markup never changes with the computer's state, so a
// redraw of the panel never cuts a takeover off.
PANEL_TITLES.computer = "Computer";
PANELS.computer = async id => {
  const c = await Server.computers.get(id);
  if (c.status !== "paused") Server.computers.wake(id).catch(() => { /* The screen says how it stands. */ });
  const body = `<p class="small muted">${esc(c.name || "Computer")}</p><div data-pinned="${esc(id)}" class="pinned-screen"></div><div class="row"><a class="small" href="#/computer/${enc(id)}/screen">Open the computer</a></div>`;
  const after = el => {
    const host = $(`[data-pinned="${CSS.escape(id)}"]`, el);
    if (!host) return;
    Screen.pinned = computerScreen(host, c, () => Panel.what === `computer:${id}` && host.isConnected, { ask: true });
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
  if (location.hash.startsWith("#/team/")) { location.hash = `#/computer/${enc(id)}/screen`; return; }
  const pinned = Screen.pinned;
  if (pinned?.c.id === id && pinned.alive()) { Screen.takeOnOpen = null; pinned.take(); return; }
  openPanel(`computer:${id}`, { history: true, focus: true, opener: b });
});
