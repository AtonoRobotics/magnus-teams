// Regressions from the review of master's workspace (2026-10-31), in a real
// browser (tests/scratch/tests/workspace_review.rs sets the appliance up):
// - a card shown in two places keeps the focus and the words in the copy
//   being typed in when the other copy is redrawn;
// - a decision in Settings > Connectors is sent once;
// - every conversation in a project sits under it;
// - backups on the server's own disk are said to be there;
// - after a sign-out, the next person's Library is not filtered by the last
//   person's search.
//
// MAGNUS_URL; MAGNUS_PERSON, MAGNUS_PASSWORD (the owner); MAGNUS_OTHER,
// MAGNUS_OTHER_PASSWORD (another person); MAGNUS_ITEM (an open app_tools
// card); MAGNUS_PROJECT_THREADS (conversations in the owner's project);
// MAGNUS_OTHER_FILE (a file of the other person's); MAGNUS_CHROMIUM.
import { createRequire } from "node:module";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "../..");
const { chromium } = createRequire(import.meta.url)(join(root, "browser/node_modules/playwright-core"));
const env = k => { const v = process.env[k]; if (!v) { console.error(`${k} is not set`); process.exit(2); } return v; };
const [base, person, password, other, otherPassword, item, threads, otherFile] =
  ["MAGNUS_URL", "MAGNUS_PERSON", "MAGNUS_PASSWORD", "MAGNUS_OTHER", "MAGNUS_OTHER_PASSWORD", "MAGNUS_ITEM", "MAGNUS_PROJECT_THREADS", "MAGNUS_OTHER_FILE"].map(env);

const browser = await chromium.launch({ executablePath: env("MAGNUS_CHROMIUM"), args: ["--no-sandbox", "--disable-dev-shm-usage"] });
const page = await browser.newPage({ viewport: { width: 1280, height: 860 } });
const errors = [];
page.on("pageerror", e => errors.push(String(e)));
const T = 60_000;
const step = async (name, f) => {
  try { await f(); console.log(`ok   ${name}`); }
  catch (e) { console.error(`FAIL ${name}: ${e.message}`); await browser.close(); process.exit(1); }
};
const signIn = async (who, pw) => {
  await page.getByLabel("Username").fill(who);
  await page.getByLabel("Password").fill(pw);
  await page.getByRole("button", { name: "Sign in" }).click();
  await page.getByRole("link", { name: /^Needs you/ }).waitFor({ timeout: T });
};
const settings = async section => {
  await page.getByRole("link", { name: "Settings" }).click();
  await page.getByRole("navigation", { name: "Settings" }).getByRole("link", { name: section }).click();
};

await step("sign in", async () => { await page.goto(base); await signIn(person, password); });

await step("a card's other copy, redrawn, leaves the one being typed in alone", async () => {
  const r = await page.evaluate(() => {
    const a = document.createElement("div"), b = document.createElement("div");
    document.body.append(a, b);
    const card = extra => `<textarea data-draft="ans-review-test"></textarea>${extra}`;
    paint(a, card("")); paint(b, card(""));
    const typed = a.querySelector("textarea");
    typed.focus(); typed.value = "half an answer";
    paint(b, card("<p>a minute later</p>"));
    const out = { focused: document.activeElement === typed, value: typed.value };
    paint(a, card("<p>redrawn</p>"));
    out.kept = a.querySelector("textarea").value;
    out.stillFocused = document.activeElement === a.querySelector("textarea");
    a.remove(); b.remove();
    return out;
  });
  if (!r.focused) throw new Error("redrawing the other copy took the focus from the one being typed in");
  if (r.kept !== "half an answer" || !r.stillFocused) throw new Error(`the words being typed were lost on a redraw: ${JSON.stringify(r)}`);
});

await step("every conversation in a project sits under it", async () => {
  const nested = await page.locator("nav.side a.conv").count();
  if (nested !== Number(threads)) throw new Error(`${nested} of the project's ${threads} conversations sit under it`);
});

await step("backups on the server's own disk are said to be there", async () => {
  await settings("Data and privacy");
  await page.locator("[data-backup-disk]", { hasText: "server's own disk" }).waitFor({ timeout: T });
});

await step("a decision in Settings > Connectors is sent once", async () => {
  const sent = [];
  page.on("request", r => { if (r.method() === "POST" && r.url().endsWith(`/v1/inbox/${item}`)) sent.push(r.url()); });
  await settings("Connectors");
  await page.locator(`button[data-item="${item}"][data-decision="keep"]`).first().click();
  await page.waitForTimeout(2000);
  if (sent.length !== 1) throw new Error(`the decision was sent ${sent.length} times`);
  if (await page.getByText("already decided").count()) throw new Error("the person was told it was already decided");
});

await step("the next person's Files are theirs, unfiltered", async () => {
  await page.locator("nav.side [data-open=files]").click();
  await page.locator("#lq").fill("no-such-file-anywhere");
  await page.getByText("No file matches.").waitFor({ timeout: T });
  await page.locator("nav.side [data-act=sign-out]").click();
  await signIn(other, otherPassword);
  await page.locator("nav.side [data-open=files]").click();
  await page.locator("[data-lib-file]", { hasText: otherFile }).waitFor({ timeout: T });
});

if (errors.length) { console.error(`FAIL page errors: ${errors.join("; ")}`); process.exit(1); }
await browser.close();
console.log("workspace review done");
