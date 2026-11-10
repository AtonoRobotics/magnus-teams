// Who is driving a cloud computer, its screen pinned beside the conversation,
// and the agent asking its person to take over (spec, User takeover and
// handback: status, preview, takeover, agent-requested takeover).
const PINNED_EVERY_MS = 3000;

/** @typedef {{ run: string, task: string, status: string, reason: string|null }} DrivingRun */
/** @typedef {{ item: string, reason: string, why: string, run: string|null, task: string|null, at: string }} TakeoverAsk */
/** @typedef {{ by: "person"|"agent"|"no_one", person: string|null, runs: DrivingRun[], request: TakeoverAsk|null }} Driving */

/** What a takeover is for, as the request names it. */
const TAKEOVER_REASONS = { sso: "A single sign-on", passkey: "A passkey", captcha: "A CAPTCHA", other_device: "An approval on another device", security_key: "A hardware security key", password_change: "A password change", other: "A step only you can do" };

/** Set by "Take control" anywhere; the computer's screen takes over as it
 * opens. `pinned` is the screen pinned beside the conversation, if one is. */
const Screen = { takeOnOpen: null, pinned: null };

const taskName = id => { const t = App.tasks.find(x => x.task_id === id); return t ? (t.title || firstLine(t.text || "") || "a task") : "a task"; };

/**
 * Who is driving, in words, with the tasks using it.
 * @param {{ status: string, controller: string|null, driving?: Driving }} c
 */
function drivingLine(c) {
  const d = c.driving;
  if (c.status !== "running") return "";
  if (!d || d.by === "no_one") return `<span class="chip" data-driving="no_one">No one is using it</span>`;
  if (d.by === "person") return `<span class="chip warn" data-driving="person">${d.person === App.me?.id ? "You have" : `${esc(d.person)} has`} control. The agent is paused.</span>`;
  const tasks = d.runs.filter(r => r.status === "running").map(r => `<a href="#/task/${enc(r.task)}" data-open="task:${esc(r.task)}">${esc(taskName(r.task))}</a>`);
  return `<span class="chip good" data-driving="agent">The agent is using it</span> <span class="small">for ${tasks.join(", ")}</span>`;
}

/** The agent's open request, with what the person can do about it. */
function takeoverAsk(c) {
  const r = c.driving?.request;
  if (!r) return "";
  const item = App.inbox.find(i => i.id === r.item);
  return `<div class="banner" role="status" data-takeover-request="${esc(r.item)}"><strong>The agent asks you to take over.</strong> ${esc(TAKEOVER_REASONS[r.reason] || r.reason)}${r.task ? ` for <a href="#/task/${enc(r.task)}" data-open="task:${esc(r.task)}">${esc(taskName(r.task))}</a>` : ""}.
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

/** How far a wake has got, in words. */
const WAKE_WORDS = { starting: "Waking: starting it", booting: "Waking: starting its desktop", restoring: "Waking: restoring your apps and files" };

/**
 * What the screen says over its last screenshot, or "" while it is live.
 * @param {{ status: string, waking?: { phase: string }|null, queue_position?: number }} c
 * @param {{ waking?: { phase: string }|null, at?: string|null }|null} shot
 */
function screenCaption(c, shot) {
  const waking = shot?.waking || c.waking;
  if (c.status === "paused") return "Paused by the kill switch. Resume it to use it again.";
  if (c.status === "queued") return `Waiting for room on the server${c.queue_position ? `, number ${c.queue_position} in line` : ""}. It wakes as soon as there is room.`;
  if (waking) return WAKE_WORDS[waking.phase] || "Waking";
  if (c.status !== "running") return `Asleep. It wakes when the agent or you use it.${shot?.at ? ` Last seen ${when(shot.at)}.` : ""}`;
  return "";
}

/**
 * Who holds control and whether the agent is paused, in one sentence.
 * @param {{ status: string, controller: string|null, driving?: Driving }} c
 * @param {boolean} holding this view holds the stream
 */
function controlWords(c, holding) {
  const mine = holding || (!!c.controller && c.controller === App.me?.id);
  if (mine) return { who: "person", mine, text: "You have control. The agent is paused." };
  if (c.controller) return { who: "person", mine, text: `${c.controller} has control. The agent is paused.` };
  if (c.status === "paused") return { who: "no_one", mine, text: "No one has control. The agent is paused by the kill switch." };
  if (c.driving?.request) return { who: "no_one", mine, text: "No one has control. The agent is paused: it asks you to take over." };
  if (c.status === "running" && c.driving?.by === "agent") return { who: "agent", mine, text: "The agent has control. It is working." };
  return { who: "no_one", mine, text: "No one has control. The agent is not paused." };
}

/** The strip above the screen, the same on the computer's page and pinned. */
function controlStrip(c, holding) {
  const w = controlWords(c, holding);
  const act = w.mine ? `<button class="primary" data-done>I'm done</button>` : c.status === "paused" ? "" : `<button class="primary" data-take>Take control</button>`;
  return `<span class="chip ${w.who === "person" ? "warn" : w.who === "agent" ? "good" : ""}" data-control="${w.who}">${esc(w.text)}</span><span class="grow"></span>${act}<button data-full title="Full screen" aria-label="Full screen">Full screen</button>`;
}

// The screen pinned beside the conversation: the same frame and strip as
// the computer's page, so a person takes control here without leaving the
// conversation. Its markup never changes with the computer's state, so a
// redraw of the panel never cuts a takeover off.
PANEL_TITLES.computer = "Computer";
PANELS.computer = async id => {
  const c = await api("GET", `/v1/computers/${enc(id)}`);
  if (c.status !== "paused") api("POST", `/v1/computers/${enc(id)}/wake`, {}).catch(() => { /* The screen says how it stands. */ });
  const body = `<div data-pinned="${esc(id)}" class="pinned-screen"></div><div class="row"><a class="small" href="#/computer/${enc(id)}/screen">Open the computer</a></div>`;
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
