// The trash in the installed app: a conversation deleted goes to the trash,
// comes back when restored, and is gone for good when the trash is emptied.
//
// MAGNUS_WEBDRIVER  tauri-driver's address      MAGNUS_APP  the app binary
// MAGNUS_URL        the appliance (https)       MAGNUS_FINGERPRINT  what it printed
// MAGNUS_PROVIDER   the identity provider's name on the sign-in button
// MAGNUS_PERSON     who signs in                MAGNUS_SHOTS  optional: screenshots
import { env, open, pinAndSignIn } from "./appdriver.mjs";

const [wd, app, url, fingerprint, provider, person] =
  ["MAGNUS_WEBDRIVER", "MAGNUS_APP", "MAGNUS_URL", "MAGNUS_FINGERPRINT", "MAGNUS_PROVIDER", "MAGNUS_PERSON"].map(env);
const d = await open(wd, app, process.env.MAGNUS_SHOTS, "trash");

const QUESTION = "Where do walruses live?";
const count = xp => d.run(`return document.evaluate(arguments[0], document, null, XPathResult.ORDERED_NODE_SNAPSHOT_TYPE, null).snapshotLength`, xp);
const gone = async (what, xp) => d.until(what, async () => (await count(xp)) === 0 || null);
const accept = d.answer;
const nav = label => d.click(d.xpath(`//nav//a[normalize-space()='${label}']`));
const row = `//tr[@data-trashed][.//div[@data-title][normalize-space()='${QUESTION}']]`;

await pinAndSignIn(d, { url, fingerprint, provider, person });

await d.step("ask in a new conversation", async () => {
  await nav("New conversation");
  await d.type(d.css("#msg"), QUESTION);
  await d.click(d.css("#send"));
  await d.seen(`Answer to: ${QUESTION}`);
});

await d.step("delete the conversation", async () => {
  await d.click(d.button("Delete conversation"));
  await accept("moves to the trash");
  await d.seen("Moved to the trash.");
  await gone("the deleted conversation", `//*[normalize-space()='Answer to: ${QUESTION}']`);
});

await d.step("restore it from the trash", async () => {
  await nav("Trash");
  await d.xpath(row);
  await d.click(d.button("Restore", row));
  await d.seen("Restored.");
  await gone("the restored row", row);
  // Back in the sidebar; it opens as it was.
  await d.click(d.xpath(`//nav//a[@data-tooltip='${QUESTION}']`));
  await d.seen(`Answer to: ${QUESTION}`);
});

await d.step("delete it again and empty the trash", async () => {
  await d.click(d.button("Delete conversation"));
  await accept("moves to the trash");
  await d.seen("Moved to the trash.");
  // The sidebar is redrawn without it; Trash is clicked after that.
  await gone("the deleted conversation in the sidebar", `//nav//a[@data-tooltip='${QUESTION}']`);
  await nav("Trash");
  await d.xpath(row);
  await d.click(d.button("Empty trash now"));
  await accept("None of it can be restored");
  await d.seen("The trash is empty.");
});

await d.close();
console.log("ok");
