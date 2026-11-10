// The real desktop app, driven through tauri-driver (W3C WebDriver): connect
// to the appliance over HTTPS and pin its certificate, sign in through the
// identity provider, connect Claude, ask for work in a conversation, approve
// under Needs you, and see the outcome with its evidence beside the
// conversation. Behavior, not appearance (U7).
//
// MAGNUS_WEBDRIVER  tauri-driver's address      MAGNUS_APP  the app binary
// MAGNUS_URL        the appliance (https)       MAGNUS_FINGERPRINT  what it printed
// MAGNUS_PROVIDER   the identity provider's name on the sign-in button
// MAGNUS_PERSON  MAGNUS_API_KEY  MAGNUS_TASK
// MAGNUS_SHOTS      optional: where to keep screenshots
import { readFileSync } from "node:fs";
import { env, open, pinAndSignIn, savedPath } from "./appdriver.mjs";

const [wd, app, url, fingerprint, provider, person, apiKey, task] =
  ["MAGNUS_WEBDRIVER", "MAGNUS_APP", "MAGNUS_URL", "MAGNUS_FINGERPRINT", "MAGNUS_PROVIDER", "MAGNUS_PERSON", "MAGNUS_API_KEY", "MAGNUS_TASK"].map(env);
const d = await open(wd, app, process.env.MAGNUS_SHOTS, "app");
const { step, css, xpath, button, click, type, text, seen, run, until } = d;

await pinAndSignIn(d, { url, fingerprint, provider, person });

await step("connect Claude", async () => {
  await click(xpath("//nav//a[@aria-label='Settings']"));
  await click(xpath("//nav[@aria-label='Settings']//a[normalize-space()='Models']"));
  const card = "//div[@data-adapter='anthropic']";
  await click(button("Connect", card));
  await type(css("#key-anthropic"), apiKey);
  await click(button("Save to the vault", card));
  await seen("Connected.");
  await css("#mname");
  await click(button("Use this model"));
  await seen("New tasks will use this model.");
});

await step("ask for the work in a conversation", async () => {
  await click(xpath("//nav//a[normalize-space()='New conversation']"));
  await type(css("#msg"), task);
  await click(css("#send"));
  await xpath("//div[@id='thread']//li[@data-chat-task]");
});

await step("approve under Needs you", async () => {
  await click(xpath("//nav//*[self::a or self::button][@aria-label='Needs you' or starts-with(@aria-label,'Needs you,') or starts-with(normalize-space(),'Needs you')]"));
  const card = "//aside[@id='detail']//article[contains(@class,'approval')]";
  await xpath(`${card}//*[contains(., 'A task is waiting')]`);
  // The panel fades in; its text is read once it is shown.
  const what = await until("the card shown", async () => (await text(xpath(`${card}//div[@aria-label='What will happen']`))) || null);
  if (!what.includes("deliver_file")) throw new Error(`the card does not name the action: ${what}`);
  await click(button("Allow", card));
  await seen("Allowed.");
});

await step("see the outcome beside the conversation, with no checks shown", async () => {
  await click(xpath("//div[@id='thread']//li[@data-chat-task][@data-status='completed']//a"));
  await xpath("//aside[@id='detail']//div[contains(@class,'detail-head')]//span[contains(@class,'chip') and normalize-space()='Done']");
  // Checking is internal (spec rev 140): the person sees the result, not its criteria.
  if (await run("return !!document.querySelector('#detail .checks, #detail details.verification-detail, #detail .evidence.pass')")) throw new Error("the task view shows its checks to the person");
  const sandbox = await run("return document.querySelector('iframe.composed')?.getAttribute('sandbox')");
  if (sandbox === null || /allow-scripts/.test(sandbox)) throw new Error(`the outcome frame may run scripts: ${sandbox}`);
  await until("the composed outcome", () => run("const f = document.querySelector('iframe.composed'); return f && f.contentDocument && f.contentDocument.body.innerText.includes('41.7M')"));
});

await step("download the delivered file", async () => {
  const body = readFileSync(await savedPath(d, () => click(css("#detail .frow button[data-file-download]"))), "utf8");
  if (!body.includes("41.7M")) throw new Error(`the downloaded file holds ${JSON.stringify(body)}`);
});

await d.close();
console.log("app flow passed");
