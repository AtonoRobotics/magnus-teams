// Settings in the installed app: every section is there for the owner, and
// each does what it says. Account lists this device and signs another out,
// and points to Admin for roles; a notification choice is kept on the
// appliance; Updates names both versions; Data and privacy keeps the
// feedback choice and sends a deletion request, and points to Admin for
// backups.
//
// MAGNUS_WEBDRIVER  tauri-driver's address      MAGNUS_APP  the app binary
// MAGNUS_URL        the appliance (https)       MAGNUS_FINGERPRINT  what it printed
// MAGNUS_PROVIDER   the identity provider's name on the sign-in button
// MAGNUS_PERSON     the owner                   MAGNUS_OTHER  a colleague
// MAGNUS_SHOTS      optional: where to keep screenshots
import { env, open, pinAndSignIn } from "./appdriver.mjs";

const [wd, app, url, fingerprint, provider, person, other] =
  ["MAGNUS_WEBDRIVER", "MAGNUS_APP", "MAGNUS_URL", "MAGNUS_FINGERPRINT", "MAGNUS_PROVIDER", "MAGNUS_PERSON", "MAGNUS_OTHER"].map(env);
const d = await open(wd, app, process.env.MAGNUS_SHOTS, "settings");
const section = async name => {
  await d.click(d.xpath(`//nav[@aria-label='Settings']//a[normalize-space()='${name}']`));
  await d.xpath(`//h2[normalize-space()='${name}']`);
};
const confirmWith = d.answer;

await pinAndSignIn(d, { url, fingerprint, provider, person });

await d.step("Settings has every section", async () => {
  await d.click(d.xpath("//nav//a[@aria-label='Settings']"));
  const tabs = await d.run("return [...document.querySelectorAll('nav[aria-label=Settings] a')].map(a => a.innerText.trim())");
  // Administration (models, connectors, roles, backups, the kill switch) is Admin's, not Settings'.
  const want = ["Account", "Rules", "Payments", "Appearance", "Notifications", "Updates", "Data and privacy"];
  if (JSON.stringify(tabs) !== JSON.stringify(want)) throw new Error(`the sections are ${tabs}`);
});

// The test signed the owner in once more, elsewhere, before starting the app.
await d.step("Account lists this device and signs the other out", async () => {
  await section("Account");
  await d.xpath("//table[@data-devices]//tr[.//span[normalize-space()='this device']]");
  await d.click(d.xpath("//table[@data-devices]//button[normalize-space()='Sign out']"));
  await d.seen("That device is signed out.");
  await d.until("one device left", async () => (await d.run("return document.querySelectorAll('[data-devices] tr').length")) === 1 || null);
});

await d.step("roles are Admin's: Account points there", async () => {
  await d.xpath("//p[@data-roles-moved]//a[@href='#/admin/people']");
});

await d.step("a notification choice is kept", async () => {
  await section("Notifications");
  await d.click(d.css('input[data-pref="notify_batched"]'));
  await d.seen("Saved.");
});

await d.step("Updates names this app's version and the appliance's", async () => {
  await section("Updates");
  await d.css("[data-app-version] .mono");
  await d.css("[data-appliance-version] .mono");
});

await d.step("Data and privacy keeps the feedback choice and sends a deletion request", async () => {
  await section("Data and privacy");
  await d.click(d.css('input[data-pref="feedback_consent"]'));
  await d.seen("Saved.");
  await d.type(d.css("#delwhat"), "my chats");
  await d.click(d.button("Ask for deletion"));
  await confirmWith("delete my chats");
  await d.xpath("//table[@data-deletions]//td[contains(., 'my chats')]");
  await d.xpath("//p[@data-backups-moved]//a[@href='#/admin/operations']");
});

await d.close();
console.log("settings passed");
