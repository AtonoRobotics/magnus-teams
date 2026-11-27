// A page Magnus delivers runs as a page in the conversation, in a real
// browser (tests/scratch/tests/html_deliverable.rs sets it up): the
// dashboard's script has run, the page is in its sandbox, and none of its
// source is shown as text.
//
// MAGNUS_URL, MAGNUS_PERSON, MAGNUS_PASSWORD  where and who
// MAGNUS_THREAD    the conversation; MAGNUS_TASK  the finished task started there
// MAGNUS_CHROMIUM  Chrome to drive; MAGNUS_SHOTS  optional: a screenshot
import { createRequire } from "node:module";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "../..");
const { chromium } = createRequire(import.meta.url)(join(root, "browser/node_modules/playwright-core"));
const env = k => { const v = process.env[k]; if (!v) { console.error(`${k} is not set`); process.exit(2); } return v; };
const [base, person, password, thread, task] = ["MAGNUS_URL", "MAGNUS_PERSON", "MAGNUS_PASSWORD", "MAGNUS_THREAD", "MAGNUS_TASK"].map(env);
const T = 60_000;

const browser = await chromium.launch({ executablePath: env("MAGNUS_CHROMIUM"), args: ["--no-sandbox", "--disable-dev-shm-usage"] });
const page = await browser.newPage({ viewport: { width: 1280, height: 900 } });
const errors = [];
page.on("pageerror", e => errors.push(String(e)));
// A screenshot is evidence, never a check.
const shoot = name => process.env.MAGNUS_SHOTS ? page.screenshot({ path: join(process.env.MAGNUS_SHOTS, name) }).catch(e => console.error(`screenshot not kept: ${e.message.split("\n")[0]}`)) : null;
const fail = async m => { console.error(`FAIL ${m}`); await shoot("html-deliverable-FAILED.png"); await browser.close(); process.exit(1); };

await page.goto(base);
await page.getByLabel("Username").fill(person);
await page.getByLabel("Password").fill(password);
await page.getByRole("button", { name: "Sign in" }).click();
await page.locator("#msg").waitFor({ timeout: T });
await page.evaluate(t => { location.hash = `#/chat/${encodeURIComponent(t)}`; }, thread);
const result = page.locator(`#thread [data-result="${task}"]`);
try { await result.waitFor({ timeout: T }); } catch { await fail("the dashboard's result is not in the conversation"); }

// The page is in the result, in a sandbox that runs its script and keeps
// its own origin away from the app's.
const live = result.locator("iframe.ui-html");
try { await live.first().waitFor({ timeout: T }); } catch { await fail("the delivered page is not drawn live in the conversation"); }
const sandbox = await live.first().getAttribute("sandbox");
if (sandbox !== "allow-scripts") await fail(`the page's sandbox is "${sandbox}", not "allow-scripts"`);

// Its script ran.
const handle = await live.first().elementHandle();
const frame = await handle?.contentFrame();
const ran = frame ? await frame.waitForFunction(() => document.getElementById("state")?.textContent === "Dashboard drawn live", null, { timeout: T }).then(() => true, () => false) : false;
if (!ran) await fail("the page's script did not run: it shows as it was written");

// None of its source is shown as text, anywhere in the result.
const source = await page.evaluate(id => {
  const r = document.querySelector(`#thread [data-result="${id}"]`);
  const docs = [r, ...[...r.querySelectorAll("iframe.composed[data-shown]")].map(f => f.contentDocument?.body).filter(Boolean)];
  return docs.some(d => /<!doctype html|<script/i.test(d.textContent || ""));
}, task);
if (source) await fail("the page's source is shown as text in the conversation");

// The rest of what was delivered is still drawn beside it.
const stat = await page.waitForFunction(id => {
  const f = document.querySelector(`#thread [data-result="${id}"] iframe.composed[data-shown]`);
  return f?.contentDocument?.querySelector(".stat .sv")?.textContent?.trim() || null;
}, task, { timeout: T }).then(h => h.jsonValue()).catch(() => null);
if (stat !== "19") await fail(`the stat beside the page shows ${JSON.stringify(stat)}`);
await shoot("html-deliverable.png");

if (errors.length) await fail(`page errors: ${errors.join("; ")}`);
await browser.close();
console.log("the delivered page runs live in the conversation");
