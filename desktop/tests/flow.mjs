// The operator flow, through the page a person uses: sign in, connect
// Claude, ask for work in a conversation, approve under Needs you, and see
// the outcome with its evidence beside the conversation. It checks behavior,
// not appearance (U7).
//
// MAGNUS_URL       the appliance (its own page, or a page that loads ../ui)
// MAGNUS_PERSON    the person signing in, with either
//   MAGNUS_PASSWORD  their password, or
//   MAGNUS_PROVIDER  the identity provider's name, and
//   MAGNUS_PROVIDER_PASSWORD  their password at the provider (Keycloak): typed
//                    in the page, then, the first time, on the provider's own
//                    page, where they also grant access
// MAGNUS_API_KEY   the key to paste when connecting Claude
// MAGNUS_TASK      the work to ask for in a conversation
// MAGNUS_CHAT_QUESTION, MAGNUS_CHAT_ANSWER  optionally, a question to ask in
//                  chat and the answer the model gives
// MAGNUS_CHROMIUM  Chrome to drive; MAGNUS_SHOTS  where to keep screenshots
import { createRequire } from "node:module";
import { mkdirSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "../..");
const { chromium } = createRequire(import.meta.url)(join(root, "browser/node_modules/playwright-core"));
const env = k => { const v = process.env[k]; if (!v) { console.error(`${k} is not set`); process.exit(2); } return v; };
const [base, person, apiKey, task] = ["MAGNUS_URL", "MAGNUS_PERSON", "MAGNUS_API_KEY", "MAGNUS_TASK"].map(env);
const provider = process.env.MAGNUS_PROVIDER;
const password = provider ? null : env("MAGNUS_PASSWORD");
const providerPassword = provider ? env("MAGNUS_PROVIDER_PASSWORD") : null;
const shots = process.env.MAGNUS_SHOTS;
if (shots) mkdirSync(shots, { recursive: true });

const browser = await chromium.launch({ executablePath: env("MAGNUS_CHROMIUM"), args: ["--no-sandbox", "--disable-dev-shm-usage"] });
const page = await browser.newPage({ viewport: { width: 1200, height: 900 } });
const errors = [];
page.on("pageerror", e => errors.push(String(e)));
let n = 0;
const shot = async name => { if (shots) await page.screenshot({ path: join(shots, `${String(++n).padStart(2, "0")}-${name}.png`), fullPage: true }).catch(e => console.error(`screenshot not kept: ${e.message.split("\n")[0]}`)); };
const step = async (name, f) => {
  try { await f(); await shot(name); console.log(`ok   ${name}`); }
  catch (e) { await shot(`FAILED-${name}`); console.error(`FAIL ${name}: ${e.message}`); await browser.close(); process.exit(1); }
};
const T = 60_000;
// Signed in: the workspace, with Needs you in the sidebar.
const home = () => page.getByRole("link", { name: /^Needs you/ }).waitFor({ timeout: T });
const panel = page.locator("#detail");
const conversations = () => page.locator('#side a[href^="#/chat/"]').count();

if (provider) await step(`sign in with ${provider}`, async () => {
  await page.goto(base);
  const popup = page.waitForEvent("popup", { timeout: T });
  await page.getByLabel("Username").fill(person);
  await page.getByLabel("Password").fill(providerPassword);
  await page.getByRole("button", { name: "Sign in" }).click();
  // The first time, the provider's page opens with the code; the person
  // signs in there and grants access.
  const idp = await popup;
  await idp.waitForLoadState();
  // Magnus opened the provider's page for the code it shows (Keycloak then
  // moves on to its own sign-in page).
  const code = (await page.locator("#code").innerText()).trim();
  const opened = await page.locator("#open").getAttribute("data-url");
  if (!opened.includes(code)) throw new Error(`Magnus opened ${opened}, which is not for code ${code}`);
  await idp.fill("#username", person);
  await idp.fill("#password", providerPassword);
  await idp.click("#kc-login");
  await idp.locator("input[name=accept], button[name=accept]").first().click({ timeout: T });
  await home();
  const page_kept = await page.evaluate(() => JSON.stringify(localStorage) + JSON.stringify(sessionStorage) + document.cookie);
  if (page_kept.includes("mgs_")) throw new Error("the session token reached the page");
});

if (!provider) await step("sign in", async () => {
  await page.goto(base);
  await page.getByLabel("Username").fill(person);
  await page.getByLabel("Password").fill("not the password");
  await page.getByRole("button", { name: "Sign in" }).click();
  await page.getByRole("alert").filter({ hasText: "wrong" }).waitFor({ timeout: T });
  await page.getByLabel("Password").fill(password);
  await page.getByRole("button", { name: "Sign in" }).click();
  await home();
});

await step("connect Claude", async () => {
  await page.getByRole("link", { name: "Settings" }).click();
  await page.getByRole("navigation", { name: "Settings" }).getByRole("link", { name: "Models" }).click();
  const card = page.locator("[data-adapter=anthropic]");
  await card.getByRole("button", { name: "Connect" }).click();
  await card.getByLabel("API key").fill(apiKey);
  await card.getByRole("button", { name: "Save to the vault" }).click();
  await card.getByText("Connected.").waitFor({ timeout: T });
  // The key went to the appliance; the page does not keep it.
  const kept = await page.evaluate(k => JSON.stringify(localStorage).includes(k) || JSON.stringify(sessionStorage).includes(k) || document.cookie.includes(k), apiKey);
  if (kept) throw new Error("the page kept the API key");
  const pick = page.getByLabel("Model");
  await pick.waitFor({ timeout: T });
  await page.getByLabel("Provider").selectOption("anthropic");
  await pick.selectOption({ index: 0 });
  await page.getByRole("button", { name: "Use this model" }).click();
  await page.getByText("New tasks will use this model.").waitFor({ timeout: T });
});

await step("ask for the work in a conversation", async () => {
  await page.getByRole("link", { name: "New conversation", exact: true }).click();
  await page.getByLabel("Model").waitFor({ timeout: T });
  await page.getByRole("textbox", { name: "Message", exact: true }).fill(task);
  await page.getByRole("button", { name: "Send" }).click();
  await page.waitForURL(/#\/chat\/(?!new)/, { timeout: T });
  await page.locator("#thread [data-chat-task]").waitFor({ timeout: T });
});

await step("approve under Needs you", async () => {
  await page.getByRole("link", { name: /^Needs you/ }).click();
  const card = panel.locator("article.item.approval").first();
  await card.waitFor({ timeout: T });
  // The card says exactly what will happen, and that a task waits on it.
  await card.getByText("A task is waiting").waitFor({ timeout: T });
  const what = await card.getByLabel("What will happen").innerText();
  if (!/deliver_file/.test(what)) throw new Error(`the card does not name the action: ${what}`);
  await shot("approval card");
  await card.getByRole("button", { name: "Approve once" }).click();
  await page.getByText("Approved.").waitFor({ timeout: T });
  // A criterion no check can settle comes back as a review card.
  const review = panel.locator("article.item.review").first();
  await review.waitFor({ timeout: T });
  await shot("review card");
  await review.getByRole("button", { name: "Accept" }).click();
  await page.getByText("Accepted.").waitFor({ timeout: T });
});

await step("see the outcome, not how it was checked", async () => {
  await page.locator("#thread [data-chat-task][data-status=completed]").waitFor({ timeout: T });
  await page.locator("#thread [data-chat-task] a.rg-row").click();
  await panel.locator(".detail-head .chip", { hasText: "Done" }).waitFor({ timeout: T });
  // Checking is internal (spec rev 140): no criteria, checks or contract.
  for (const gone of [".verification-detail", "[data-done-line]", ".evidence.pass"]) {
    if (await panel.locator(gone).count()) throw new Error(`the person sees how the work was checked: ${gone}`);
  }
  if (await panel.getByText(/checks verified|appliance-verified|done-criteria/i).count()) throw new Error("the person sees the checks");
  const frame = page.frameLocator("#detail iframe.composed");
  await frame.locator("body").getByText(/\S/).first().waitFor({ timeout: T });
  // The deliverable leads, with the delivered file shown in full, before the
  // summary and the cost.
  await frame.getByText("Q3 revenue: 41.7M, up from 38.2M in Q2.").waitFor({ timeout: T });
  const order = await page.evaluate(() => ["iframe.composed", "[data-summary]", "[data-cost]"].map(q => {
    const el = document.querySelector(`#detail ${q}`);
    return el ? el.getBoundingClientRect().top : null;
  }));
  if (order.some(y => y === null) || order.some((y, i) => i && y <= order[i - 1])) throw new Error(`the outcome is out of order (deliverable, summary, cost): ${order}`);
  // The composed outcome cannot run scripts.
  const sandbox = await panel.locator("iframe.composed").getAttribute("sandbox");
  if (sandbox === null || /allow-scripts/.test(sandbox)) throw new Error(`the outcome frame may run scripts: ${sandbox}`);
  const files = await panel.locator("button[data-file-download]").count();
  if (!files) throw new Error("the delivered file is not offered");
  await shot("outcome");
  // The plan left u1 active; once the task is done, nothing shows as active.
  await panel.getByRole("tab", { name: "Plan" }).click();
  await panel.locator("[data-unit-status]").first().waitFor({ timeout: T });
  await panel.locator("ol .chip", { hasText: "appliance-verified" }).first().waitFor({ timeout: T });
  const states = await panel.locator("[data-unit-status]").evaluateAll(els => els.map(e => e.dataset.unitStatus));
  if (states.some(s => s === "active" || s === "pending")) throw new Error(`a finished task shows plan units as ${states.join(", ")}`);
  await panel.getByRole("tab", { name: "Steps" }).click();
  await panel.locator(".call").first().waitFor({ timeout: T });
});

const [question, answer] = [process.env.MAGNUS_CHAT_QUESTION, process.env.MAGNUS_CHAT_ANSWER];
if (question) await step("ask in a new conversation and see the answer", async () => {
  const answers = n => page.locator("#thread [data-answer]", { hasText: answer }).nth(n - 1).waitFor({ timeout: T });
  const before = await conversations();
  await page.getByRole("link", { name: "New conversation", exact: true }).click();
  await page.waitForURL(/#\/chat\/new$/, { timeout: T });
  await page.locator('.conv-head').getByRole('heading',{name:'New conversation',exact:true}).waitFor({timeout:T});
  await page.getByText("Magnus answers, starts a task, or proposes a schedule").waitFor({ timeout: T });
  if (await page.locator("#thread [data-answer]").count()) throw new Error("a new conversation shows the old one's answers");
  await page.getByLabel("Model").waitFor({ timeout: T });
  const composer=page.getByRole("textbox", { name: "Message", exact: true });
  if(await composer.inputValue()) throw new Error("a new draft retained another conversation's text");
  await composer.fill(question);
  await page.getByRole("button", { name: "Send" }).click();
  await answers(1);
  // The conversation goes on: a reply stays in the same thread.
  await page.getByRole("textbox", { name: "Message", exact: true }).fill("And the quarter before?");
  await page.getByRole("button", { name: "Send" }).click();
  await answers(2);
  await page.waitForFunction(n => document.querySelectorAll('#side a[href^="#/chat/"]').length === n, before + 1, { timeout: T });
  await shot("chat thread");
  // A chat turn is a conversation, not a task.
  await page.getByRole("link", { name: "Activity", exact: true }).click();
  await page.getByRole("navigation", { name: "Activity views" }).getByRole("link", { name: "Tasks", exact: true }).click();
  await panel.locator("section.tsec").first().waitFor({ timeout: T });
  if (await panel.getByText(question).count()) throw new Error("the chat message is listed as a task");
  // Opening Magnus again returns to the latest conversation, not a blank one.
  await page.goto(base);
  await home();
  await answers(2);
});

if (errors.length) { console.error(`FAIL page errors:\n${errors.join("\n")}`); await browser.close(); process.exit(1); }
await browser.close();
console.log("flow passed");
