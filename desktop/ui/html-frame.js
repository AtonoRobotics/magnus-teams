// Wraps agent-written magnus-html blocks with Magnus theme CSS variables,
// transparent flush-inline defaults, live theme updates, and auto-sizing
// over postMessage (since sandbox="allow-scripts" frames have an opaque origin).
"use strict";

const HtmlFrame = (() => {
  const MIN_FRAME_HEIGHT_PX = 48;
  const MAX_INLINE_HEIGHT_PX = 640;
  const RESIZE_BUFFER_PX = 2;

  const CHART_PALETTE = {
    light: ["#3B5BDB", "#0F766E", "#B45309", "#BE123C", "#6D28D9", "#4B5563"],
    dark: ["#7C8CFF", "#5EEAD4", "#FBBF24", "#FDA4AF", "#C4B5FD", "#A6B0C3"],
  };

  const safeCssValue = v => String(v ?? "").replace(/<\/style/gi, "").replace(/[;\n\r{}]/g, " ").trim();

  function themeVars(tokens, mode) {
    const m = mode === "dark" ? "dark" : "light";
    const t = tokens || {};
    const vars = {};
    for (const [k, v] of Object.entries(t)) vars[`--${k}`] = safeCssValue(v);
    const pick = (key, fallback) => vars[`--${key}`] || fallback;
    const fg = pick("fg", m === "dark" ? "#ECEFF4" : "#111318");
    const muted = pick("muted", m === "dark" ? "#8892A6" : "#565E6B");
    const main = pick("main", m === "dark" ? "#111318" : "#FFFFFF");
    const surface = pick("surface", m === "dark" ? "#171A21" : "#F8F9FB");
    const raised = pick("raised", m === "dark" ? "#1E222B" : "#EEF1F6");
    const line = pick("line", m === "dark" ? "#222631" : "#E5E7EB");
    const accent = pick("accent", m === "dark" ? "#7C8CFF" : "#3B5BDB");
    const accentFg = pick("accent-fg", m === "dark" ? "#090A0D" : "#FFFFFF");
    const accentText = pick("accent-text", m === "dark" ? "#9BA7FF" : "#364FC7");
    Object.assign(vars, {
      "--background": main,
      "--content": main,
      "--card": surface,
      "--card-border": line,
      "--border": line,
      "--foreground": fg,
      "--muted-foreground": muted,
      "--placeholder": pick("faint", muted),
      "--code-foreground": fg,
      "--primary": accent,
      "--primary-foreground": accentFg,
      "--secondary": raised,
      "--secondary-foreground": fg,
      "--link": accentText,
      "--success": pick("ok", m === "dark" ? "#34D399" : "#146C34"),
      "--warning": pick("warn", m === "dark" ? "#FBBF24" : "#9A4A00"),
      "--error": pick("bad", m === "dark" ? "#F87171" : "#B91C1C"),
    });
    CHART_PALETTE[m].forEach((c, i) => { vars[`--chart-${i + 1}`] = c; });
    return vars;
  }

  function frameScript(mode) {
    const m = mode === "dark" ? "dark" : "light";
    return `<script>(function(){
  var root = document.documentElement;
  function setMode(m){
    root.style.colorScheme = m;
    root.classList.toggle("light", m === "light");
    root.classList.toggle("dark", m === "dark");
  }
  setMode(${JSON.stringify(m)});
  var lastHeight = 0;
  function measure(){
    if (!document.body) return;
    var h = Math.ceil(Math.max(document.body.scrollHeight, document.body.getBoundingClientRect().height)) + ${RESIZE_BUFFER_PX};
    if (h > 0 && h !== lastHeight) {
      lastHeight = h;
      try { window.parent.postMessage({ type: "magnus:resize", height: h }, "*"); } catch {}
    }
  }
  function observe(){
    measure();
    if (typeof ResizeObserver === "function" && document.body) {
      new ResizeObserver(measure).observe(document.body);
    }
  }
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", observe);
  else observe();
  window.addEventListener("load", measure);
  window.addEventListener("message", function(e){
    var d = e.data;
    if (!d || d.type !== "magnus:theme") return;
    if (d.vars) {
      for (var k in d.vars) {
        if (Object.prototype.hasOwnProperty.call(d.vars, k)) root.style.setProperty(k, d.vars[k]);
      }
    }
    setMode(d.mode === "dark" ? "dark" : "light");
    window.dispatchEvent(new Event("themechange"));
    measure();
  });
})();<\/script>`;
  }

  function wrap(body, tokens, mode) {
    const m = mode === "dark" ? "dark" : "light";
    const vars = themeVars(tokens, m);
    const cssVars = Object.entries(vars).map(([k, v]) => `${k}:${v}`).join(";");
    const injected = `<style id="magnus-html-theme">:root{${cssVars}}html{color-scheme:${m}}body{margin:0;padding:0;background:transparent;color:var(--foreground);font:14px/1.45 system-ui,-apple-system,"Segoe UI",sans-serif;-webkit-font-smoothing:antialiased}</style>${frameScript(m)}`;
    const raw = String(body ?? "");
    if (/<head\b[^>]*>/i.test(raw)) return raw.replace(/<head\b[^>]*>/i, match => `${match}${injected}`);
    if (/<html\b[^>]*>/i.test(raw)) return raw.replace(/<html\b[^>]*>/i, match => `${match}<head><meta charset="utf-8">${injected}</head>`);
    return `<!doctype html><html class="${m}" style="color-scheme:${m}"><head><meta charset="utf-8">${injected}</head><body>${raw}</body></html>`;
  }

  function clampHeight(height) {
    if (!Number.isFinite(height)) return null;
    return Math.max(MIN_FRAME_HEIGHT_PX, Math.min(MAX_INLINE_HEIGHT_PX, Math.ceil(height)));
  }

  if (typeof window !== "undefined" && typeof document !== "undefined") {
    window.addEventListener("message", e => {
      const d = e.data;
      if (!d || d.type !== "magnus:resize") return;
      const h = clampHeight(d.height);
      if (h === null) return;
      for (const frame of document.querySelectorAll("iframe.ui-html[data-auto-size]")) {
        if (frame.contentWindow === e.source) {
          if (frame.style.height !== `${h}px`) frame.style.height = `${h}px`;
          break;
        }
      }
    });
    window.addEventListener("magnus-theme", e => {
      const look = e.detail?.look || window.Theme?.look;
      if (!look || !window.Theme?.tokens) return;
      const vars = themeVars(window.Theme.tokens(look), look.mode);
      const msg = { type: "magnus:theme", mode: look.mode, vars };
      for (const frame of document.querySelectorAll("iframe.ui-html[data-auto-size]")) {
        try { frame.contentWindow?.postMessage(msg, "*"); } catch { /* frame unavailable */ }
      }
    });
  }

  return { MIN_FRAME_HEIGHT_PX, MAX_INLINE_HEIGHT_PX, themeVars, wrap, clampHeight };
})();
if (typeof module !== "undefined") module.exports = HtmlFrame;
