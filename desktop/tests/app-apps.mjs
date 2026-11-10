// An app the admin added for everyone (in the admin console, never in the
// app: the test adds it through the API), which the owner connects with
// their own account from its card in the Library: connecting shows the one
// connect screen (what Magnus can read, change and send, and the risks), and
// accepting it opens a loopback address on this computer, the app's sign-in
// page sends its code there, and the shell hands it to the appliance. No
// tool is reviewed on its own (spec rev 140). When the app later adds tools
// that change or send, one card for the app waits on its connector, and the
// owner keeps them.
// Fails if the Connectors page, the Library's card, the connect screen or
// its connection flow is gone.
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

await d.step("connect it from its card in the Library, through its connect screen; the code comes back to this computer", async () => {
  // The sidebar's link, not the Library's address: setting the hash before
  // the app's router is ready loses to its initial route, and the step then
  // waits for the card on the wrong page (run 42059799939, #200). The link
  // exists only once the app can navigate.
  await d.click(d.xpath("//nav//a[normalize-space()='Apps']"));
  await d.click(d.xpath("//*[@data-connector='calendar']//button[starts-with(@aria-label,'Connect ')]"));
  // The one screen: what Magnus can read, what it can change and send, the
  // risks. Accepting it is the consent; nothing per tool follows.
  const screen = "//dialog//*[@data-connect='calendar']";
  await d.seen("What Magnus can read");
  await d.seen("What it can change and send");
  await d.seen("The risks");
  await d.click(d.xpath(`${screen}//button[@data-connect-answer='yes']`));
  // Connect opens the app's page in this computer's browser (the test's, as
  // a person's would): the page approves and sends its code to the shell's
  // one-time loopback address, and the card turns Connected.
  await d.css('[data-connector="calendar"][data-state="connected"]');
  await d.click(d.xpath("//nav//a[@aria-label='Settings']"));
  await d.click(d.xpath("//nav[@aria-label='Settings']//a[normalize-space()='Connectors']"));
});

// The test has the app add tools that change or send once the worker has used it.
await d.step("one card for the app's new tools waits on its connector until the owner keeps them", async () => {
  const card = "//div[@data-app='calendar']//article[contains(@class,'app_tools')]";
  await d.until("the app's card on its connector", async () => (await d.call("POST", d.s("/elements"), { using: "xpath", value: card })).length > 0 || null, 180_000);
  await d.click(d.button("Keep it", card));
  await d.until("the card answered", async () => (await d.call("POST", d.s("/elements"), { using: "xpath", value: card })).length === 0 || null);
});

await d.close();
console.log("apps passed");
