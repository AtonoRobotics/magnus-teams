// An app the admin added for everyone (in the admin console, never in the
// app: the test adds it through the API), which the owner connects with
// their own account from its card in the Library: the shell opens a loopback address on this computer,
// the app's sign-in page sends its code there, and the shell hands it to the
// appliance. Then the owner lets the worker read the app's events, and
// accepts a change to that tool's definition on the connector.
// Fails if the Connectors page, the Library's card or its connection flow is gone.
//
// MAGNUS_WEBDRIVER  tauri-driver's address      MAGNUS_APP  the app binary
// MAGNUS_URL        the appliance (https)       MAGNUS_FINGERPRINT  what it printed
// MAGNUS_PROVIDER   the identity provider's name on the sign-in button
// MAGNUS_PERSON     the owner                   MAGNUS_APP_URL  the app's MCP address
// MAGNUS_SHOTS      optional: where to keep screenshots
import { env, open, pinAndSignIn } from "./appdriver.mjs";

const [wd, app, url, fingerprint, provider, person, appUrl] =
  ["MAGNUS_WEBDRIVER", "MAGNUS_APP", "MAGNUS_URL", "MAGNUS_FINGERPRINT", "MAGNUS_PROVIDER", "MAGNUS_PERSON", "MAGNUS_APP_URL"].map(env);
const d = await open(wd, app, process.env.MAGNUS_SHOTS, "apps");

await pinAndSignIn(d, { url, fingerprint, provider, person });

await d.step("connect it from its card in the Library; the code comes back to this computer", async () => {
  // The sidebar's link, not the Library's address: setting the hash before
  // the app's router is ready loses to its initial route, and the step then
  // waits for the card on the wrong page (run 42059799939, #200). The link
  // exists only once the app can navigate. (The address was for reaching
  // the Library from Settings, which has no sidebar; the app is added
  // through the API now, so sign-in lands on the workspace with its own.)
  await d.click(d.xpath("//nav//a[normalize-space()='Apps']"));
  // Connect opens the app's page in this computer's browser (the test's, as
  // a person's would): the page approves and sends its code to the shell's
  // one-time loopback address, and the card turns Connected. Nothing else
  // may use that address first.
  await d.click(d.xpath("//*[@data-connector='calendar']//button[starts-with(@aria-label,'Connect ')]"));
  await d.css('[data-connector="calendar"][data-state="connected"]');
  await d.click(d.xpath("//nav//a[@aria-label='Settings']"));
  await d.click(d.xpath("//nav[@aria-label='Settings']//a[normalize-space()='Connectors']"));
});

await d.step("let the worker read events; the rest stays not offered", async () => {
  await d.css('select[data-tool="list_events"]');
  await d.run(`const s = document.querySelector('select[data-tool="list_events"]'); s.value = "read"; s.dispatchEvent(new Event("change", { bubbles: true }));`);
  await d.seen("list_events: reads only.");
});

// The test changes the tool's definition at the app once the worker has used it.
await d.step("a changed tool waits on its connector until the owner accepts it", async () => {
  const card = "//div[@data-app='calendar']//article[contains(@class,'tool_change')]";
  await d.until("the changed tool on its connector", async () => (await d.call("POST", d.s("/elements"), { using: "xpath", value: card })).length > 0 || null, 180_000);
  await d.click(d.button("Accept the change", card));
  await d.until("the change accepted", async () => (await d.call("POST", d.s("/elements"), { using: "xpath", value: card })).length === 0 || null);
});

await d.close();
console.log("apps passed");
