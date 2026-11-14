// Programs on the person's computers, in Apps (connectors spec, Local
// desktop connectors): one consent per program per computer, "Use
// <program> on this computer", Allow or Not now; once allowed, where it
// stands (closed or ready) and a way to stop using it there. Each other
// computer is listed by its name with its own set, and a program can be
// stopped there from anywhere. Allowing is only ever done on the computer
// itself, from the app.
"use strict";

const Programs = { device: null, list: null, later: new Set() };
const PROGRAM_STATES = { not_installed: "Not installed", not_enabled: "Not enabled", closed: "Closed: open it and Magnus can work in it", ready: "Ready" };

/** @typedef {{ program: string, title: string, state: "not_installed"|"not_enabled"|"closed"|"ready", allowed: boolean, version: ?string }} ProgramView */
/** @typedef {{ id: string, name: string, online: boolean, programs: ProgramView[] }} DeviceView */

/** One program on this computer: the consent, or where it stands. */
function programCard(p, later) {
  const t = esc(p.title), id = esc(p.program);
  const version = p.version ? ` <span class="small faint">${esc(p.version)}</span>` : "";
  if (p.state === "not_enabled" && !later) {
    return `<article class="lcard pcard" data-program="${id}" data-state="consent"><div class="lc-main">
      <h3>Use ${t} on this computer</h3>${version}
      <p class="lc-sum">Magnus can work in ${t} here when you ask it to. Anything that runs code inside ${t} asks you first, and you can stop at any time.</p>
      <div class="row"><button class="primary" data-act="program-allow" data-program="${id}">Allow</button><button data-act="program-later" data-program="${id}">Not now</button></div></div></article>`;
  }
  if (p.state === "not_enabled") {
    return `<article class="lcard pcard" data-program="${id}" data-state="not_enabled"><div class="lc-main"><h3>${t}</h3>${version}<p class="lc-note">Not enabled</p></div>
      <button data-act="program-allow" data-program="${id}">Use ${t} on this computer</button></article>`;
  }
  return `<article class="lcard pcard" data-program="${id}" data-state="${esc(p.state)}"><div class="lc-main"><h3>${t}</h3>${version}
    <p class="lc-note${p.state === "ready" ? " ok" : ""}">${esc(PROGRAM_STATES[p.state] || p.state)}</p></div>
    <button data-act="program-stop" data-program="${id}" data-device="">Stop using ${t} here</button></article>`;
}

/** Another computer, by its name, with the programs found or allowed there. */
function otherComputer(d) {
  const shown = d.programs.filter(p => p.state !== "not_installed" || p.allowed);
  const rows = shown.length ? shown.map(p => `<li data-program="${esc(p.program)}" data-state="${esc(p.state)}">${esc(p.title)}: ${esc(PROGRAM_STATES[p.state] || p.state)}${p.allowed
    ? ` <button class="link" data-act="program-stop" data-program="${esc(p.program)}" data-device="${esc(d.id)}">Stop using it there</button>` : ""}</li>`).join("") : `<li class="faint">No supported programs found</li>`;
  return `<div class="pcomputer" data-device="${esc(d.id)}"><h3>${esc(d.name)}${d.online ? "" : ' <span class="small faint">not connected now</span>'}</h3><ul class="cp-list">${rows}</ul></div>`;
}

/**
 * The Apps page's programs: this computer's, then the others'.
 * @param {?{id: string}} device this computer, in the app; null in a browser
 * @param {?DeviceView[]} list the person's computers, as the server has them
 * @param {Set<string>} later programs the person said Not now to here
 */
function programsSection(device, list, later) {
  if (!list) return "";
  const here = device && list.find(d => d.id === device.id);
  const others = list.filter(d => d !== here);
  let html = "";
  if (here) {
    const found = here.programs.filter(p => p.state !== "not_installed");
    html += `<section class="lsect" data-section="programs-here"><div class="lsect-head"><div><h2>On this computer</h2><p>Programs Magnus found here. Allow one and Magnus can work in it.</p></div></div>
      ${found.length ? `<div class="lgrid">${found.map(p => programCard(p, later.has(p.program))).join("")}</div>` : `<p class="empty">No supported programs found on this computer yet (Blender, FreeCAD, Autodesk Fusion, SolidWorks).</p>`}</section>`;
  }
  if (others.length) {
    html += `<section class="lsect" data-section="programs-elsewhere"><div class="lsect-head"><div><h2>${here ? "On your other computers" : "On your computers"}</h2><p>Each computer has its own programs. A program is allowed on the computer itself.</p></div></div>
      ${others.map(otherComputer).join("")}</section>`;
  }
  return html;
}

/** Allow, Not now or Stop: the consent goes to the server, which keeps it. */
async function programAct(act, program, deviceId) {
  const id = deviceId || Programs.device?.id;
  if (act === "program-later") { Programs.later.add(program); return; }
  if (!id) return;
  const allowed = act === "program-allow";
  if (!allowed && !(await ask("Stop Magnus using this program on that computer? You can allow it again here at any time.", "Stop"))) return;
  await api("PUT", `/v1/desktop/devices/${enc(id)}/programs/${enc(program)}`, { allowed });
  Programs.later.delete(program);
  await Magnus.desktopRefresh();
}

async function loadPrograms() {
  Programs.device = await Magnus.desktopDevice().catch(() => null);
  Programs.list = await api("GET", "/v1/desktop/devices").catch(() => null);
}

/** Draw the programs into the Apps page, and keep them current. */
async function mountPrograms(root) {
  const box = root.querySelector("#lib-programs");
  if (!box) return;
  const paint = () => { box.innerHTML = programsSection(Programs.device, Programs.list, Programs.later); };
  await loadPrograms();
  paint();
  if (box.dataset.bound) return;
  box.dataset.bound = "1";
  box.addEventListener("click", async e => {
    const a = e.target.closest("[data-act^='program-']");
    if (!a) return;
    e.preventDefault();
    try { await programAct(a.dataset.act, a.dataset.program, a.dataset.device); } catch (err) { toast(err.message); }
    await loadPrograms();
    paint();
  });
}
