// Two people on one installed app, one after the other: the owner signs in
// (the first time on the identity provider's page) and sees the owner's
// controls and their task; they sign out (the shell forgets the session) and
// a colleague signs in on the same machine, with only their own rights: no
// Models in Settings, no kill switch, not the owner's task. Then the owner
// signs in again with the username and password in the app alone.
//
// MAGNUS_WEBDRIVER  tauri-driver's address      MAGNUS_APP  the app binary
// MAGNUS_URL        the appliance (https)       MAGNUS_FINGERPRINT  what it printed
// MAGNUS_PROVIDER   the identity provider's name
// MAGNUS_PERSON     the owner                   MAGNUS_OTHER  the colleague
// MAGNUS_TASK_TITLE the owner's task, by title
// MAGNUS_SHOTS      optional: where to keep screenshots
import { env, open, pinAndSignIn, signIn } from "./appdriver.mjs";

const [wd, app, url, fingerprint, provider, person, other, title] =
  ["MAGNUS_WEBDRIVER", "MAGNUS_APP", "MAGNUS_URL", "MAGNUS_FINGERPRINT", "MAGNUS_PROVIDER", "MAGNUS_PERSON", "MAGNUS_OTHER", "MAGNUS_TASK_TITLE"].map(env);
const d = await open(wd, app, process.env.MAGNUS_SHOTS, "people");
const shows = t => d.run("return document.body.innerText.includes(arguments[0])", t);

await pinAndSignIn(d, { url, fingerprint, provider, person });

await d.step("the owner sees the owner's controls and their task", async () => {
  await d.click(d.xpath("//nav//a[@aria-label='Settings']"));
  await d.xpath("//nav[@aria-label='Settings']//a[normalize-space()='Models']");
  await d.click(d.xpath("//nav[@aria-label='Settings']//a[normalize-space()='Kill switch']"));
  await d.css('[data-kill-switch="off"]');
  // Activity exposes the labeled Tasks view beside the conversation.
  await d.click(d.xpath("//nav//a[@aria-label='Activity']"));
  await d.click(d.xpath("//nav[@aria-label='Activity views']//a[normalize-space()='Tasks']"));
  await d.xpath("//aside[@id='detail']//h2[normalize-space()='Tasks']");
  await d.seen(title);
});

await d.step("sign out; the next person keeps the pinned appliance", async () => {
  await d.click(d.button("Sign out"));
  await d.css("#addr");
  const kept = await d.run("return document.querySelector('#addr')?.value");
  if (kept !== url) throw new Error(`the address after signing out is ${kept}`);
  await d.click(d.button("Continue"));
  await d.css("#who");
  await d.button("Use a passkey instead");
  if (await d.run("return !!document.querySelector('#fp')")) throw new Error("the pinned certificate was asked for again");
});

await signIn(d, { provider, person, as: other });

await d.step("the colleague has only their own rights", async () => {
  const who = await d.attr(d.css("nav .who .whoami [data-tooltip]"), "data-tooltip");
  if (who !== other) throw new Error(`signed in as ${who}`);
  if (await d.run("return !!document.querySelector('[data-kill-switch]')")) throw new Error("a colleague sees the kill switch");
  await d.click(d.xpath("//nav//a[@aria-label='Settings']"));
  await d.xpath("//nav[@aria-label='Settings']//a[normalize-space()='Account']");
  if (await d.run("return [...document.querySelectorAll('nav a')].some(a => ['Models', 'Connectors', 'Kill switch'].includes(a.innerText.trim()))")) throw new Error("a colleague sees Models, Connectors or the kill switch");
  await d.click(d.xpath("//nav//a[@aria-label='Activity']"));
  await d.click(d.xpath("//nav[@aria-label='Activity views']//a[normalize-space()='Tasks']"));
  await d.xpath("//aside[@id='detail']//h2[normalize-space()='Tasks']");
  await new Promise(r => setTimeout(r, 1500));
  if (await shows(title)) throw new Error("a colleague sees the owner's task");
});

await d.step("sign out again", async () => {
  await d.click(d.button("Sign out"));
  await d.click(d.button("Continue"));
  await d.css("#who");
});

await signIn(d, { provider, person, browser: false });

await d.close();
console.log("people passed");
