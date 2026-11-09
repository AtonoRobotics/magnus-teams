// A person's browser signing in at the identity provider: the real Chromium
// (Playwright's, as the browser sidecar uses) opens the address, fills the
// provider's own login page, grants access if the provider asks (the device
// grant does), and follows wherever the provider sends it.
//
// node sign-in-browser.mjs URL USERNAME PASSWORD [deny]
//   deny: decline when the provider asks to grant access
// node sign-in-browser.mjs URL
//   as a computer's default browser, for the installed app: who signs in is
//   in the file MAGNUS_SIGN_IN_AS names (username, then password, a line each)
//   MAGNUS_CHROMIUM  the Chromium to run (default: Playwright's, as tests use)
//
// Prints the address the browser ended on, or the provider's error and exits 1.
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
import path from "node:path";
import { readFileSync } from "node:fs";

const here = path.dirname(fileURLToPath(import.meta.url));
const require = createRequire(path.join(here, "../../browser/package.json"));
const { chromium } = require("playwright-core");

const args = process.argv.slice(2);
if (args.length === 1 && process.env.MAGNUS_SIGN_IN_AS) args.push(...readFileSync(process.env.MAGNUS_SIGN_IN_AS, "utf8").split("\n").slice(0, 2));
const [url, username, password, answer] = args;
if (!url || !username || !password) {
  console.error("usage: sign-in-browser.mjs URL USERNAME PASSWORD [deny], or URL with MAGNUS_SIGN_IN_AS");
  process.exit(2);
}
const executablePath = process.env.MAGNUS_CHROMIUM || path.join(process.env.HOME, ".cache/ms-playwright/chromium-1234/chrome-linux64/chrome");

const browser = await chromium.launch({ executablePath, headless: true });
try {
  const page = await (await browser.newContext()).newPage();
  await page.goto(url);
  // The provider's account page is a script that sends the browser on to
  // its sign-in form: wait for a form to fill before looking for one.
  await page.waitForSelector("#username, #password-new, a[href*='login-actions/action-token'], input[name=accept], button[name=accept]", { timeout: 15000 }).catch(() => {});
  // A link from the provider's email (an invitation) opens on "Perform the
  // following action: Update password", then on choosing a password: there
  // is no sign-in form to fill.
  const proceed = page.locator("a[href*='login-actions/action-token']");
  if (!(await page.locator("#username").count()) && (await proceed.count())) {
    await Promise.all([page.waitForEvent("load"), proceed.first().click()]);
  }
  if (await page.locator("#username").count()) {
    await page.fill("#username", username);
    await page.fill("#password", password);
    // Each submit waits for the next page's own load: waiting for the load
    // state returns at once on the page already loaded, and the steps below
    // would then look at the page they are leaving (on a busy machine, the
    // grant was skipped and the sign-in never approved).
    await Promise.all([page.waitForEvent("load"), page.click("#kc-login")]);
  }
  // A new account may have to choose its password first (Keycloak's
  // "Update password" required action); the person keeps the same one.
  if (await page.locator("#password-new").count()) {
    await page.fill("#password-new", password);
    await page.fill("#password-confirm", password);
    await Promise.all([page.waitForEvent("load"), page.locator("input[type=submit], button[type=submit]").first().click()]);
  }
  // The device grant asks the person to grant access to the client.
  const grant = page.locator(answer === "deny" ? "input[name=cancel], button[name=cancel]" : "input[name=accept], button[name=accept]");
  if (await grant.count()) await Promise.all([page.waitForEvent("load"), grant.first().click()]);
  const error = page.locator("#input-error, .kc-feedback-text, .alert-error, #kc-error-message");
  const landed = page.url();
  if (landed.startsWith(new URL(url).origin) && await error.count()) {
    console.error(`the provider said: ${(await error.first().innerText()).trim()}`);
    process.exit(1);
  }
  console.log(landed);
} finally {
  await browser.close();
}
