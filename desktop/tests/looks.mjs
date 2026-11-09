// The looks as the Magnus Workspace references draw them, in a real browser
// (tests/scratch/tests/looks.rs sets the scene up): each look the references
// draw, at their size, with the trip open beside its conversation, and
// Settings > Appearance.
//
// MAGNUS_URL, MAGNUS_PERSON, MAGNUS_PASSWORD  where and who
// MAGNUS_TASK      the finished trip, opened in the panel; each look's colors
//                  and sizes are checked against the references' values
// MAGNUS_CHROMIUM  Chrome to drive; MAGNUS_SHOTS  where to keep screenshots
import { createRequire } from "node:module";
import { mkdirSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "../..");
const { chromium } = createRequire(import.meta.url)(join(root, "browser/node_modules/playwright-core"));
const env = k => { const v = process.env[k]; if (!v) { console.error(`${k} is not set`); process.exit(2); } return v; };
const [base, person, password, task] = ["MAGNUS_URL", "MAGNUS_PERSON", "MAGNUS_PASSWORD", "MAGNUS_TASK"].map(env);
const shots = process.env.MAGNUS_SHOTS;
if (shots) mkdirSync(shots, { recursive: true });
const T = 60_000;
// A screenshot is evidence, never a check.
const shoot = (page, name) => shots ? page.screenshot({ path: join(shots, name) }).catch(e => console.error(`screenshot not kept: ${e.message.split("\n")[0]}`)) : null;

const browser = await chromium.launch({ executablePath: env("MAGNUS_CHROMIUM"), args: ["--no-sandbox", "--disable-dev-shm-usage"] });
const failures = [];
// The Magnus Workspace design's values (owner-approved 2026-11-01), copied
// here apart from theme.js so a drift in either shows. Glass frosts the
// navigation chrome only; the conversation and the work panel stay solid.
const LIGHT = { side: "#F3F3F0", main: "#FFFFFF", text: "#1C1C1E" };
const NIGHT = { side: "#161618", main: "#1C1C1F", text: "#F2F2F4" };
const REF = {
  "light-solid": LIGHT, "light-glass": { ...LIGHT, side: "rgba(248,248,246,0.62)" },
  "dark-solid": NIGHT, "dark-glass": { ...NIGHT, side: "rgba(26,26,30,0.58)" },
};
// A color as [r, g, b, a], from #RRGGBB or rgb()/rgba().
function rgba(c) {
  const h = /^#([0-9a-f]{6})$/i.exec(c);
  if (h) return [0, 2, 4].map(i => parseInt(h[1].slice(i, i + 2), 16)).concat(1);
  const m = /rgba?\(([^)]+)\)/.exec(c);
  if (!m) return null;
  const v = m[1].split(/[ ,\/]+/).filter(Boolean).map(Number);
  return [v[0], v[1], v[2], v.length > 3 ? v[3] : 1];
}
const same = (a, b) => { const [x, y] = [rgba(a), rgba(b)]; return x && y && x.every((v, i) => Math.abs(v - y[i]) <= (i === 3 ? 0.01 : 1)); };
function expect(look, what, got, want, eq = same) {
  if (got == null) failures.push(`${look}: ${what} is not drawn`);
  else if (!eq(got, want)) failures.push(`${look}: ${what} is ${got}, the reference's is ${want}`);
}
const px = (a, b) => Math.abs(parseFloat(a) - parseFloat(b)) < 0.5;

const LOOKS = Object.keys(REF).map(name => { const [mode, material] = name.split('-'); return [name, { mode, material, accent: 'blue', motion: 'off' }]; });

async function open(look) {
  const page = await browser.newPage({ viewport: { width: 1440, height: 1060 } });
  await page.addInitScript(({l,person}) => { try { localStorage.setItem(`magnus.ui.v2.${location.origin}.${person}`, JSON.stringify(l)); } catch { /* none */ } }, {l:look,person});
  page.on("pageerror", e => failures.push(`page error: ${e}`));
  await page.goto(base);
  await page.getByLabel("Username").fill(person);
  await page.getByLabel("Password").fill(password);
  await page.getByRole("button", { name: "Sign in" }).click();
  await page.getByRole("link", { name: /^Needs you/ }).waitFor({ timeout: T });
  return page;
}

for (const [name, look] of LOOKS) {
  const page = await open(look);
  // The conversation is home; the trip opens beside it.
  await page.locator(`[data-open="task:${task}"]`).first().click();
  await page.locator("aside.detail").waitFor({ timeout: T });
  await page.locator("iframe.composed").first().waitFor({ timeout: T });
  // Measured again until every part is drawn: a live update redraws the
  // panel, and one measured mid-redraw found none of its parts (#250, run
  // 42169676436). A part still missing after T is reported as not drawn.
  const measure = () => page.evaluate(() => {
    const cs = (sel, prop, root = document) => { const e = root.querySelector(sel); return e ? getComputedStyle(e)[prop] : null; };
    const frame = document.querySelector("aside.detail iframe.composed")?.contentDocument;
    return {
      body: cs("body", "color"),
      documentFont: frame ? cs("body", "fontSize", frame) : null,
      side: cs("nav.side", "backgroundColor"),
      panel: cs("aside.detail", "backgroundColor"),
      panelWidth: cs("aside.detail", "width"),
      main: cs("main", "backgroundColor"),
      search: cs("#find input", "backgroundColor"),
      searchHeight: cs("#find input", "height"),
      needsHeight: cs("nav.side a.needs", "height"),
      needsRadius: cs("nav.side a.needs", "borderTopLeftRadius"),
      seg: cs("aside.detail .seg.layers", "backgroundColor"),
      segOn: cs("aside.detail .seg.layers button.on", "backgroundColor"),
      tabHeight: cs("aside.detail .seg.layers button", "height"),
      groupHead: cs(".rg-head", "backgroundColor"),
      rowHi: cs(".rg-row.open", "backgroundColor"),
      h3: cs("aside.detail .layer h3", "color"),
      h3Case: cs("aside.detail .layer h3", "textTransform"),
      h3Size: cs("aside.detail .layer h3", "fontSize"),
      verdictHeight: cs("aside.detail .verdict button[data-verdict]", "height"),
      card: frame && cs("figure", "backgroundColor", frame),
      cardEdge: frame && cs("figure", "borderTopColor", frame),
      cardRadius: frame && cs("figure", "borderTopLeftRadius", frame),
      when: frame && cs(".tl .when", "color", frame),
      cardText: frame && cs(".tl .lbl", "color", frame),
      frameBg: frame && cs("body", "backgroundColor", frame),
    };
  });
  const missing = got => Object.values(got).some(v => v == null);
  const until = Date.now() + T;
  let got = await measure();
  while (missing(got) && Date.now() < until) {
    await page.waitForTimeout(250);
    got = await measure();
  }
  await shoot(page, `app-${name}.png`);
  const r = REF[name];
  for (const [what,key,want] of [['content text','body',r.text],['conversation','main',r.main],['result text','cardText',r.text]]) expect(name,what,got[key],want);
  const chrome = await page.locator('nav.side').evaluate(el => getComputedStyle(el).color);
  expect(name,'chrome text',chrome,r.text);
  // Check a real chrome control's hover/pressed fills and contrast, cancelling the pointer click.
  const action=page.locator('[data-expand-work]');const box=await action.boundingBox();
  const color=()=>action.evaluate(el=>{const c=document.createElement('canvas');c.width=c.height=1;const x=c.getContext('2d');const rgba=value=>{x.clearRect(0,0,1,1);x.fillStyle=value;x.fillRect(0,0,1,1);return [...x.getImageData(0,0,1,1).data];};const s=getComputedStyle(el);return {fg:rgba(s.color),bg:rgba(s.backgroundColor)};});
  await action.hover();await page.waitForTimeout(200);const hover=await color();await page.mouse.down();await page.waitForTimeout(200);const pressed=await color();await page.mouse.move(4,4);await page.mouse.up();
  const luminance=c=>c.slice(0,3).map(v=>{v/=255;return v<=.04045?v/12.92:((v+.055)/1.055)**2.4;}).reduce((n,v,i)=>n+v*[.2126,.7152,.0722][i],0);
  for(const state of [hover,pressed]){const [a,b]=[luminance(state.fg),luminance(state.bg)].sort((a,b)=>b-a);if((a+.05)/(b+.05)<4.5)failures.push(`${name}: control state contrast failed`);}
  if(hover.bg.join(',')===pressed.bg.join(','))failures.push(`${name}: pressed fill must differ from hover`);

  if (parseFloat(got.documentFont)<15 || !got.documentFont) failures.push(`${name}: document typography is ${got.documentFont}, expected at least 15px`);
  const avatarContrast = await page.locator('nav.side .avatar').evaluate(el => {
    const hex = value => '#' + value.match(/\d+/g).slice(0,3).map(v => Number(v).toString(16).padStart(2,'0')).join('');
    const css = getComputedStyle(el); return Theme.contrast(hex(css.color), hex(css.backgroundColor));
  });
  if (avatarContrast < 4.5) failures.push(`${name}: avatar initial contrast ${avatarContrast.toFixed(2)}:1`);
  expect(name,'navigation surface',got.side,r.side);
  expect(name,'work panel',got.panel,r.main);
  const prior = await page.locator('iframe.composed').first().evaluate(f => ({html:f.contentDocument.body.innerHTML,scroll:f.contentWindow.scrollY}));
  const other = look.mode === 'dark' ? 'light' : 'dark';
  await page.evaluate(mode => Theme.set({mode}), other);
  const after = await page.locator('iframe.composed').first().evaluate(f => ({html:f.contentDocument.body.innerHTML,scroll:f.contentWindow.scrollY,color:getComputedStyle(f.contentDocument.body).color}));
  if (prior.html !== after.html || prior.scroll !== after.scroll) failures.push(`${name}: changing theme replaced frame content or scroll`);
  expect(name,'open frame theme',after.color,REF[`${other}-solid`].text);
  console.log(`checked ${name}`);
  await page.close();
}

{
  const page = await open(LOOKS[0][1]);
  await page.getByRole("link", { name: "Settings" }).click();
  await page.getByRole("navigation", { name: "Settings" }).getByRole("link", { name: "Appearance" }).click();
  await page.waitForTimeout(800);
  await shoot(page, "app-appearance.png");
  const got = await page.evaluate(() => {
    const cs = (sel, prop) => { const e = document.querySelector(sel); return e ? getComputedStyle(e)[prop] : null; };
    return {
      labels: [...document.querySelectorAll(".appearance h3")].map(h => h.textContent),
      track: cs(".appearance .seg", "backgroundColor"),
      option: cs(".appearance .seg button", "height"),
      swatch: cs(".appearance .swatch", "width"),
    };
  });
  const want = ["Mode", "Material", "Accent color", "Motion"];
  if (want.join("|") !== got.labels.join("|")) failures.push(`appearance: the sections are ${got.labels.join(", ")}, the reference's are ${want.join(", ")}`);

  expect("appearance", "a choice's height", got.option, "40px", px);
  expect("appearance", "a swatch's size", got.swatch, "44px", px);
  console.log("checked appearance");
  await page.close();
}

// A closed panel is gone: in a window narrow enough that the panel lies over
// the conversation, nothing covers the conversation while it is closed.
{
  const page = await open(LOOKS[0][1]);
  await page.setViewportSize({ width: 1000, height: 800 });
  await page.waitForTimeout(500);
  const hit = await page.evaluate(() => {
    const d = document.querySelector("#detail");
    const m = document.querySelector("main").getBoundingClientRect();
    const e = document.elementFromPoint(m.right - 40, m.top + m.height / 2);
    return { hidden: d.hidden, over: !!e?.closest("#detail"), what: e ? (e.id || e.className || e.tagName) : null };
  });
  if (!hit.hidden) failures.push("narrow: the panel is open on arrival");
  else if (hit.over) failures.push(`narrow: the closed panel covers the conversation (${hit.what})`);
  console.log("checked the closed panel");
  await page.close();
}

await browser.close();
if (failures.length) { console.error(failures.map(f => `FAIL ${f}`).join("\n")); process.exit(1); }
console.log("looks done");
