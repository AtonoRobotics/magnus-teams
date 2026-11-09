// Drives the installed desktop app through tauri-driver (W3C WebDriver), for
// the installed-app scripts: open a session on the app, act on its page,
// and the steps every script begins with (pin the appliance's certificate,
// sign in with a username and password).
import { writeFileSync, mkdirSync } from "node:fs";
import { join } from "node:path";

export const env = k => { const v = process.env[k]; if (!v) { console.error(`${k} is not set`); process.exit(2); } return v; };
const E = "element-6066-11e4-a52e-4f735466cecf";
const T = 60_000;

// A session on the app at `app`, through the driver at `wd`. Screenshots go
// to `shots` (optional), named with `prefix`.
export async function open(wd, app, shots, prefix) {
  if (shots) mkdirSync(shots, { recursive: true });
  async function call(method, path, body) {
    const r = await fetch(`${wd}${path}`, { method, headers: { "content-type": "application/json" }, body: body === undefined ? undefined : JSON.stringify(body) });
    const text = await r.text();
    let j;
    try { j = JSON.parse(text); } catch { throw new Error(`${method} ${path}: HTTP ${r.status}, not WebDriver's answer: ${text.slice(0, 200) || "(empty)"}`); }
    if (!r.ok || (j.value && j.value.error)) throw new Error(`${method} ${path}: ${JSON.stringify(j.value)}`);
    return j.value;
  }
  const session = (await call("POST", "/session", { capabilities: { alwaysMatch: { "tauri:options": { application: app } } } })).sessionId;
  const s = p => `/session/${session}${p}`;
  const d = { call, s };
  d.run = (script, ...args) => call("POST", s("/execute/sync"), { script, args });
  d.until = async (what, f, ms = T) => {
    const end = Date.now() + ms;
    let last;
    while (Date.now() < end) {
      try { const v = await f(); if (v) return v; } catch (e) { last = e; }
      await new Promise(r => setTimeout(r, 250));
    }
    throw new Error(`timed out waiting for ${what}${last ? `: ${last.message}` : ""}`);
  };
  // Where each element was found, to find it again: the app redraws parts
  // of its page (its navigation, as counts change), and an element found
  // just before a redraw is gone ("stale element reference") when used.
  const how = new Map();
  const find = (using, value) => d.until(value, async () => {
    const els = await call("POST", s("/elements"), { using, value });
    if (!els.length) return null;
    how.set(els[0][E], [using, value]);
    return els[0][E];
  });
  const on = async (el, act) => {
    let id = await el;
    const end = Date.now() + T;
    for (;;) {
      try { return await act(id); }
      catch (e) {
        if (!/stale element reference/.test(e.message) || !how.has(id) || Date.now() > end) throw e;
        id = await find(...how.get(id));
      }
    }
  };
  d.css = v => find("css selector", v);
  d.xpath = v => find("xpath", v);
  d.button = (label, within = "") => d.xpath(`${within}//button[normalize-space()="${label}"]`);
  // A click lands where the element was when the driver measured it. A card
  // sizes its outcome frame once the frame has loaded (`data-sized`), which
  // moves the buttons below it; on a busy runner the update test's Accept
  // was clicked just before, landed on the frame, and nothing was accepted
  // (runs 41969612463 and 42105845733, #207). So an element is clicked once
  // no outcome frame in its card is still loading and it is where it was a
  // tenth of a second before. A stale element goes back to `on` to be found
  // again.
  const settled = async id => {
    const at = () => d.run("const c = arguments[0].closest('article, [data-entry], #detail'); if (c && c.querySelector('iframe.composed:not([data-sized])')) return null; const r = arguments[0].getBoundingClientRect(); return [r.x, r.y, r.width, r.height].join()", { [E]: id });
    const end = Date.now() + T;
    for (let last = await at(); ; ) {
      await new Promise(r => setTimeout(r, 100));
      const now = await at();
      if (now !== null && now === last) return;
      if (Date.now() > end) throw new Error(`timed out waiting for the element to stop moving (${now ?? "an outcome frame in its card is still loading"})`);
      last = now;
    }
  };
  d.click = el => on(el, async id => {
    // WebKit's automatic click scroll can miss clipping by nested scroll
    // regions for absolute frame actions. Scroll the actual target into
    // its container first, then measure and perform the real native click.
    await d.run("arguments[0].scrollIntoView({block:'nearest',inline:'nearest'})",{[E]:id});
    await settled(id);
    try{return await call("POST",s(`/element/${id}/click`),{});}catch(error){
      if(/element click intercepted|element not interactable/.test(error.message)){
        try{const evidence=await d.run("const e=arguments[0],r=e.getBoundingClientRect(),x=Math.max(0,Math.min(innerWidth-1,r.left+r.width/2)),y=Math.max(0,Math.min(innerHeight-1,r.top+r.height/2));return {target:{tag:e.tagName,role:e.getAttribute('role'),label:e.getAttribute('aria-label'),text:e.textContent.slice(0,100),rect:r.toJSON()},hit:document.elementsFromPoint(x,y).slice(0,5).map(n=>({tag:n.tagName,id:n.id,class:n.className,role:n.getAttribute('role')})),viewport:[innerWidth,innerHeight]}",{[E]:id});throw new Error(`${error.message}; visible hit-test evidence: ${JSON.stringify(evidence)}`);}catch(evidenceError){if(evidenceError.message.includes('visible hit-test evidence'))throw evidenceError;}
      }
      throw error;
    }
  });
  d.type = (el, text) => on(el, id => call("POST", s(`/element/${id}/value`), { text }));
  d.clear = el => on(el, id => call("POST", s(`/element/${id}/clear`), {}));
  d.text = el => on(el, id => call("GET", s(`/element/${id}/text`)));
  d.attr = (el, name) => on(el, id => call("GET", s(`/element/${id}/attribute/${name}`)));
  // The app asks before an action in its own dialog, never the webview's
  // (which may answer no without showing anything, as Windows' did): that
  // it says `words`, and yes.
  d.answer = async words => {
    const asked = await d.until("the confirmation", () => d.run(`const q = document.querySelector("dialog.ask[open] p"); return q && q.textContent`));
    if (!asked.includes(words)) throw new Error(`the confirmation says: ${asked}`);
    await d.click(d.css("dialog.ask[open] [data-ask=yes]"));
  };
  d.seen = (t, ms) => d.until(`"${t}"`, () => d.run("return document.body.innerText.includes(arguments[0])", t), ms);
  let n = 0;
  const shot = async name => {
    if (!shots) return;
    const file = join(shots, `${prefix}-${String(++n).padStart(2, "0")}-${name}.png`);
    // Evidence only: a screenshot that cannot be taken fails no check.
    try { writeFileSync(file, Buffer.from(await call("GET", s("/screenshot")), "base64")); } catch (e) { console.error(`screenshot not kept: ${String(e.message).split("\n")[0]}`); }
  };
  d.close = () => call("DELETE", s("")).catch(() => {});
  d.step = async (name, f) => {
    try { await f(); await shot(name); console.log(`ok   ${name}`); }
    catch (e) { await shot(`FAILED-${name}`).catch(() => {}); console.error(`FAIL ${name}: ${e.message}`); await d.close(); process.exit(1); }
  };
  return d;
}

// Do `act` (a click that downloads), then wait for the app's next toast and
// return the path of the file it saved, or fail with what it said instead.
// Toasts already showing are from earlier steps and are passed over.
export async function savedPath(d, act) {
  await d.run("document.querySelectorAll('.toast').forEach(t => t.dataset.seen = '1')");
  await act();
  const said = await d.until("the app to say what happened", () => d.run("const t = [...document.querySelectorAll('.toast:not([data-seen])')]; return t.length ? JSON.stringify(t.map(e => e.innerText)) : null"));
  const saved = JSON.parse(said).find(t => t.startsWith("Saved "));
  if (!saved) throw new Error(`the app did not save the file; it said ${said}`);
  return saved.slice("Saved ".length).trim();
}

// Connect to the appliance over HTTPS, pinning the certificate it printed,
// and sign in.
export async function pinAndSignIn(d, { url, fingerprint, provider, person }) {
  await d.step("pin the appliance's certificate", async () => {
    await d.type(d.css("#addr"), url);
    await d.click(d.button("Continue"));
    const shown = (await d.text(d.css("#fp"))).trim();
    if (shown !== fingerprint) throw new Error(`the app shows ${shown}, the appliance printed ${fingerprint}`);
    await d.click(d.button("They match"));
  });
  await signIn(d, { provider, person });
}

// Sign in as `person` (or `as`) on an app already pinned to its appliance:
// type the username and password (MAGNUS_PASSWORDS has each person's) and
// press Sign in. The first time, the app opens the provider's page in the
// computer's default browser, which signs in there as whoever the file
// MAGNUS_SIGN_IN_AS names (the test's browser), and comes back to the app.
// `browser: false`: this sign-in must not need the browser, which is made
// unable to sign anyone in.
export async function signIn(d, { provider, person, as, browser = true }) {
  const who = as || person;
  await d.step(`sign in as ${who}${browser ? "" : " in the app alone"}`, async () => {
    const passwords = JSON.parse(env("MAGNUS_PASSWORDS"));
    if (!passwords[who]) throw new Error(`no password for ${who}`);
    writeFileSync(env("MAGNUS_SIGN_IN_AS"), browser ? `${who}\n${passwords[who]}\n` : "\n\n");
    await d.clear(d.css("#who"));
    await d.type(d.css("#who"), who);
    await d.type(d.css("#pw"), passwords[who]);
    await d.click(d.button("Sign in"));
    // The conversation is home: the workspace shows, with Needs you.
    await d.xpath("//nav//*[self::a or self::button][@aria-label='Needs you' or starts-with(@aria-label,'Needs you,') or starts-with(normalize-space(),'Needs you')]");
    const page = await d.run("return JSON.stringify(localStorage) + JSON.stringify(sessionStorage) + document.cookie");
    if (page.includes("mgs_")) throw new Error("the session token reached the page");
    if (page.includes(passwords[who])) throw new Error("the password stayed in the page's storage");
  });
}
