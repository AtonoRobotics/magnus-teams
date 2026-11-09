// Read aloud in the installed app, in the appliance's own voice: the answer's
// listening script is made a sentence at a time and played to the end; Stop
// ends it at once and nothing more is made.
//
// MAGNUS_WEBDRIVER  tauri-driver's address      MAGNUS_APP  the app binary
// MAGNUS_URL        the appliance (https)       MAGNUS_FINGERPRINT  what it printed
// MAGNUS_PROVIDER   the identity provider's name on the sign-in button
// MAGNUS_PERSON     who signs in                MAGNUS_SHOTS  optional: screenshots
// MAGNUS_SPOKEN_LOG one line per text the appliance's voice was asked to read
// MAGNUS_SENTENCES  how many sentences the script has
// MAGNUS_SECONDS    how long the voice speaks each one
import { readFileSync } from "node:fs";
import { env, open, pinAndSignIn } from "./appdriver.mjs";

const [wd, app, url, fingerprint, provider, person, log] =
  ["MAGNUS_WEBDRIVER", "MAGNUS_APP", "MAGNUS_URL", "MAGNUS_FINGERPRINT", "MAGNUS_PROVIDER", "MAGNUS_PERSON", "MAGNUS_SPOKEN_LOG"].map(env);
const sentences = Number(env("MAGNUS_SENTENCES")), seconds = Number(env("MAGNUS_SECONDS"));
const d = await open(wd, app, process.env.MAGNUS_SHOTS, "read-aloud");

const read = () => { try { return readFileSync(log, "utf8").split("\n").filter(Boolean); } catch { return []; } };
const listen = () => d.xpath("//div[@data-kind='answer']//div[@role='group']//button[@aria-label='Read aloud']");
const busy = () => d.run("return !!document.querySelector('[data-msg-act=hush]')");

await pinAndSignIn(d, { url, fingerprint, provider, person });

await d.step("an answer", async () => {
  // The conversation is home: signing in opens a new one.
  await d.type(d.css("#msg"), "How did revenue go?");
  await d.click(d.css("#send"));
  await d.xpath("//div[@data-kind='answer']");
});

await d.step("read to the end, a sentence at a time", async () => {
  const start = Date.now();
  await d.click(listen());
  await d.until("reading to start", async () => (await busy()) || null);
  await d.until("reading to end", async () => !(await busy()) || null, 60000 + sentences * seconds * 1000);
  const took = (Date.now() - start) / 1000;
  if (took < sentences * seconds * 0.9) throw new Error(`done in ${took}s; ${sentences} sentences of ${seconds}s were not all heard`);
  if (await d.run("return document.body.innerText.includes('Could not play')")) throw new Error("the audio did not play");
  if (read().length !== sentences) throw new Error(`the voice read ${JSON.stringify(read())}`);
});

await d.step("Stop ends it and nothing more is made", async () => {
  await d.click(listen());
  await d.until("the first sentence", () => read().length > sentences || null);
  await d.click(d.xpath("//div[@data-kind='answer']//div[@role='group']//button[@aria-label='Stop reading']"));
  await d.until("reading to stop", async () => !(await busy()) || null, seconds * 1000);
  const made = read().length;
  await new Promise(r => setTimeout(r, sentences * seconds * 1000));
  if (read().length !== made || made > sentences + 2) throw new Error(`after Stop the voice read ${read().length - sentences} sentences`);
});

await d.close();
console.log("read aloud passed");
