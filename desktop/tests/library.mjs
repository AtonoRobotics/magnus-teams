// The library and projects, as a person uses them in the app: make a
// project with instructions, upload a file into it, start a task there,
// see its output in full on the outcome page and in the library under the
// project, search the library, download a file, open files, and start a chat in the
// project with a file attached.
//
// MAGNUS_URL, MAGNUS_PERSON, MAGNUS_PASSWORD  where and who
// MAGNUS_PRICES        the CSV to upload; MAGNUS_INSTRUCTIONS  the project's instructions
// MAGNUS_CHROMIUM  Chrome to drive; MAGNUS_SHOTS  where to keep screenshots
import { createRequire } from "node:module";
import { mkdirSync, readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "../..");
const { chromium } = createRequire(import.meta.url)(join(root, "browser/node_modules/playwright-core"));
const env = k => { const v = process.env[k]; if (!v) { console.error(`${k} is not set`); process.exit(2); } return v; };
const [base, person, password, prices, instructions] = ["MAGNUS_URL", "MAGNUS_PERSON", "MAGNUS_PASSWORD", "MAGNUS_PRICES", "MAGNUS_INSTRUCTIONS"].map(env);
const shots = process.env.MAGNUS_SHOTS;
if (shots) mkdirSync(shots, { recursive: true });
const win = JSON.parse(readFileSync(join(root, "desktop/src-tauri/tauri.conf.json"), "utf8")).app.windows[0];

const browser = await chromium.launch({ executablePath: env("MAGNUS_CHROMIUM"), args: ["--no-sandbox", "--disable-dev-shm-usage"] });
const page = await browser.newPage({ viewport: { width: win.width, height: win.height }, acceptDownloads: true });
const errors = [];
page.on("pageerror", e => errors.push(String(e)));
let n = 0;
const shot = async name => { if (shots) await page.screenshot({ path: join(shots, `library-${String(++n).padStart(2, "0")}-${name}.png`), fullPage: true }).catch(e => console.error(`screenshot not kept: ${e.message.split("\n")[0]}`)); };
const step = async (name, f) => {
  try { await f(); await shot(name); console.log(`ok   ${name}`); }
  catch (e) { await shot(`FAILED-${name}`); console.error(`FAIL ${name}: ${e.message}`); await browser.close(); process.exit(1); }
};
const T = 60_000;
const until = async (what, f) => {
  for (const end = Date.now() + T; Date.now() < end; await page.waitForTimeout(200)) if (await f()) return;
  throw new Error(`timed out waiting for ${what}`);
};

await step("sign in", async () => {
  await page.goto(base);
  await page.getByLabel("Username").fill(person);
  await page.getByLabel("Password").fill(password);
  await page.getByRole("button", { name: "Sign in" }).click();
  // Signed in: the workspace, with Needs you in the sidebar.
  await page.getByRole("link", { name: /^Needs you/ }).waitFor({ timeout: T });
});

await step("make a project with instructions", async () => {
  // Projects, and a new one, open beside the conversation.
  await page.getByRole("link", { name: "All", exact: true }).click();
  await page.getByLabel("Name").fill("Kitchen remodel");
  await page.getByLabel(/Instructions for everything in it/).fill(instructions);
  await page.getByRole("button", { name: "Create" }).click();
  await page.getByRole("heading", { name: "Kitchen remodel" }).waitFor({ timeout: T });
  if (await page.locator("#pin").inputValue() !== instructions) throw new Error("the instructions were not kept");
});

await step("upload a file into the project", async () => {
  await page.locator("#detail input[data-attach]").setInputFiles({ name: "prices.csv", mimeType: "text/csv", buffer: Buffer.from(prices) });
  await page.locator("[data-project-file]", { hasText: "prices.csv" }).waitFor({ timeout: T });
});

await step("ask for work in the project and see its output in full", async () => {
  await page.getByRole("link", { name: "New conversation in this project" }).click();
  await page.locator(".conv-head").getByRole("link", { name: "Kitchen remodel" }).waitFor({ timeout: T });
  await page.getByRole("textbox", { name: "Message", exact: true }).fill("Total the prices in prices.csv.");
  await page.getByRole("button", { name: "Send" }).click();
  await page.locator("#thread [data-chat-task][data-status=completed]").waitFor({ timeout: T });
  await page.locator("#thread [data-chat-task] a.rg-row").click();
  await page.locator("#detail .detail-head .chip", { hasText: "Done" }).waitFor({ timeout: T });
  // The delivered report is in the panel, drawn as a table, without opening it.
  const frame = page.frameLocator("#detail iframe.composed");
  await frame.getByText("Total: 42 EUR").waitFor({ timeout: T });
  await frame.locator("td", { hasText: "widget" }).waitFor({ timeout: T });
});

await step("find the output in Files under the project", async () => {
  await page.getByRole("link", { name: "Files", exact: true }).click();
  await page.getByLabel("Filter by project").selectOption({ label: "Kitchen remodel" });
  const row = page.locator("[data-lib-file]", { hasText: "report.md" });
  await row.waitFor({ timeout: T });
  await row.getByText("Delivered by Magnus").waitFor({ timeout: T });
  await row.getByRole("link", { name: "Total the prices" }).waitFor({ timeout: T });
  if (await row.locator("select[data-move]").inputValue() === "") throw new Error("the output is not under the project");
  await page.locator("[data-lib-file]", { hasText: "prices.csv" }).getByText("Uploaded by you").waitFor({ timeout: T });
});

await step("search the library by content and download a file", async () => {
  await page.getByLabel("Filter by project").selectOption({ label: "All files" });
  await page.getByLabel("Search files").fill("bolt");
  await until("two matches", async () => await page.locator("[data-lib-file]").count() === 2);
  await page.getByLabel("Search files").fill("nothing-matches-this");
  await page.getByText("No file matches.").waitFor({ timeout: T });
  await page.getByLabel("Search files").fill("");
  const [download] = await Promise.all([page.waitForEvent("download", { timeout: T }), page.locator("[data-lib-file]", { hasText: "report.md" }).getByRole("button", { name: "Download report.md" }).click()]);
  if (download.suggestedFilename() !== "report.md") throw new Error(`downloaded ${download.suggestedFilename()}`);
});

await step("open files from the library", async () => {
  // A delivered report opens beside the conversation, drawn in full, with
  // where it came from and a way to save it.
  await page.locator("[data-lib-file]", { hasText: "report.md" }).getByRole("link", { name: "report.md" }).click();
  await page.getByRole("heading", { name: "report.md" }).waitFor({ timeout: T });
  const frame = page.frameLocator("#detail iframe.composed");
  await frame.getByText("Total: 42 EUR").waitFor({ timeout: T });
  await frame.getByRole("cell", { name: "widget", exact: true }).waitFor({ timeout: T });
  await page.locator("#detail").getByRole("link", { name: "Total the prices" }).waitFor({ timeout: T });
  const [download] = await Promise.all([page.waitForEvent("download", { timeout: T }), page.locator("#detail").getByRole("button", { name: "Download report.md", exact: true }).click()]);
  if (download.suggestedFilename() !== "report.md") throw new Error(`downloaded ${download.suggestedFilename()}`);
  // An uploaded CSV opens as a table.
  await page.getByRole("link", { name: "Files", exact: true }).first().click();
  await page.locator("[data-lib-file]", { hasText: "prices.csv" }).getByRole("link", { name: "prices.csv" }).click();
  await page.getByRole("heading", { name: "prices.csv" }).waitFor({ timeout: T });
  await page.frameLocator("#detail iframe.composed").getByRole("cell", { name: "bolt", exact: true }).waitFor({ timeout: T });
  await page.locator("#detail").getByText("Uploaded by you").waitFor({ timeout: T });
});

await step("the task is listed in Tasks", async () => {
  await page.getByRole("link", { name: "Activity", exact: true }).click();
  await page.getByRole("navigation", { name: "Activity views" }).getByRole("link", { name: "Tasks", exact: true }).click();
  // Under the request that started it, a conversation; done work is folded.
  const group = page.locator("#detail .tgroup", { hasText: "Total the prices" });
  await group.locator("summary").click();
  await group.locator(".trow", { hasText: "Total the prices" }).waitFor({ timeout: T });
});

await step("chat in the project with a file attached", async () => {
  await page.getByRole("link", { name: "All", exact: true }).click();
  await page.locator(".plist").getByRole("link", { name: /Kitchen remodel/ }).click();
  await page.getByRole("heading", { name: "Kitchen remodel" }).waitFor({ timeout: T });
  await page.getByRole("link", { name: "New conversation in this project" }).click();
  await page.locator(".conv-head").getByRole("heading", { name: "New conversation" }).waitFor({ timeout: T });
  await page.locator(".conv-head").getByRole("link", { name: "Kitchen remodel" }).waitFor({ timeout: T });
  await page.locator("#say input[data-attach]").setInputFiles({ name: "notes.txt", mimeType: "text/plain", buffer: Buffer.from("Bathroom tiles next.") });
  await page.locator("#pending").getByText("notes.txt").waitFor({ timeout: T });
  await page.getByRole("textbox", { name: "Message", exact: true }).fill("Keep these notes for later.");
  await page.getByRole("button", { name: "Send" }).click();
  await page.waitForURL(/#\/chat\/thread_/, { timeout: T });
  await page.locator(".msg.me").getByRole("link", { name: "notes.txt" }).waitFor({ timeout: T });
  await page.locator("[data-answer]", { hasText: "Noted." }).waitFor({ timeout: T });
  await page.locator(".conv-head").getByRole("link", { name: "Kitchen remodel" }).waitFor({ timeout: T });
});

if (errors.length) { console.error(`page errors: ${errors.join("; ")}`); process.exit(1); }
await browser.close();
