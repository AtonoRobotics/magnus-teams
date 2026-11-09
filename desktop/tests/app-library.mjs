// The installed app downloads a file from the library: the shell fetches its
// bytes and saves them in the person's Downloads folder. A browser test cannot
// see this; the shell's own file fetch refused library files until 0.4.1.
//
// MAGNUS_WEBDRIVER  tauri-driver's address      MAGNUS_APP  the app binary
// MAGNUS_URL        the appliance (https)       MAGNUS_FINGERPRINT  what it printed
// MAGNUS_PROVIDER   the identity provider's name on the sign-in button
// MAGNUS_PERSON
// MAGNUS_FILE       the library file's name     MAGNUS_FILE_TEXT  its content
// MAGNUS_SHOTS      optional: where to keep screenshots
import { readFileSync } from "node:fs";
import { env, open, pinAndSignIn, savedPath } from "./appdriver.mjs";

const [wd, app, url, fingerprint, provider, person, name, content] =
  ["MAGNUS_WEBDRIVER", "MAGNUS_APP", "MAGNUS_URL", "MAGNUS_FINGERPRINT", "MAGNUS_PROVIDER", "MAGNUS_PERSON", "MAGNUS_FILE", "MAGNUS_FILE_TEXT"].map(env);
const d = await open(wd, app, process.env.MAGNUS_SHOTS, "library");

await pinAndSignIn(d, { url, fingerprint, provider, person });

await d.step("download a file from the library", async () => {
  // Files open beside the conversation.
  await d.click(d.xpath("//nav//a[normalize-space()='Files']"));
  const row = `//aside[@id='detail']//li[@data-lib-file][.//*[normalize-space()='${name}']]`;
  // The shell says where it saved the file, or why it could not; a toast
  // with no words is a failure the person could not read.
  const body = readFileSync(await savedPath(d, () => d.click(d.xpath(`${row}//button[@data-file-download]`))), "utf8");
  if (body !== content) throw new Error(`the downloaded file holds ${JSON.stringify(body)}`);
});

await d.close();
console.log("library download passed");
