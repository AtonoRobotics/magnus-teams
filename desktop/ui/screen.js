// Who is driving a cloud computer, its screen pinned beside the conversation,
// and the agent asking its person to take over (spec, User takeover and
// handback: status, preview, takeover, agent-requested takeover).
const PINNED_EVERY_MS = 3000;

/** @typedef {{ run: string, task: string, status: string, reason: string|null }} DrivingRun */
/** @typedef {{ item: string, reason: string, why: string, run: string|null, task: string|null, at: string }} TakeoverAsk */
/** @typedef {{ by: "person"|"agent"|"no_one", person: string|null, runs: DrivingRun[], request: TakeoverAsk|null }} Driving */

/** What a takeover is for, as the request names it. */
const TAKEOVER_REASONS = { sso: "A single sign-on", passkey: "A passkey", captcha: "A CAPTCHA", other_device: "An approval on another device", security_key: "A hardware security key", password_change: "A password change", other: "A step only you can do" };

/** Set by "Take control" anywhere; the computer's screen takes over as it opens. */
const Screen = { takeOnOpen: null, last: {}, looking: null };

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

// The screen pinned beside the conversation: it streams while it is open.
PANEL_TITLES.computer = "Computer";
PANELS.computer = async id => {
  const c = await api("GET", `/v1/computers/${enc(id)}`);
  const running = c.status === "running";
  const body = `<div class="row" data-pinned="${esc(id)}">${drivingLine(c) || `<span class="chip">${esc(c.status)}</span>`}</div>${takeoverAsk(c)}
    ${running ? `<div class="pinned-screen"><img data-pinned-screen alt="The computer's screen"${Screen.last[id] ? ` src="${Screen.last[id]}"` : ""}></div>` : `<p class="empty">Start this computer to see its screen.</p>`}
    <div class="row">${running && !c.controller ? `<button class="primary" data-take-over="${esc(id)}">Take control</button>` : ""}<a class="small" href="#/computer/${enc(id)}/screen">Open full screen</a></div>`;
  return { body, after: () => running && pinnedLook(id) };
};

/** One look loop per pinned computer, while it is pinned. */
async function pinnedLook(id) {
  if (Screen.looking === id) return;
  Screen.looking = id;
  while (Panel.what === `computer:${id}`) {
    try {
      const { image_b64 } = await api("GET", `/v1/computers/${enc(id)}/screen`);
      Screen.last[id] = `data:image/png;base64,${image_b64}`;
      const img = $("[data-pinned-screen]");
      if (img && Panel.what === `computer:${id}`) img.src = Screen.last[id];
    } catch { /* The next look may work. */ }
    await new Promise(r => setTimeout(r, PINNED_EVERY_MS));
  }
  Screen.looking = null;
}

// "Take control" from a card, a banner or the pinned screen: the computer
// opens full size and the person takes it.
document.addEventListener("click", e => {
  const b = e.target.closest("[data-take-over]");
  if (!b) return;
  e.preventDefault();
  Screen.takeOnOpen = b.dataset.takeOver;
  const to = `#/computer/${enc(b.dataset.takeOver)}/screen`;
  if (location.hash === to) showComputer($("#main"), b.dataset.takeOver, "screen"); else location.hash = to;
});
