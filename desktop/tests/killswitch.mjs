// The owner's kill switch, in a real browser: one control in the top bar
// stops everything with no question, Admin › Overview lists the sign-ins
// revoked at their providers, and releasing asks for the password and says
// nothing resumes by itself.
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
    for (const q of document.querySelectorAll("dialog.ask[open]:not(.step-up):not([data-answered])")) {
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

// The switch is in the top bar on every page (spec rev 141, Kill switch);
// what it revoked and sealed is Admin › Overview's.
const bar = () => page.locator("#workspace-bar [data-admin-kill]");
const report = async () => {
  await page.getByRole("link", { name: "Admin" }).first().click();
  await page.getByRole("navigation", { name: "Admin sections" }).getByRole("link", { name: "Overview" }).click();
};

await step("one control stops everything, with no question", async () => {
  await bar().getByRole("button", { name: "Stop everything" }).click();
  await bar().getByText("Everything is stopped").waitFor({ timeout: T });
  if (dialogs.length) throw new Error(`pressing asked first: ${dialogs}`);
  await report();
  await page.locator('[data-kill-switch="on"]').waitFor({ timeout: T });
  await page.locator('[data-revoked] tr[data-secret="oa_credential"]', { hasText: "revoked by the provider" }).waitFor({ timeout: T });
  await page.locator('[data-sealed] [data-secret="mail_token"]').waitFor({ timeout: T });
  // Above the conversation, it says so, naming who.
  await page.getByRole("link", { name: "New conversation", exact: true }).click();
  await page.getByText("The kill switch is on.").waitFor({ timeout: T });
});

await step("releasing asks for the password and resumes nothing", async () => {
  await bar().getByRole("button", { name: "Release" }).click();
  const sheet = page.getByRole("dialog", { name: "Confirm your password" });
  await sheet.getByLabel("Password").fill(password);
  await sheet.getByRole("button", { name: "Confirm" }).click();
  await page.getByText("Released. Stopped tasks stay stopped until each is continued.").waitFor({ timeout: T });
  await report();
  await page.locator('[data-kill-switch="off"]').waitFor({ timeout: T });
  await page.locator('[data-revoked] tr[data-secret="oa_credential"]').waitFor({ timeout: T });
  await page.getByText("connect these again").waitFor({ timeout: T });
});

if (errors.length) { console.error(`FAIL page errors:\n${errors.join("\n")}`); await browser.close(); process.exit(1); }
await browser.close();
console.log("kill switch passed");
