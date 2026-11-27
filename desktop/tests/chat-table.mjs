// A table in a chat answer, in the real app (tests/scratch/tests/chat_table.rs
// sets it up): each row is as tall as its tallest cell's text, the table fits
// the conversation's width, and a link in a cell reads as its words.
//
// MAGNUS_URL, MAGNUS_PASSWORD  the server and alice's password
// MAGNUS_THREAD                the conversation with the table
// MAGNUS_CHROMIUM              Chrome to drive; MAGNUS_SHOTS  optional
import { createRequire } from "node:module";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { mkdirSync } from "node:fs";

const root = join(dirname(fileURLToPath(import.meta.url)), "../..");
const { chromium } = createRequire(import.meta.url)(join(root, "browser/node_modules/playwright-core"));
const browser = await chromium.launch({ executablePath: process.env.MAGNUS_CHROMIUM, args: ["--no-sandbox", "--disable-dev-shm-usage"] });
const page = await browser.newPage({ viewport: { width: 1344, height: 860 } });
const shots = process.env.MAGNUS_SHOTS;
if (shots) mkdirSync(shots, { recursive: true });
/** @type {string[]} */
const errors = [], failures = [];
page.on("pageerror", e => errors.push(e.message));
const T = 20_000;

try {
  await page.goto(process.env.MAGNUS_URL || "");
  await page.getByLabel("Username").fill("alice");
  await page.getByLabel("Password").fill(process.env.MAGNUS_PASSWORD || "");
  await page.getByRole("button", { name: "Sign in", exact: true }).click();
  await page.locator("#msg").waitFor({ timeout: T });
  for (const mode of ["dark", "light"]) {
    await page.evaluate(m => Theme.set({ style: "nova", mode: m, motion: "reduced" }), mode);
    await page.evaluate(t => { location.hash = `#/chat/${encodeURIComponent(t)}`; }, process.env.MAGNUS_THREAD || "");
    await page.locator("#thread table").first().waitFor({ timeout: T });
    await page.waitForTimeout(1500);
    const m = await page.evaluate(() => {
      const table = /** @type {HTMLTableElement} */ (document.querySelector("#thread table"));
      const wrap = table.parentElement;
      const rows = [...table.querySelectorAll("tbody tr")].map(tr => {
        const cells = [...tr.children].map(td => {
          const range = document.createRange(); range.selectNodeContents(td);
          const s = getComputedStyle(td);
          return { text: Math.round(range.getBoundingClientRect().height), pad: parseFloat(s.paddingTop) + parseFloat(s.paddingBottom), font: s.fontSize };
        });
        return { height: Math.round(tr.getBoundingClientRect().height), cells };
      });
      const chain = [];
      for (let n = /** @type {?Element} */ (table); n && n !== document.body; n = n.parentElement) {
        const s = getComputedStyle(n);
        chain.push(`${n.tagName.toLowerCase()}.${[...n.classList].join(".")} h=${Math.round(n.getBoundingClientRect().height)} disp=${s.display} ws=${s.whiteSpace} ow=${s.overflowWrap} lh=${s.lineHeight} fs=${s.fontSize}`);
      }
      return {
        rows,
        overflow: wrap ? wrap.scrollWidth - wrap.clientWidth : 0,
        links: [...table.querySelectorAll("td")].some(td => /\]\(https?:/.test(td.textContent || "")),
        chain,
        cellCss: (s => `va=${s.verticalAlign} ws=${s.whiteSpace} h=${s.height} lh=${s.lineHeight} disp=${s.display}`)(getComputedStyle(/** @type {Element} */ (table.querySelector("td")))),
      };
    });
    console.log(`${mode}: ${JSON.stringify(m, null, 1)}`);
    m.rows.forEach((r, i) => {
      const tallest = Math.max(...r.cells.map(c => c.text + c.pad));
      if (r.height > tallest + 4) failures.push(`${mode}: row ${i + 1} is ${r.height}px tall for ${tallest}px of text`);
    });
    if (m.overflow > 1) failures.push(`${mode}: the table is ${m.overflow}px wider than the conversation`);
    if (m.links) failures.push(`${mode}: a link in a cell shows as Markdown source`);
    if (shots) {
      await page.screenshot({ path: join(shots, `chat-table-${mode}.png`) });
      await page.locator("#thread table").first().screenshot({ path: join(shots, `chat-table-${mode}-table.png`) });
    }
  }
} catch (e) {
  failures.push(`could not check: ${/** @type {Error} */ (e).message}`);
}
if (errors.length) failures.push(`page errors: ${errors.join("; ")}`);
await browser.close();
if (failures.length) { console.log(failures.map(f => `not ok - ${f}`).join("\n")); process.exit(1); }
console.log("ok - the chat's table is as tall as its rows");
