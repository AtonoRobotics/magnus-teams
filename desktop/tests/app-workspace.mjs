// One workspace, in the installed app: research is started in a
// conversation, approved there, its result read and its file downloaded
// there, and a schedule created and checked there. Deliberately opened work
// creates Back/Forward history while the conversation stays mounted.
// The decisions are the same cards under Needs you, and acting on one in the
// thread updates Needs you.
//
// Ease of use is measured: each flow's steps (a click, a typed entry, a key)
// and navigations are counted and held to ease-budgets.json; a flow that
// takes more than its budget fails, and with it the release.
//
// MAGNUS_WEBDRIVER  tauri-driver's address      MAGNUS_APP  the app binary
// MAGNUS_URL        the appliance (https)       MAGNUS_FINGERPRINT  what it printed
// MAGNUS_PROVIDER   the identity provider's name on the sign-in button
// MAGNUS_PERSON     who signs in
// MAGNUS_RESEARCH   the message that starts the research
// MAGNUS_SCHEDULE   the message that asks for a schedule
// MAGNUS_SHOTS      optional: screenshots
import { readFileSync } from "node:fs";
import { env, open, pinAndSignIn, savedPath } from "./appdriver.mjs";

const [wd, app, url, fingerprint, provider, person, research, schedule] =
  ["MAGNUS_WEBDRIVER", "MAGNUS_APP", "MAGNUS_URL", "MAGNUS_FINGERPRINT", "MAGNUS_PROVIDER", "MAGNUS_PERSON", "MAGNUS_RESEARCH", "MAGNUS_SCHEDULE"].map(env);
const d = await open(wd, app, process.env.MAGNUS_SHOTS, "workspace");
const { step, css, xpath, button, text, run, until } = d;

// Every click, typed entry or key press in a measured flow is one step.
const budgets = JSON.parse(readFileSync(new URL("./ease-budgets.json", import.meta.url), "utf8")).flows;
const used = {};
let flow = null;
const counted = f => async (...a) => { if (flow) used[flow].steps++; return f(...a); };
const click = counted(d.click), type = counted(d.type);
const navigations = () => run("return window.__workspace === 'kept' ? window.__navigations.length : 1000");
// Run `f` as the flow `name`: count its steps and the navigations it caused.
async function measure(name, f) {
  if (!budgets[name]) throw new Error(`no budget for "${name}" in ease-budgets.json`);
  const before = await navigations();
  flow = name; used[name] = { steps: 0, navigations: 0 };
  try { await f(); } finally { flow = null; }
  used[name].navigations = (await navigations()) - before;
}

const thread = "//div[@id='thread']";
const panel = "//aside[@id='detail']";
const task = `${thread}//li[@data-chat-task]`;
const count = () => run("return document.querySelector('[data-needs-count]')?.innerText.trim()");
const present = xp => run("return document.evaluate(arguments[0], document, null, XPathResult.FIRST_ORDERED_NODE_TYPE, null).singleNodeValue !== null", xp);
const gone = (what, xp) => until(what, async () => !(await present(xp)) || null);
const say = async words => {
  await type(css("#msg"), words);
  await click(css("#send"));
};

await pinAndSignIn(d, { url, fingerprint, provider, person });

// From here on, any navigation is counted: a change of address, a history
// step, or a page load (which would lose the mark set on the window).
let address;
await step("start research in a conversation", async () => {
  await run(`window.__workspace = "kept"; window.__navigations = [];
    addEventListener("hashchange", e => __navigations.push("hashchange " + e.newURL));
    addEventListener("popstate", () => __navigations.push("popstate " + location.href));
    const push=history.pushState.bind(history);history.pushState=(state,...args)=>{__navigations.push({kind:'work',work:state?.work});return push(state,...args);};`);
  await measure("start work", async () => {
    await say(research);
    await xpath(`${thread}//div[@data-kind='message']//div[normalize-space()='${research}']`);
    await xpath(task);
  });
  address = await until("the conversation's address", async () => { const h = await run("return location.hash"); return h.startsWith("#/chat/") && h !== "#/chat/new" ? h : null; });
});

let approval;
await step("the task's row shows its live status, and the approval it waits on follows", async () => {
  await xpath(`${task}[@data-status='suspended']//span[contains(@class,'why') and normalize-space()='waiting for an approval']`);
  await xpath(`${task}[@data-status='suspended']//span[contains(@class,'chip') and normalize-space()='Needs you']`);
  const id = await d.attr(xpath(task), "data-chat-task");
  const card = `${thread}//div[@data-for-task='${id}']//article[contains(@class,'approval')]`;
  approval = await d.attr(xpath(card), "data-id");
  // New cards rise in; the text is read once the card is shown.
  const what = await until("the card shown", async () => (await text(xpath(`${card}//div[@aria-label='What will happen']`))) || null);
  if (!what.includes("deliver_file")) throw new Error(`the card does not name the action: ${what}`);
});

await step("Needs you counts it and holds the same card", async () => {
  await until("Needs you to count 1", async () => (await count()) === "1" || null);
  await click(xpath("//nav//a[contains(@class,'needs')]"));
  await xpath(`${panel}//article[@data-id='${approval}']`);
});

await step("approve in the conversation; Needs you follows", async () => {
  await measure("approve", async () => {
    await click(button("Allow", `${thread}//article[@data-id='${approval}']`));
    await d.seen("Allowed.");
  });
  await gone("the approval in the thread", `${thread}//article[@data-id='${approval}']`);
  await gone("the approval under Needs you", `${panel}//article[@data-id='${approval}']`);
});

await step("the outcome completes with nothing more to decide", async () => {
  // Checking is internal (spec rev 140): no review card follows the work.
  await until("Needs you to count 0", async () => (await count()) === "0" || null);
  await xpath(`${task}[@data-status='completed']`);
});

await step("read the result beside the conversation", async () => {
  const result = `${thread}//div[@data-result]`;
  await xpath(`${result}//span[contains(@class,'chip') and normalize-space()='Done']`);
  await until("the file's preview in the thread", () => run("const p = document.querySelector('#thread [data-result] [data-preview] .glimpse pre'); return p && p.innerText.includes('41.7M')"));
  await measure("read a result", async () => {
    await click(xpath(`${result}//a[normalize-space()='See the outcome']`));
    await until("the composed outcome", () => run("const f = document.querySelector('#detail iframe.composed'); return f && f.contentDocument && f.contentDocument.body.innerText.includes('41.7M')"));
  });
  // The panel leads with the deliverable, under the task's title. Checking
  // is internal (spec rev 140): no criteria or checks are shown.
  await xpath(`${panel}//div[contains(@class,'detail-head')]//h2[normalize-space()='Q3 revenue summary']`);
  await xpath(`${panel}//div[contains(@class,'seg')]//button[@aria-selected='true' and normalize-space()='Result']`);
  const shown = await run("return !!document.querySelector('#detail .checks, #detail details.verification-detail, #detail .evidence.pass')");
  if (shown) throw new Error("the task view shows its checks to the person");
});

await step("download the file from the conversation", async () => {
  const body = readFileSync(await savedPath(d, () => click(xpath(`${thread}//div[@data-result]//button[normalize-space()='Download']`))), "utf8");
  if (!body.includes("41.7M")) throw new Error(`the downloaded file holds ${JSON.stringify(body)}`);
});

await step("find the file by searching", async () => {
  await measure("find a file", async () => {
    await type(css("#q"), "summary");
    await type(css("#q"), "\uE007");
    await click(xpath(`${panel}//li[@data-lib-file]//a[normalize-space()='summary.txt']`));
    await xpath(`${panel}//h2[contains(@class,'ptitle') and normalize-space()='summary.txt']`);
    await xpath(`${panel}//button[normalize-space()='Download']`);
  });
});

await step("create a schedule in the conversation", async () => {
  await measure("create a schedule", async () => {
    await say(schedule);
    // Asking set it up: the card shows it scheduled, with nothing to confirm.
    const card = `${thread}//div[@data-card]`;
    await xpath(`${card}//span[contains(@class,'chip') and normalize-space()='Scheduled']`);
  });
});

await step("check the schedule's status in the conversation and beside it", async () => {
  const card = `${thread}//div[@data-card]`;
  let seen = "";
  const said = await until("the schedule's next run", async () => { seen = await run("return document.evaluate(arguments[0], document, null, XPathResult.FIRST_ORDERED_NODE_TYPE, null).singleNodeValue?.innerText ?? ''", `${card}//span[@data-last]`); return seen.startsWith("Next run") ? seen : null; }, 15_000).catch(e => { throw new Error(`${e.message}; it says "${seen}"`); });
  if (!said.includes("No results yet")) throw new Error(`the new schedule says ${said}`);
  await click(xpath(`${card}//a[contains(@class,'rg-row')]`));
  await xpath(`${panel}//div[@data-schedule]//div[@data-last][starts-with(normalize-space(),'Next run')]`);
  await xpath(`${panel}//button[normalize-space()='Stop this schedule']`);
});

await step("work history preserves the conversation and page", async () => {
  const [kept, navigations, now] = await run("return [window.__workspace, window.__navigations, location.hash]");
  if (kept !== "kept") throw new Error("the page was loaded again");
  if(navigations.some(n=>typeof n!=='object' || n.kind!=='work' || !n.work))throw new Error(`The conversation navigated: ${JSON.stringify(navigations)}`);
  if(navigations.length<4)throw new Error('Deliberately opened work did not create navigable history');
  if (now !== address) throw new Error(`the address changed from ${address} to ${now}`);
  await xpath(`${thread}//div[@data-kind='message']//div[normalize-space()='${research}']`);
});

await step("each flow is within its ease-of-use budget", async () => {
  const over = [];
  for (const [name, b] of Object.entries(budgets)) {
    const u = used[name];
    if (!u) { over.push(`${name}: not measured`); continue; }
    console.log(`     ${name}: ${u.steps} steps (budget ${b.steps}), ${u.navigations} navigations (budget ${b.navigations})`);
    if (u.steps > b.steps) over.push(`${name} took ${u.steps} steps; its budget is ${b.steps}`);
    if (u.navigations > b.navigations) over.push(`${name} navigated ${u.navigations} times; its budget is ${b.navigations}`);
  }
  if (over.length) throw new Error(`ease of use regressed: ${over.join("; ")}`);
});

await d.close();
console.log("workspace passed");
