// Shared test helper: reads the decomposed app*.js frontend modules in load order.
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const uiDir = join(dirname(fileURLToPath(import.meta.url)), "../ui");

export const APP_MODULES = [
  "app.js",
  "app-shell.js",
  "app-files.js",
  "app-polling.js",
  "app-chat.js",
  "app-inbox.js",
  "app-panel.js",
  "app-library.js",
  "app-settings.js",
  "app-boot.js",
];

/** Concatenate all app*.js modules in browser load order. */
export function readAppBundle() {
  return APP_MODULES.map(f => readFileSync(join(uiDir, f), "utf8")).join("\n");
}
