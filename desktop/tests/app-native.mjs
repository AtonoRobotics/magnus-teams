// What only the installed app does: scripts in a sandboxed page in WebKit,
// connecting this computer, and carrying a takeover's screen itself.
// MAGNUS_WEBDRIVER, MAGNUS_APP, MAGNUS_COMPUTER, MAGNUS_FOLDER; MAGNUS_SHOTS optional.
import { env, open } from "./appdriver.mjs";

const [wd, app, computer, folder] = ["MAGNUS_WEBDRIVER", "MAGNUS_APP", "MAGNUS_COMPUTER", "MAGNUS_FOLDER"].map(env);
const d = await open(wd, app, process.env.MAGNUS_SHOTS, "native");
const go = hash => d.run("location.hash = arguments[0]", hash);

await d.step("start signed in", () => d.seen("Computers"));

await d.step("run a sandboxed page's script", async () => {
  // The driver cannot look inside a frame with no origin, so the page says so.
  await d.run("window.addEventListener('message', e => { if (e.data === 'script ran') window.frameRan = true; })");
  await go("#/chat/new");
  await d.type(d.css("#msg"), "Make a page");
  await d.click(d.css("#send"));
  await d.css("iframe.ui-html");
  await d.until("the page's script", () => d.run("return document.querySelector('iframe.ui-html').src.includes('magnus-frame') && window.frameRan === true"));
});

await d.step("connect this computer", async () => {
  await go("#/computer");
  await d.clear(d.css("#lc-name"));
  await d.type(d.css("#lc-name"), "test-desk");
  await d.type(d.css("#lc-folders"), folder);
  await d.click(d.button("Connect"));
  await d.seen("connected as");
  await d.until("it to come online", async () => {
    await go("#/teams"); await go("#/computer");
    return d.run("return !!document.querySelector('tr[data-local]') && document.querySelector('tr[data-local]').innerText.includes('Online')");
  });
});

await d.step("pause the agent here", async () => {
  await d.click(d.button("Pause the agent"));
  await d.xpath("//button[normalize-space()='Let the agent work']");
  await d.click(d.button("Let the agent work"));
  await d.xpath("//button[normalize-space()='Pause the agent']");
  await d.click(d.button("Stop"));
  await d.css("#lc-folders");
});

await d.step("take over through the app", async () => {
  await go(`#/computer/${encodeURIComponent(computer)}`);
  await d.click(d.button("Take control"));
  await d.until("the screen", () => d.run("const c = document.querySelector('#display'); return !!c && c.width > 0 && c.getContext('2d').getImageData(0, 0, 1, 1).data[3] === 255"));
  await d.click(d.button("I'm done"));
  await d.xpath("//button[normalize-space()='Take control']");
});

await d.close();
console.log("native ok");
