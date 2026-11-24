// The reference the desktop acceptance check compares against (spec rev 145,
// "The desktop": "Mockup. The Desktop and Wallpaper artboards"): the Desktop
// artboard with no window open, over the wallpaper, in dark and light,
// rendered by a real Chromium at the computer's 1280x800. The wallpaper is
// the one make_theme.py draws: it replaced the Wallpaper artboard's on
// 2026-11-16 (decisions/2026-11-24-the-desktop-bar-is-the-apps-color.md).
//
// node tests/scratch/reference/render-desktop.mjs
//   MAGNUS_CHROMIUM  the Chromium to run (default: Playwright's, as tests use)
//
// Writes desktop-dark.png and desktop-light.png beside this file. Run it
// again when the artboard or the wallpaper changes, and commit what it writes.
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
import path from "node:path";
import { readFileSync } from "node:fs";
import { execFileSync } from "node:child_process";

const here = path.dirname(fileURLToPath(import.meta.url));
const require = createRequire(path.join(here, "../../../browser/package.json"));
const { chromium } = require("playwright-core");
const design = path.join(here, "../../../docs/design/team-and-computer");

// An artboard's markup and the colors its script gives for a theme.
function artboard(file, theme) {
  const src = readFileSync(path.join(design, file), "utf8");
  const body = src.slice(src.indexOf("</helmet>") + "</helmet>".length, src.indexOf("</x-dc>"));
  const script = src.slice(src.indexOf("class Component"), src.lastIndexOf("</script>"));
  const DCLogic = class { constructor() { this.props = { theme }; } };
  const Component = new Function("DCLogic", `${script}; return Component;`)(DCLogic);
  const vals = new Component().renderVals();
  return body.replace(/\{\{\s*([\w]+)\s*\}\}/g, (_, k) => String(vals[k] ?? ""));
}

function page(theme) {
  const wallpaper = execFileSync("python3", ["-c", `import sys; sys.path.insert(0, sys.argv[1]); import make_theme; print(make_theme.wallpaper_svg(sys.argv[2]))`, path.join(here, "../../../sandbox/desktop"), theme], { encoding: "utf8" });
  let desktop = artboard("computer-desktop.dc.html", theme);
  desktop = desktop.replace(/<dc-import name="Wallpaper"[^>]*><\/dc-import>/, wallpaper);
  // No window open: a computer as it wakes.
  desktop = desktop.replace(/<section[\s\S]*<\/section>/, "");
  // Nothing is open, so no app's button in the bar is marked as open.
  desktop = desktop.replace(/background: #[0-9A-Fa-f]{6}; color: ([^;]+); font-size: 12px"><svg aria-hidden="true" width="15"/g, 'background: transparent; color: $1; font-size: 12px"><svg aria-hidden="true" width="15"');
  return `<!doctype html><html><head><meta charset="utf-8"><style>body{margin:0}button{font-family:inherit}</style></head><body>${desktop}</body></html>`;
}

const executablePath = process.env.MAGNUS_CHROMIUM || path.join(process.env.HOME, ".cache/ms-playwright/chromium-1234/chrome-linux64/chrome");
const browser = await chromium.launch({ executablePath });
const tab = await browser.newPage({ viewport: { width: 1280, height: 800 } });
for (const theme of ["dark", "light"]) {
  await tab.setContent(page(theme));
  await tab.screenshot({ path: path.join(here, `desktop-${theme}.png`) });
}
await browser.close();
