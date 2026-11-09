// The owner's kill switch on Home, in a real browser: one control stops
// everything (after a confirmation), the panel lists the sign-ins revoked at
// their providers, and turning it off says nothing resumes by itself.
//
// MAGNUS_URL, MAGNUS_PERSON, MAGNUS_PASSWORD  where and who (the owner)
// MAGNUS_CHROMIUM  Chrome to drive; MAGNUS_SHOTS  where to keep screenshots
import { createRequire } from "node:module";
import { mkdirSync, readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "../..");
const { chromium } = createRequire(import.meta.url)(join(root, "browser/node_modules/playwright-core"));
const env = k => { const v = process.env[k]; if (!v) { console.error(`${k} is not set`); process.exit(2); } return v; };
const [base, person, password] = ["MAGNUS_URL", "MAGNUS_PERSON", "MAGNUS_PASSWORD"].map(env);
const shots = process.env.MAGNUS_SHOTS;
if (shots) mkdirSync(shots, { recursive: true });
const win = JSON.parse(readFileSync(join(root, "desktop/src-tauri/tauri.conf.json"), "utf8")).app.windows[0];

const browser = await chromium.launch({ executablePath: env("MAGNUS_CHROMIUM"), args: ["--no-sandbox", "--disable-dev-shm-usage"] });
const page = await browser.newPage({ viewport: { width: win.width, height: win.height } });
const errors = [];
const dialogs = [];
page.on("pageerror", e => errors.push(String(e)));
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
const T = 60_000;
let n = 0;
const step = async (name, f) => {
  try { await f(); if (shots) await page.screenshot({ path: join(shots, `killswitch-${String(++n).padStart(2, "0")}-${name.replace(/\W+/g, "-")}.png`), fullPage: true }).catch(e => console.error(`screenshot not kept: ${e.message.split("\n")[0]}`)); console.log(`ok   ${name}`); }
  catch (e) { if (shots) await page.screenshot({ path: join(shots, "killswitch-FAILED.png"), fullPage: true }).catch(e => console.error(`screenshot not kept: ${e.message.split("\n")[0]}`)); console.error(`FAIL ${name}: ${e.message}`); await browser.close(); process.exit(1); }
};

await step("sign in", async () => {
  await page.goto(base);
  await page.getByLabel("Username").fill(person);
  await page.getByLabel("Password").fill(password);
  await page.getByRole("button", { name: "Sign in" }).click();
  // Signed in: the workspace, with Needs you in the sidebar.
  await page.getByRole("link", { name: /^Needs you/ }).waitFor({ timeout: T });
});

const killSwitch = async () => {
  await page.getByRole("link", { name: "Settings" }).click();
  await page.getByRole("navigation", { name: "Settings" }).getByRole("link", { name: "Kill switch" }).click();
};

await step("one control stops everything", async () => {
  await killSwitch();
  await page.locator('[data-kill-switch="off"]').getByRole("button", { name: "Stop everything" }).click();
  await page.locator('[data-kill-switch="on"]').waitFor({ timeout: T });
  if (!dialogs.some(m => m.includes("every sign-in in the vault is revoked at its provider"))) throw new Error(`no confirmation said what it does: ${dialogs}`);
  await page.locator('[data-revoked] tr[data-secret="oa_credential"]', { hasText: "revoked by the provider" }).waitFor({ timeout: T });
  await page.locator('[data-sealed] [data-secret="mail_token"]').waitFor({ timeout: T });
  // Above the conversation, it says so.
  await page.getByRole("link", { name: "New conversation", exact: true }).click();
  await page.getByText("The kill switch is on.").waitFor({ timeout: T });
  await killSwitch();
});

await step("turning it off resumes nothing", async () => {
  await page.getByRole("button", { name: "Turn the kill switch off" }).click();
  await page.locator('[data-kill-switch="off"]').waitFor({ timeout: T });
  if (!dialogs.some(m => m.includes("stay suspended until you resume each one"))) throw new Error(`no confirmation said nothing resumes: ${dialogs}`);
  await page.locator('[data-revoked] tr[data-secret="oa_credential"]').waitFor({ timeout: T });
  await page.getByText("connect these again").waitFor({ timeout: T });
});

if (errors.length) { console.error(`FAIL page errors:\n${errors.join("\n")}`); await browser.close(); process.exit(1); }
await browser.close();
console.log("kill switch passed");
