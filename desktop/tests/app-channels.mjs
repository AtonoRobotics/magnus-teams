// Release channels (DECISIONS.md, "Release channels"): the owner's app
// updates from development, pull requests' builds.
//
// MAGNUS_WEBDRIVER  tauri-driver's address      MAGNUS_APP  the app (an AppImage)
// MAGNUS_URL        the appliance (https)       MAGNUS_FINGERPRINT  what it printed
// MAGNUS_PROVIDER   the identity provider's name
// MAGNUS_PERSON     the owner                   MAGNUS_NEW  the build development has
// MAGNUS_SHOTS      optional: where to keep screenshots
import { env, open, pinAndSignIn } from "./appdriver.mjs";

const [wd, app, url, fingerprint, provider, person, fresh] =
  ["MAGNUS_WEBDRIVER", "MAGNUS_APP", "MAGNUS_URL", "MAGNUS_FINGERPRINT", "MAGNUS_PROVIDER", "MAGNUS_PERSON", "MAGNUS_NEW"].map(env);
const d = await open(wd, app, process.env.MAGNUS_SHOTS, "channels");

await pinAndSignIn(d, { url, fingerprint, provider, person });

await d.step("the owner's app gets development's build", async () => {
  await d.click(d.xpath("//nav//a[@aria-label='Settings']"));
  await d.click(d.xpath("//nav[@aria-label='Settings']//a[normalize-space()='Updates']"));
  await d.seen(`Magnus ${fresh} is downloaded and checked.`);
  await d.seen("Sources in every answer");
});

await d.close();
console.log("development followed");
