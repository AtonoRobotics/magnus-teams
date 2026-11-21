// A filmstrip of the mockup (website/) beside the app in Aurora, playing the
// same request, so a change to how Aurora moves is judged by what it looks
// like over time and not only by its checks. Evidence, never a check.
//
//   MAGNUS_CHROMIUM=... node desktop/tests/aurora-filmstrip.mjs OUT_DIR
//
// OUT_DIR gets filmstrip.png (each moment, the mockup left and the app
// right) and a video of each, mockup.mp4 and app.mp4, from Chrome's
// screencast, made by the system's ffmpeg (FFMPEG overrides it).
//
// The app's page has the app's own theme, sheets and Aurora scripts, and a
// conversation as the app draws one. Its task moves through the mockup's four
// steps at the mockup's pace (520 ms each); in the app the pill reads the
// task every 1.5 s, so there each step shows for as long as the task is on it.
import { createRequire } from "node:module";
import { readFileSync, writeFileSync, mkdirSync, rmSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "../..");
const ui = name => readFileSync(join(here, "../ui", name), "utf8");
const { chromium } = createRequire(import.meta.url)(join(root, "browser/node_modules/playwright-core"));
const exe = process.env.MAGNUS_CHROMIUM;
const out = process.argv[2];
if (!exe || !out) { console.error("MAGNUS_CHROMIUM and OUT_DIR are needed"); process.exit(2); }
mkdirSync(out, { recursive: true });

const STEP_MS = 520;
const STEPS = ["Scanning 47 overnight threads in Gmail", "Identifying 2 urgent client renewals", "Checking Q4 pricing & calendar availability", "Staging 6 personal drafts in your tone"];
const PROMPT = "Clear my morning inbox and draft replies for urgent clients";
const ANSWER = "Your morning inbox is clear. I sorted 47 overnight threads, filed newsletters away, and staged 6 personal replies — including two urgent renewal follow-ups with your Q4 pricing attached. Nothing sends until you click approve.";
// The moments compared, each as the mockup's own run has it (ms after load).
const MOMENTS = [["Request in, thinking", 120], ["Step 2: working", 700], ["Step 4: working", 1800], ["Answer arriving", 2300], ["Settled", 3900]];

const sheets = ["base.css", "layout.css", "chat.css", "aurora.css", "aurora-widgets.css", "aurora-stage.css"].map(ui).join("\n");
const stubs = `
var TASK = { task_id: "t4", status: "running", created_at: new Date().toISOString(), status_line: null, plan: null };
var Server = { tasks: { get: async () => JSON.parse(JSON.stringify(TASK)) }, files: { list: async () => ({ files: [] }) } };
var App = { me: { id: "p1" }, thread: "c1", tasks: [], schedules: [], connectors: [], computers: [] };
var Panel = { what: null };
var ENDED = ["completed", "failed", "stopped", "cancelled"];
function icon(k) { return '<svg class="ic"></svg>'; }
function when() { return "just now"; } function until() { return ""; } function firstLine(s) { return String(s || "").split("\\n")[0]; }
function enc(s) { return encodeURIComponent(s); } function stateOf(c) { return c.state; } function needsYou() { return false; } function needsMe() { return []; }
function openPanel() {} function closePanel() {} function drawPanel() {}`;
const appPage = `<!doctype html><html><head><style>${sheets}
body { margin: 0; height: 100vh; } .shell { display: block; height: 100vh; padding: 24px; box-sizing: border-box; }
#main { max-width: 860px; height: calc(100vh - 48px); margin: 0 auto; display: flex; flex-direction: column; }
section.conv { display: flex; flex-direction: column; height: 100%; } .thread { flex: 1; overflow: auto; padding: 20px 28px; display: flex; flex-direction: column; gap: 14px; }</style></head>
<body><script>${ui("theme.js")}</script><script>Theme.set({ style: "aurora", mode: "dark", motion: "full", background: "particles" });</script>
<div class="shell"><main id="main"><section class="conv"><header class="conv-head"><div class="conv-title"><h1>Morning inbox</h1></div><div class="row"></div></header>
<div class="thread" id="thread"></div>
<div class="composer-slot" id="slot"><form class="say" onsubmit="return false"><textarea id="msg" rows="1" placeholder="Ask Magnus to handle something for your team..."></textarea><div class="say-bar"><span class="grow"></span><button type="button" class="icon-btn voice-in" aria-pressed="false" aria-label="Voice input"><svg class="ic"></svg></button><button class="send primary" aria-label="Send">Send</button></div></form></div>
</section></main></div>
<script>${stubs}</script><script>${ui("aurora.js")}</script><script>${ui("aurora-stage.js")}</script><script>${ui("aurora-reasoning.js")}</script>
<script>const box = document.getElementById("msg"); box.addEventListener("input", () => { box.style.height = "auto"; box.style.height = box.scrollHeight + "px"; });</script></body></html>`;

/**
 * Chrome's screencast of a page, as it draws, made into an MP4 cropped to
 * `crop` when stopped.
 * @param {any} context @param {any} page @param {string} name
 */
async function screencast(context, page, name) {
  const cdp = await context.newCDPSession(page);
  /** @type {Array<{ data: string, at: number }>} */
  const shots = [];
  cdp.on("Page.screencastFrame", (/** @type {any} */ f) => { shots.push({ data: f.data, at: f.metadata.timestamp }); cdp.send("Page.screencastAck", { sessionId: f.sessionId }).catch(() => {}); });
  const view = page.viewportSize();
  await cdp.send("Page.startScreencast", { format: "jpeg", quality: 85, everyNthFrame: 1, maxWidth: view.width, maxHeight: view.height });
  return async (/** @type {?{x:number,y:number,width:number,height:number}} */ crop) => {
    await cdp.send("Page.stopScreencast").catch(() => {});
    const dir = join(out, `frames-${name}`);
    mkdirSync(dir, { recursive: true });
    const list = shots.map((f, i) => {
      writeFileSync(join(dir, `${String(i).padStart(5, "0")}.jpg`), Buffer.from(f.data, "base64"));
      const next = shots[i + 1];
      return `file '${String(i).padStart(5, "0")}.jpg'\nduration ${next ? Math.max(0.001, next.at - f.at).toFixed(3) : "0.5"}`;
    });
    writeFileSync(join(dir, "list.txt"), `${list.join("\n")}\nfile '${String(shots.length - 1).padStart(5, "0")}.jpg'\n`);
    const cut = crop ? `crop=${Math.round(crop.width / 2) * 2}:${Math.round(crop.height / 2) * 2}:${Math.round(crop.x)}:${Math.round(crop.y)},` : "";
    execFileSync(process.env.FFMPEG || "ffmpeg", ["-y", "-loglevel", "error", "-f", "concat", "-safe", "0", "-i", join(dir, "list.txt"), "-vf", `${cut}fps=30,scale=trunc(iw/2)*2:trunc(ih/2)*2,format=yuv420p`, "-movflags", "+faststart", join(out, `${name}.mp4`)]);
    rmSync(dir, { recursive: true, force: true });
  };
}

const browser = await chromium.launch({ executablePath: exe, args: ["--no-sandbox", "--disable-dev-shm-usage"] });
const size = { width: 1280, height: 900 };
/** @type {Record<string, Buffer[]>} */
const frames = { mockup: [], app: [] };
try {
  // The mockup plays its inbox run on load.
  const mctx = await browser.newContext({ viewport: { width: 1280, height: 1500 } });
  const m = await mctx.newPage();
  await m.goto(`file://${join(root, "website/index.html")}`);
  // Its run plays on load; it is let finish, and played again from its chip
  // once the screencast runs (the screencast does not survive the load).
  await m.waitForTimeout(4500);
  const clip = await m.evaluate(() => {
    const a = document.getElementById("prompt-spotlight-wrap").getBoundingClientRect(), b = document.getElementById("workspace-stage").getBoundingClientRect();
    const x = Math.max(0, Math.min(a.left, b.left) - 24), y = Math.max(0, a.top - 60);
    return { x, y, width: Math.min(innerWidth - x, Math.max(a.right, b.right) + 24 - x), height: Math.min(innerHeight - y, b.bottom + 24 - y) };
  });
  const stopMockup = await screencast(mctx, m, "mockup");
  await m.waitForTimeout(300);
  const loaded = Date.now();
  await m.click('.chip-btn[data-wf="inbox"]');
  for (const [, at] of MOMENTS) {
    await m.waitForTimeout(Math.max(0, at - (Date.now() - loaded)));
    frames.mockup.push(await m.screenshot({ clip }));
  }
  await m.waitForTimeout(600);
  await stopMockup(clip);
  await mctx.close();

  // The app plays the same request.
  const actx = await browser.newContext({ viewport: size });
  const p = await actx.newPage();
  const stopApp = await screencast(actx, p, "app");
  p.on("pageerror", e => console.error(`app page error: ${e.message}`));
  await p.setContent(appPage);
  await p.waitForTimeout(600);
  await p.focus("#msg");
  await p.keyboard.type(PROMPT, { delay: 12 });
  await p.waitForTimeout(300);
  const shot = () => p.screenshot({ clip: { x: 0, y: 0, ...size } });
  const start = Date.now();
  const at = async ms => p.waitForTimeout(Math.max(0, ms - (Date.now() - start)));
  await p.evaluate(text => {
    const box = document.getElementById("msg"); box.value = ""; box.style.height = "";
    document.getElementById("thread").innerHTML = `<div class="entry me enter" data-entry="m1" data-kind="message"><div class="msg me">${text}</div></div>
      <div class="entry agent enter" data-entry="a:t4" data-kind="turn"><div class="msg magnus"><div class="small faint">Working on an answer…</div><div class="pre" data-live="t4"></div></div></div>`;
    TASK.created_at = new Date().toISOString();
  }, PROMPT);
  // One timeline: the task's steps, the answer, and the moments shot.
  const answer = async () => p.evaluate(a => {
    TASK.status = "completed";
    const turn = document.querySelector('[data-entry="a:t4"]');
    turn.outerHTML = `<div class="entry agent enter" data-entry="a:t4" data-kind="answer"><div class="msg magnus" data-answer data-task="t4"><div class="small faint">Magnus · just now</div><div class="answer-body"><p>${a}</p></div></div></div>`;
  }, ANSWER);
  const events = [
    ...STEPS.map((line, i) => [STEP_MS * (i + 0.15), () => p.evaluate(l => { TASK.status_line = l; return AuroraReasoning.poll(); }, line)]),
    [STEP_MS * STEPS.length, answer],
    ...MOMENTS.map(([, ms]) => [ms, async () => { frames.app.push(await shot()); }]),
  ].sort((x, y) => x[0] - y[0]);
  for (const [ms, run] of events) { await at(ms); await run(); }
  await p.waitForTimeout(800);
  await stopApp(null);
  await actx.close();

  // Each moment, the mockup left and the app right.
  const img = b => `data:image/png;base64,${b.toString("base64")}`;
  const rows = MOMENTS.map(([name, ms], i) => `<tr><th colspan="2">${i + 1}. ${name} <span>· ${(ms / 1000).toFixed(1)}s</span></th></tr><tr><td><img src="${img(frames.mockup[i])}"></td><td><img src="${img(frames.app[i])}"></td></tr>`).join("");
  const sheet = await browser.newPage({ viewport: { width: 1240, height: 800 } });
  await sheet.setContent(`<!doctype html><style>body{margin:0;padding:20px;background:#0b0d12;color:#e8eef9;font:15px system-ui,sans-serif}table{border-collapse:collapse;width:1200px}th{text-align:left;padding:18px 6px 8px;font-weight:600}th span{color:#8592a8;font-weight:400}td{width:50%;vertical-align:top;padding:4px 6px}img{width:100%;border-radius:8px;display:block}.head td{color:#8592a8;font-weight:600;padding-bottom:0}</style>
    <table><tr class="head"><td>Mockup (website/)</td><td>App, Aurora</td></tr>${rows}</table>`);
  await sheet.screenshot({ path: join(out, "filmstrip.png"), fullPage: true });
  console.log(`filmstrip written to ${out}`);
} finally {
  await browser.close();
}
