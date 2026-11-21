// What the Aurora filmstrips share (aurora-filmstrip.mjs, aurora-filmstrip-alerts.mjs):
// Chrome's screencast made into an MP4, and the sheet of moments with the
// mockup left and the app right. Evidence, never a check.
import { writeFileSync, mkdirSync, rmSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { join } from "node:path";

/**
 * Chrome's screencast of a page, as it draws, made into OUT/NAME.mp4,
 * cropped to `crop`, when the returned function is called.
 * @param {any} context @param {any} page @param {string} out @param {string} name
 */
export async function screencast(context, page, out, name) {
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

/**
 * OUT/filmstrip.png: each moment, the mockup left (or a note where it has no
 * such moment) and the app right.
 * @param {any} browser @param {string} out
 * @param {Array<{ title: string, mockup: ?Buffer, app: Buffer, note?: string }>} rows
 */
export async function sheet(browser, out, rows) {
  const img = (/** @type {Buffer} */ b) => `<img src="data:image/png;base64,${b.toString("base64")}">`;
  const html = rows.map((r, i) => `<tr><th colspan="2">${i + 1}. ${r.title}</th></tr><tr><td>${r.mockup ? img(r.mockup) : `<p class="none">${r.note || "The mockup has no such moment."}</p>`}</td><td>${img(r.app)}</td></tr>`).join("");
  const page = await browser.newPage({ viewport: { width: 1240, height: 800 } });
  await page.setContent(`<!doctype html><style>body{margin:0;padding:20px;background:#0b0d12;color:#e8eef9;font:15px system-ui,sans-serif}table{border-collapse:collapse;width:1200px}th{text-align:left;padding:18px 6px 8px;font-weight:600}td{width:50%;vertical-align:top;padding:4px 6px}img{width:100%;border-radius:8px;display:block}.head td{color:#8592a8;font-weight:600;padding-bottom:0}.none{color:#8592a8;padding:24px;border:1px dashed #2a3142;border-radius:8px;margin:0}</style>
    <table><tr class="head"><td>Mockup (website/)</td><td>App, Aurora</td></tr>${html}</table>`);
  await page.screenshot({ path: join(out, "filmstrip.png"), fullPage: true });
  await page.close();
}
