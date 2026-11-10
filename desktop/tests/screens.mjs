// The desktop app's screens, in a browser against a real appliance with a
// real computer. MAGNUS_URL, MAGNUS_TOKEN, MAGNUS_CHROMIUM; MAGNUS_SHOTS optional.
import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { mkdirSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "../..");
const { chromium } = createRequire(import.meta.url)(join(root, "browser/node_modules/playwright-core"));
const env = k => { const v = process.env[k]; if (!v) { console.error(`${k} is not set`); process.exit(2); } return v; };
const [base, token] = ["MAGNUS_URL", "MAGNUS_TOKEN"].map(env), shots = process.env.MAGNUS_SHOTS;
if (shots) mkdirSync(shots, { recursive: true });
const T = 120_000;

const browser = await chromium.launch({ executablePath: env("MAGNUS_CHROMIUM"), args: ["--no-sandbox", "--disable-dev-shm-usage"] });
const context = await browser.newContext({ viewport: { width: 1280, height: 900 }, extraHTTPHeaders: { authorization: `Bearer ${token}` } });
const page = await context.newPage();
page.setDefaultTimeout(T);
const errors = [];
page.on("pageerror", e => errors.push(String(e)));
page.on("dialog", d => d.accept());
let n = 0;
const step = async (name, f) => {
  try { await f(); if (shots) await page.screenshot({ path: join(shots, `${String(++n).padStart(2, "0")}-${name.replaceAll(" ", "-")}.png`) }); console.log(`ok   ${name}`); }
  catch (e) { if (shots) await page.screenshot({ path: join(shots, `FAILED-${name.replaceAll(" ", "-")}.png`) }).catch(() => {}); console.error(`FAIL ${name}: ${e.message}\npage errors: ${errors.join("; ")}`); await browser.close(); process.exit(1); }
};
const go = hash => page.evaluate(h => { location.hash = h; }, hash);
const ask = async text => {
  await go("#/chat/new");
  await page.locator("#msg").fill(text);
  await page.locator("#send").click();
};
// The page's policy forbids waitForFunction.
const until = async fn => {
  for (const end = Date.now() + T; Date.now() < end; await page.waitForTimeout(100)) if (await page.evaluate(fn)) return;
  throw new Error(`timed out: ${fn}`);
};
let computer;

await step("open the app", async () => {
  await page.goto(base);
  await page.getByRole("link", { name: "Computers" }).waitFor();
});

await step("make and start a computer", async () => {
  await page.getByRole("link", { name: "Computers" }).click();
  await page.getByRole("button", { name: "New computer" }).click();
  await page.getByRole("button", { name: "Start" }).click();
  await page.locator("[data-state]", { hasText: "Running" }).waitFor();
  computer = new URL(page.url()).hash.split("/")[2];
});

await step("watch its screen", async () => {
  await until(() => document.querySelector("#preview")?.src.startsWith("data:image"));
});

await step("take control and hand back", async () => {
  await page.getByRole("button", { name: "Take control" }).click();
  await until(() => { const c = document.querySelector("#display"); return c?.width > 0 && c.getContext("2d").getImageData(0, 0, 1, 1).data[3] === 255; });
  await page.locator("#display").click({ position: { x: 50, y: 50 } });
  await page.keyboard.press("a");
  assert.equal(await page.locator("#display-error").isHidden(), true);
  const held = await page.evaluate(id => Magnus.api("GET", `/v1/computers/${id}`), computer);
  assert.ok(held.controller, "nobody holds the screen");
  await page.getByRole("button", { name: "I'm done" }).click();
  await page.getByRole("button", { name: "Take control" }).waitFor();
  const after = await page.evaluate(id => Magnus.api("GET", `/v1/computers/${id}`), computer);
  assert.equal(after.controller, null, "the screen was not handed back");
});

await step("see the takeover in the activity", async () => {
  await page.locator("nav.tabs").getByRole("link", { name: "Activity", exact: true }).click();
  await page.locator("#computer-tab tr").first().waitFor();
});

await step("browse, edit and bulk delete files", async () => {
  const write = (path, text) => page.evaluate(([id, path, text]) => Magnus.api("PUT", `/v1/computers/${id}/files`, { path, data_b64: btoa(text) }), [computer, path, text]);
  await write("/workspace/note.txt", "first");
  await write("/workspace/old.txt", "gone soon");
  await page.locator("nav.tabs").getByRole("link", { name: "Files", exact: true }).click();
  const row = name => page.locator(`tr[data-file="${name}"]`);
  await row("note.txt").locator("[data-browse]").click();
  await page.locator("#file-text").fill("second");
  await page.getByRole("button", { name: "Save", exact: true }).click();
  await row("note.txt").waitFor();
  const raw = await context.request.get(`${base}/v1/computers/${computer}/files/raw?path=${encodeURIComponent("/workspace/note.txt")}`);
  assert.equal(await raw.text(), "second", "the edit was not saved");
  await row("note.txt").locator("[data-pick]").check();
  await row("old.txt").locator("[data-pick]").check();
  await page.getByRole("button", { name: "Delete selected" }).click();
  await row("old.txt").waitFor({ state: "detached" });
  await row("note.txt").waitFor({ state: "detached" });
});

await step("use a team", async () => {
  await page.getByRole("link", { name: "Teams" }).click();
  await page.locator("#team-name").fill("Research");
  await page.getByRole("button", { name: "Create" }).click();
  await page.locator("#channel-name").fill("general");
  await page.getByRole("button", { name: "Add" }).click();
  await page.locator("#say-text").fill("Hello team");
  await page.getByRole("button", { name: "Send" }).click();
  await page.locator("#messages", { hasText: "Hello team" }).waitFor();
});

await step("whisper and post it", async () => {
  await page.locator("#say-text").fill("Say something privately");
  await page.getByRole("button", { name: "Ask my agent privately" }).click();
  // Checking is internal (spec rev 140): no review comes first.
  await page.locator("#whispers", { hasText: "A private answer" }).waitFor();
  assert.equal(await page.locator("#messages", { hasText: "A private answer" }).count(), 0, "the whisper reached the channel");
  await page.getByRole("button", { name: "Post to the channel" }).click();
  await page.locator("#messages", { hasText: "A private answer" }).waitFor();
});

await step("manage members, files and knowledge", async () => {
  await page.locator("nav.tabs").getByRole("link", { name: "Members", exact: true }).click();
  await page.locator("#member-id").fill("bob");
  await page.getByRole("button", { name: "Add" }).click();
  await page.locator('tr[data-member="bob"]').waitFor();
  await page.locator("nav.tabs").getByRole("link", { name: "Files", exact: true }).click();
  await page.locator("#team-upload").setInputFiles({ name: "plan.txt", mimeType: "text/plain", buffer: Buffer.from("the plan") });
  const file = page.locator('tr[data-team-file="plan.txt"]');
  await file.waitFor();
  await file.locator("[data-delete]").click();
  await file.waitFor({ state: "detached" });
  await page.locator("nav.tabs").getByRole("link", { name: "Knowledge", exact: true }).click();
  await page.locator("#entry-text").fill("Fridays are for reviews");
  await page.getByRole("button", { name: "Add" }).click();
  await page.locator("tr[data-entry]", { hasText: "Fridays" }).waitFor();
});

await step("run agent-made html with no network", async () => {
  await ask("Make a page");
  await page.frameLocator("iframe.ui-html").locator("body", { hasText: "script ran" }).waitFor();
});

await step("show a connector's view", async () => {
  await ask("Look at record 42");
  await page.frameLocator("iframe.ui-html").locator("body", { hasText: "view got" }).waitFor();
});

await step("sign in through a secure card", async () => {
  const sent = [];
  await page.route("**/v1/identities/ident/secure-form", r => { sent.push(r.request().postDataJSON()); r.fulfill({ json: { filled: true } }); });
  await go("#/teams");
  await page.getByRole("heading", { name: "Teams" }).waitFor();
  await page.evaluate(() => {
    const item = { id: "sf", kind: "secure_form", status: "open", can_respond: true, summary: "Sign in", details: { identity: "ident", site: "example.test", password_field: "#p", username_field: "#u" } };
    App.inbox.push(item);
    document.querySelector("#main").innerHTML = card(item);
  });
  await page.getByRole("button", { name: "Decline" }).waitFor();
  await page.getByLabel("Username").fill("ada");
  await page.getByLabel("Password").fill("hunter2");
  await page.getByRole("button", { name: "Sign in" }).click();
  await until(() => document.querySelector("#main [data-password]")?.value === "" || !document.querySelector("#main [data-password]"));
  assert.deepEqual(sent.map(s => [s.target, s.value, s.submit]), [["#u", "ada", false], ["#p", "hunter2", true]]);
  await page.unroute("**/v1/identities/ident/secure-form");
  await page.evaluate(() => { App.inbox = App.inbox.filter(i => i.id !== "sf"); });
});

await step("settle what the monitor held", async () => {
  await go("#/inbox");
  const held = page.locator("article.item.monitor");
  await held.waitFor();
  await held.getByRole("button", { name: "Allow" }).click();
  await held.waitFor({ state: "detached" });
});

assert.deepEqual(errors, [], "the page threw");
await browser.close();
console.log("screens ok");
