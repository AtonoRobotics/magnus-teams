// The installed app updates itself from the appliance, in two sessions:
//   MAGNUS_PHASE=before  sign in; the app finds the appliance's newer release,
//                        downloads and checks it, and offers "Restart to
//                        update"; the person restarts. The app replaces
//                        itself and starts again, outside this session.
//   MAGNUS_PHASE=after   the updated app, still signed in, is the new
//                        version; the task that waited through the update is
//                        approved in the inbox and finishes.
//
// MAGNUS_WEBDRIVER  tauri-driver's address      MAGNUS_APP  the app (an AppImage)
// MAGNUS_URL        the appliance (https)       MAGNUS_FINGERPRINT  what it printed
// MAGNUS_PROVIDER   the identity provider's name on the sign-in button
// MAGNUS_PERSON     who signs in                MAGNUS_NEW  the version offered
// MAGNUS_NOTES      after: the release's notes  MAGNUS_SHOTS  optional: where to keep screenshots
import { env, open, pinAndSignIn } from "./appdriver.mjs";

const [wd, app, phase, fresh] = ["MAGNUS_WEBDRIVER", "MAGNUS_APP", "MAGNUS_PHASE", "MAGNUS_NEW"].map(env);
const d = await open(wd, app, process.env.MAGNUS_SHOTS, `update-${phase}`);
const { step, css, xpath, button, click, seen, run } = d;
const version = () => run("return window.__TAURI__.core.invoke('app_info').then(i => i.app)");

if (phase === "before") {
  const [url, fingerprint, provider, person] = ["MAGNUS_URL", "MAGNUS_FINGERPRINT", "MAGNUS_PROVIDER", "MAGNUS_PERSON"].map(env);
  await pinAndSignIn(d, { url, fingerprint, provider, person });
  await step(`the app offers ${fresh}, downloaded and checked`, async () => {
    await seen(`Magnus ${fresh} is ready.`);
    if ((await version()) === fresh) throw new Error("the app is already the new version");
  });
  await step("restart to update", async () => {
    // The app installs and restarts: this session ends with it.
    await click(button("Restart to update")).catch(() => {});
  });
  // The app is gone; end its session so the driver takes the next one.
  await d.close();
  console.log("update offered and started");
} else {
  await step("the updated app is the new version, still signed in", async () => {
    await xpath("//nav//*[self::a or self::button][@aria-label='Needs you' or starts-with(@aria-label,'Needs you,') or starts-with(normalize-space(),'Needs you')]");
    const v = await version();
    if (v !== fresh) throw new Error(`the app is ${v}, not ${fresh}`);
  });
  await step("what's new, in the release's words, once", async () => {
    // The text as written (the heading is shown in capitals).
    const said = await d.until("what's new", () => run(`return document.querySelector('[aria-label="What\\'s new"]')?.textContent`));
    for (const t of [`What's new in Magnus ${fresh}`, env("MAGNUS_NOTES")]) if (!said.includes(t)) throw new Error(`what's new does not say "${t}": ${said}`);
    await click(button("Got it"));
  });
  await step("the task that waited through the update is approved and finishes", async () => {
    await click(xpath("//nav//*[self::a or self::button][@aria-label='Needs you' or starts-with(@aria-label,'Needs you,') or starts-with(normalize-space(),'Needs you')]"));
    const card = "//aside[@id='detail']//article[contains(@class,'approval')]";
    await click(button("Allow", card));
    // The update has just restarted the app. On build-box-beta the rest of
    // the installed-app tests were still up, and 60s was not enough
    // (run 41969612463).
    await seen("Allowed.", 120_000);
  });
  await d.close();
  console.log("update finished");
}
