// The Library, the connector catalog, as a person sees it (connectors spec
// rev 48, What the user sees, The connector card and Library, and the
// owner's catalog mockup), with the real catalog: Google's Gmail, Calendar,
// Drive and Chat, already set up. Discover and Yours; search and category
// chips; each card its tile, the review mark, one plain sentence, who makes
// it and one action, with no protocol words; a connector opens to what
// Magnus can do with it and what it asks for. The page renders in light,
// dark and glass, and at phone width with no sideways scroll. Connecting is
// the desktop app's, at Google's own page. Nothing on the connect path, and
// nothing in the owner's Connectors settings, asks anyone to type anything:
// no client ID, secret or address. No fakes (owner, 2026-11-01).
//
// MAGNUS_URL, MAGNUS_PERSON, MAGNUS_PASSWORD  where and who (the owner)
// MAGNUS_CHROMIUM  Chrome to drive; MAGNUS_SHOTS  where to keep screenshots
import { createRequire } from "node:module";
import { mkdirSync, readFileSync, readdirSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "../..");
const { chromium } = createRequire(import.meta.url)(join(root, "browser/node_modules/playwright-core"));
const env = k => { const v = process.env[k]; if (!v) { console.error(`${k} is not set`); process.exit(2); } return v; };
const [base, person, password] = ["MAGNUS_URL", "MAGNUS_PERSON", "MAGNUS_PASSWORD"].map(env);
const shots = process.env.MAGNUS_SHOTS;
if (shots) mkdirSync(shots, { recursive: true });
const win = JSON.parse(readFileSync(join(root, "desktop/src-tauri/tauri.conf.json"), "utf8")).app.windows[0];
// The catalog as it ships (`catalog/*.toml`): how many connectors it has,
// and which are code services, so the Library is checked against it.
const manifests = readdirSync(join(root, "catalog")).filter(f => f.endsWith(".toml") && f !== "registrations.toml").map(f => readFileSync(join(root, "catalog", f), "utf8"));
const field = (m, k) => m.match(new RegExp(`^${k} = "([^"]*)"`, "m"))?.[1];
const code = manifests.filter(m => field(m, "category") === "code").map(m => field(m, "id"));

const browser = await chromium.launch({ executablePath: env("MAGNUS_CHROMIUM"), args: ["--no-sandbox", "--disable-dev-shm-usage"] });
const page = await browser.newPage({ viewport: { width: win.width, height: win.height } });
const errors = [];
page.on("pageerror", e => errors.push(String(e)));
const dialogs = [];
// A webview may answer window.confirm no without showing it (the Windows
// app's did, 2026-11-03): here it does, and the app's own question, in
// its dialog, is answered yes.
await page.exposeFunction("magnusAsked", m => dialogs.push(m));
await page.addInitScript(() => {
  window.confirm = () => false;
  new MutationObserver(() => {
    for (const q of document.querySelectorAll("dialog.ask[open]:not([data-answered])")) {
      q.dataset.answered = "1";
      window.magnusAsked(q.querySelector("p").textContent);
      q.querySelector("[data-ask=yes]").click();
    }
  }).observe(document, { childList: true, subtree: true, attributes: true, attributeFilter: ["open"] });
});
let n = 0;
const shot = async name => { if (shots) await page.screenshot({ path: join(shots, `connectors-${String(++n).padStart(2, "0")}-${name}.png`), fullPage: true }).catch(e => console.error(`screenshot not kept: ${e.message.split("\n")[0]}`)); };
const step = async (name, f) => {
  try { await f(); await shot(name); console.log(`ok   ${name}`); }
  catch (e) { await shot(`FAILED-${name}`); console.error(`FAIL ${name}: ${e.message}`); await browser.close(); process.exit(1); }
};
const T = 60_000;
const PROTOCOL = /\b(oauth|scopes?|tokens?|mcp|port|https?:)/i;
// Text a person could type in, other than the Library's own search box.
const typed = scope => page.locator(`${scope} input:not([type=search]):not([type=checkbox]):not([type=radio]):not([type=file]):not([type=hidden]), ${scope} textarea`).count();

await step("sign in", async () => {
  await page.goto(base);
  await page.getByLabel("Username").fill(person);
  await page.getByLabel("Password").fill(password);
  await page.getByRole("button", { name: "Sign in" }).click();
  await page.getByRole("link", { name: /^Needs you/ }).waitFor({ timeout: T });
});

// A server without a provider's client secret yet (a new one, 2026-11-09):
// the plus on those connectors did nothing at all when clicked. It says why.
await step("a connector the server cannot sign anyone in to yet says so when its plus is clicked", async () => {
  await page.getByRole("link", { name: "Apps", exact: true }).click();
  await page.getByRole("heading", { name: "Apps", level: 1 }).waitFor({ timeout: T });
  const off = page.locator(".lc-act.off").first();
  if (!await off.count()) { console.log("     every connector is ready on this server: nothing to click"); return; }
  // aria-disabled tells a screen reader it is not ready; a click still
  // reaches it, which Playwright only does when told to.
  await off.click({ force: true });
  await page.getByText("can't be connected on this server yet", { exact: false }).waitFor({ timeout: T });
});

await step("the Library is the catalog: Google's apps under Popular, each a card with a plus to connect", async () => {
  await page.getByRole("link", { name: "Apps", exact: true }).click();
  await page.getByRole("heading", { name: "Apps", level: 1 }).waitFor({ timeout: T });
  const popular = page.locator(".lsect[data-section=popular]");
  for (const name of ["google-gmail", "google-calendar", "google-drive", "google-chat"]) await popular.locator(`[data-connector=${name}]`).getByRole("button", { name: /^Connect / }).waitFor({ timeout: T });
  const gmail = popular.locator("[data-connector=google-gmail]");
  await gmail.getByRole("img", { name: "Reviewed by Alpha Vector" }).waitFor({ timeout: T });
  await gmail.getByText("by Google", { exact: true }).waitFor({ timeout: T });
  const said = (await page.locator("#lib-sections").allInnerTexts()).join(" ");
  if (PROTOCOL.test(said)) throw new Error(`a card shows a protocol word: ${said}`);
  for (const t of ["Gmail", "Google Calendar", "Google Drive", "Google Chat", "Magnus can search and read your email", `${manifests.length} connectors`]) if (!said.includes(t)) throw new Error(`the Library does not say "${t}": ${said}`);
  // Yours: nothing connected yet.
  await page.getByRole("button", { name: "Yours", exact: true }).click();
  await page.getByText("Nothing connected yet.", { exact: false }).waitFor({ timeout: T });
  await page.getByRole("button", { name: "Discover", exact: true }).click();
  await gmail.waitFor({ timeout: T });
});

await step("search and the category chips narrow the catalog", async () => {
  await page.getByLabel("Search apps and capabilities").fill("drive");
  await page.locator("[data-connector=google-gmail]").waitFor({ state: "detached", timeout: T });
  await page.locator("[data-connector=google-drive]").waitFor({ timeout: T });
  // What the person wants done finds it too.
  await page.getByLabel("Search apps and capabilities").fill("free or busy");
  await page.locator("[data-connector=google-drive]").waitFor({ state: "detached", timeout: T });
  await page.getByLabel("Search apps and capabilities").fill("find a time");
  await page.locator("[data-connector=google-calendar]").waitFor({ timeout: T });
  await page.getByLabel("Search apps and capabilities").fill("no-such-app-anywhere");
  await page.getByText("No connectors match.", { exact: false }).waitFor({ timeout: T });
  await page.getByLabel("Search apps and capabilities").fill("");
  await page.getByRole("button", { name: "Files", exact: true }).click();
  await page.locator("[data-connector=google-drive]").waitFor({ timeout: T });
  await page.locator("[data-connector=google-gmail]").waitFor({ state: "detached", timeout: T });
  await page.getByRole("button", { name: "Code", exact: true }).click();
  await page.locator("[data-connector=google-gmail]").waitFor({ state: "detached", timeout: T });
  for (const id of code) await page.locator(`[data-connector=${id}]`).waitFor({ timeout: T });
  await page.getByRole("button", { name: "Design & CAD", exact: true }).click();
  await page.getByText("No connectors match.", { exact: false }).waitFor({ timeout: T });
  await page.getByRole("button", { name: "All", exact: true }).click();
  await page.locator("[data-connector=google-gmail]").waitFor({ timeout: T });
});

await step("the Library renders in light, dark and glass, and at phone width with no sideways scroll", async () => {
  for (const [mode, material] of [["light", "solid"], ["dark", "solid"], ["light", "glass"], ["dark", "glass"]]) {
    for (const width of [win.width, 390]) {
      await page.setViewportSize({ width, height: win.height });
      await page.evaluate(([mode, material]) => Theme.set({ mode, material }), [mode, material]);
      const over = await page.evaluate(() => Math.max(document.documentElement.scrollWidth - document.documentElement.clientWidth, ...[...document.querySelectorAll("main")].map(m => m.scrollWidth - m.clientWidth)));
      if (over > 0) throw new Error(`the Library in ${mode} ${material} at ${width}px scrolls sideways by ${over}px`);
      await shot(`library-${mode}-${material}-${width}`);
    }
  }
  await page.setViewportSize({ width: win.width, height: win.height });
  await page.evaluate(() => Theme.set({ mode: "system", material: "solid" }));
});

await step("connecting happens in the desktop app, on the app's own page, and asks nothing here", async () => {
  if (await typed("main")) throw new Error("the Library asks for something to be typed");
  await page.locator("[data-connector=google-gmail]").getByRole("button", { name: "Connect Gmail" }).click();
  await page.getByText("Connect apps from the Magnus desktop app.").waitFor({ timeout: T });
  for (const scope of ["main", "#detail", "[role=dialog]", ".sheet", ".toast"]) if (await typed(scope)) throw new Error(`connecting asks for something to be typed in Magnus (${scope})`);
});

await step("a connector opens to what Magnus can do with it and what it asks for, in plain words", async () => {
  await page.locator("[data-connector=google-gmail]").getByRole("link", { name: "Gmail" }).click();
  const d = page.locator(".cp");
  await d.getByRole("heading", { name: "Gmail", exact: true }).waitFor({ timeout: T });
  for (const t of ["What Magnus can do with Gmail", "What it asks for", "Write drafts, replies and forwards, and send one when you allow it", "Read, write, send and label your email"]) await d.getByText(t, { exact: true }).first().waitFor({ timeout: T });
  await d.getByText("by Google · Reviewed by Alpha Vector").waitFor({ timeout: T });
  await d.getByText("You sign in on Google's own page. Magnus never sees your password, and you can disconnect at any time.").waitFor({ timeout: T });
  await d.getByRole("button", { name: "Connect Gmail" }).waitFor({ timeout: T });
  const text = await d.innerText();
  if (PROTOCOL.test(text)) throw new Error(`the connector's page shows a protocol word: ${text}`);
  if (await d.getByRole("button", { name: /Disconnect/ }).count()) throw new Error("Disconnect is offered for an app the person has not connected");
  if (await typed("main")) throw new Error("the connector's page asks for something to be typed");
  await page.getByRole("link", { name: "Apps", exact: true }).first().click();
  await page.locator("[data-connector=google-gmail]").waitFor({ timeout: T });
});

await step("the owner's Connectors settings are the catalog's apps, and ask for nothing", async () => {
  await page.getByRole("link", { name: "Settings" }).click();
  await page.getByRole("link", { name: "Connectors" }).click();
  await page.locator('[data-app="google-gmail"]').getByText("from the catalog").waitFor({ timeout: T });
  const said = await page.locator("main").innerText();
  if (/client (id|secret)|registration|its address/i.test(said)) throw new Error(`the settings ask for setup: ${said}`);
  if (await typed("main")) throw new Error("the owner's Connectors settings ask for something to be typed");
});

await step("from Settings, the Library's address opens the Library", async () => {
  await page.evaluate(() => { location.hash = "#/library"; });
  await page.locator(".lsect [data-connector=google-gmail]").waitFor({ timeout: T });
});

if (errors.length) { console.error(`FAIL page errors: ${errors.join("; ")}`); await browser.close(); process.exit(1); }
await browser.close();
console.log("connectors passed");
