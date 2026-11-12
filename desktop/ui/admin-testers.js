// Admin › People › Testers: who is in the tester program, and who is
// invited. An admin may invite someone; only that person can join, by
// accepting the Tester Terms in their own app.
"use strict";

/**
 * @typedef {{ person: string, version: string, current: boolean }} AdminTester
 * @typedef {{ person: string, by: string }} AdminTesterInvite
 */

/** @param {AdminTester[]} testers @param {AdminTesterInvite[]} invited @param {{ id: string, name: string }[]} people */
function adminTestersHtml(testers, invited, people) {
  const name = id => people.find(p => p.id === id)?.name || id;
  const busy = new Set([...testers.map(t => t.person), ...invited.map(i => i.person)]);
  const rows = testers.map(t => `<tr data-tester="${esc(t.person)}"><td>${esc(name(t.person))}</td><td><span class="chip ok">Tester</span>${t.current ? "" : ` <span class="small muted">accepted earlier terms; shown in full again once they accept the new ones</span>`}</td></tr>`)
    .concat(invited.map(i => `<tr data-tester-invited="${esc(i.person)}"><td>${esc(name(i.person))}</td><td><span class="chip">Invited</span> <span class="small muted">by ${esc(name(i.by))}</span></td></tr>`));
  const others = people.filter(p => !busy.has(p.id));
  return `<h2>Testers</h2>
    <p class="small muted">Magnus's developers see a tester's work in full. People join only by accepting the Tester Terms themselves, and leave in Settings.</p>
    ${rows.length ? `<table class="t" data-testers><tbody>${rows.join("")}</tbody></table>` : `<p class="small faint">Nobody is in the tester program.</p>`}
    ${others.length ? `<form class="row" data-tester-invite><select name="person" aria-label="Who to invite">${others.map(p => `<option value="${esc(p.id)}">${esc(p.name)}</option>`).join("")}</select><button>Invite to the tester program</button></form>` : ""}`;
}

/** Draw the testers into `box`. @param {HTMLElement} box @param {{ id: string, name: string }[]} people */
async function adminTesters(box, people) {
  const v = await adminApi("GET", "/v1/admin/testers");
  paint(box, adminTestersHtml(v.testers || [], v.invited || [], people));
  const form = $("[data-tester-invite]", box);
  if (form) form.onsubmit = async e => {
    e.preventDefault();
    const f = Object.fromEntries(new FormData(/** @type {HTMLFormElement} */ (e.target)));
    try { await adminApi("POST", "/v1/admin/testers/invite", { person: f.person }); toast("Invited. They are asked when they next open Magnus."); }
    catch (err) { if (!err.cancelled) toast(err.body?.error || err.message); }
    await adminTesters(box, people);
  };
}
