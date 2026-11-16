// Antigravity theme system: Obsidian Dark & Alabaster Light, matte tonal
// hierarchy, 5 contrast-checked presets + custom hex accent picker.
"use strict";

(function (root) {
  const LIGHT = {
    bg: "#F6F7F9", side: "#F1F3F6", panel: "#F1F3F6", main: "#FFFFFF", surface: "#F8F9FB", raised: "#EEF1F6",
    bubble: "#EEF1F6", seg: "#E9ECF2", "seg-on": "#FFFFFF",
    fg: "#111318", fg2: "#4B5563", muted: "#565E6B", faint: "#565E6B",
    line: "#E5E7EB", hair: "#EDF0F4", "line-strong": "#CBD5E1", "btn-border": "#D1D5DB", "button-bg": "#FFFFFF",
    "row-sel": "rgba(15,23,42,0.055)", search: "rgba(15,23,42,0.06)", "group-head": "#F8F9FB", "row-hi": "#EEF2FF",
    ok: "#146C34", "ok-soft": "#E6F4EA", "ok-icon": "#16A34A", warn: "#9A4A00", "warn-soft": "#FEF3C7", "warn-icon": "#D97706",
    sch: "#364FC7", "sch-soft": "#EDF2FF", "approval-bg": "#F8F9FB", "approval-border": "#CBD5E1", bad: "#B91C1C", "bad-soft": "#FEE2E2",
    shadow: "0 1px 3px rgba(15,23,42,0.04), 0 8px 24px rgba(15,23,42,0.06)", avatar: "#D8DEE9", chev: "#6B7280", "pdf-bg": "#FEE2E2", "pdf-fg": "#991B1B",
    "t-read": "#565E6B", "t-reversible": "#364FC7", "t-irreversible": "#9A4A00", "t-financial": "#B91C1C",
    "k-question": "#6D28D9", "k-review": "#0F766E", "k-notice": "#9A4A00", "k-proposal": "#4338CA", "k-message": "#565E6B", "k-tool": "#9A3412",
  };
  const DARK = {
    bg: "#090A0D", side: "#0D0F13", panel: "#0D0F13", main: "#111318", surface: "#171A21", raised: "#1E222B",
    bubble: "#1E222B", seg: "#171A21", "seg-on": "#242834",
    fg: "#ECEFF4", fg2: "#A6B0C3", muted: "#8892A6", faint: "#8892A6",
    line: "#222631", hair: "#1B1E27", "line-strong": "#323846", "btn-border": "#2A2F3D", "button-bg": "#171A21",
    "row-sel": "rgba(255,255,255,0.055)", search: "rgba(255,255,255,0.06)", "group-head": "#151820", "row-hi": "#1A2033",
    ok: "#34D399", "ok-soft": "#102A22", "ok-icon": "#34D399", warn: "#FBBF24", "warn-soft": "#2B210E", "warn-icon": "#FBBF24",
    sch: "#9BA7FF", "sch-soft": "#1C2136", "approval-bg": "#171A21", "approval-border": "#2E3444", bad: "#F87171", "bad-soft": "#2D1619",
    shadow: "0 8px 28px rgba(0,0,0,0.45)", avatar: "#242834", chev: "#6E788C", "pdf-bg": "#2D1619", "pdf-fg": "#FCA5A5",
    "t-read": "#8892A6", "t-reversible": "#9BA7FF", "t-irreversible": "#FBBF24", "t-financial": "#F87171",
    "k-question": "#C4B5FD", "k-review": "#5EEAD4", "k-notice": "#FBBF24", "k-proposal": "#A5B4FC", "k-message": "#8892A6", "k-tool": "#FDBA74",
  };
  const ACCENTS = [
    { id: "graphite", label: "Monochrome (Default)", swatch: "#ECEFF4", light: { accent: "#111318", "accent-fg": "#FFFFFF", "accent-text": "#111318", "accent-soft": "#E9ECF2" }, dark: { accent: "#ECEFF4", "accent-fg": "#090A0D", "accent-text": "#ECEFF4", "accent-soft": "#242834" } },
    { id: "blue", label: "Indigo", swatch: "#7C8CFF", light: { accent: "#3B5BDB", "accent-fg": "#FFFFFF", "accent-text": "#364FC7", "accent-soft": "#EDF2FF" }, dark: { accent: "#7C8CFF", "accent-fg": "#090A0D", "accent-text": "#9BA7FF", "accent-soft": "#1C2136" } },
    { id: "emerald", label: "Emerald", swatch: "#34D399", light: { accent: "#146C34", "accent-fg": "#FFFFFF", "accent-text": "#146C34", "accent-soft": "#ECFDF3" }, dark: { accent: "#34D399", "accent-fg": "#090A0D", "accent-text": "#6EE7B7", "accent-soft": "#102A22" } },
    { id: "amber", label: "Amber", swatch: "#FBBF24", light: { accent: "#B45309", "accent-fg": "#FFFFFF", "accent-text": "#9A4A00", "accent-soft": "#FEF3C7" }, dark: { accent: "#FBBF24", "accent-fg": "#090A0D", "accent-text": "#FCD34D", "accent-soft": "#2B210E" } },
    { id: "rose", label: "Rose", swatch: "#FB7185", light: { accent: "#BE123C", "accent-fg": "#FFFFFF", "accent-text": "#BE123C", "accent-soft": "#FFF1F2" }, dark: { accent: "#FB7185", "accent-fg": "#090A0D", "accent-text": "#FDA4AF", "accent-soft": "#31161D" } },
  ];
  const STYLES = [["antigravity", "Antigravity"], ["companion", "Companion"]];
  const STYLE_TOKENS = {
    antigravity: {
      "r-xs": "4px", "r-sm": "6px", "r-md": "8px", "r-lg": "10px", "r-xl": "10px",
      "r-pill": "6px", "r-send": "6px", "r-avatar": "6px", "r-composer": "10px",
      "focus-ring": "0 0 0 1px var(--accent)",
    },
    companion: {
      "r-xs": "6px", "r-sm": "8px", "r-md": "12px", "r-lg": "16px", "r-xl": "18px",
      "r-pill": "999px", "r-send": "50%", "r-avatar": "50%", "r-composer": "16px",
      "focus-ring": "0 0 0 3px var(--accent-soft)",
    },
  };
  const MODES = [["light", "Light"], ["dark", "Dark"], ["system", "Match computer"]];
  const MOTIONS = [["full", "Full"], ["reduced", "Reduced"], ["off", "Off"]];
  const MOTION_HINTS = {
    full: "Panels glide, cards update smoothly, and finished work gets a quiet moment.",
    reduced: "Quick fades instead of movement.",
    off: "No animation. Everything changes instantly.",
  };
  const DEFAULTS = { style: "antigravity", mode: "dark", accent: "graphite", motion: "full" };
  const LAYOUT = { navigation: "expanded", conversationWidthPx: 420 };

  const isHex = v => typeof v === "string" && /^#[0-9a-fA-F]{6}$/.test(v);
  const mixHex = (a, b, w) => "#" + [1, 3, 5].map(i => Math.round(parseInt(a.slice(i, i + 2), 16) * (1 - w) + parseInt(b.slice(i, i + 2), 16) * w).toString(16).padStart(2, "0")).join("");
  function customAccentTokens(hex, mode) {
    const pal = mode === "dark" ? DARK : LIGHT;
    const target = mode === "dark" ? "#FFFFFF" : "#111318";
    const bestFg = c => contrast(c, "#FFFFFF") >= contrast(c, "#090A0D") ? "#FFFFFF" : "#090A0D";
    let accent = hex;
    for (let step = 1; step <= 16 && (contrast(accent, pal.main) < 3.0 || contrast(accent, bestFg(accent)) < 4.5); step++) {
      accent = mixHex(hex, target, step / 16);
    }
    const fg = bestFg(accent);
    const soft = mixHex(accent, pal.main, mode === "dark" ? 0.86 : 0.90);
    let text = hex;
    for (let step = 1; step <= 16 && (contrast(text, pal.main) < 4.5 || contrast(text, pal.surface) < 4.5 || contrast(text, pal.raised) < 4.5 || contrast(text, soft) < 4.5); step++) {
      text = mixHex(hex, target, step / 16);
    }
    return { accent, "accent-fg": fg, "accent-text": text, "accent-soft": soft };
  }

  function effective(prefs, os = {}) {
    const p = { ...DEFAULTS, ...(prefs || {}) };
    const dark = p.mode === "dark" || (p.mode === "system" && !!os.dark);
    const order = ["off", "reduced", "full"];
    const motion = os.reduceMotion && order.indexOf(p.motion) > 1 ? "reduced" : p.motion;
    const style = STYLES.some(s => s[0] === p.style) ? p.style : DEFAULTS.style;
    const accent = isHex(p.accent) ? p.accent : (ACCENTS.find(a => a.id === p.accent) || ACCENTS[0]).id;
    return { style, mode: dark ? "dark" : "light", motion, accent };
  }

  function tokens(look) {
    const dark = look.mode === "dark";
    const t = { ...(dark ? DARK : LIGHT) };
    const style = STYLES.some(s => s[0] === look.style) ? look.style : DEFAULTS.style;
    Object.assign(t, STYLE_TOKENS[style]);
    t.shell = t.bg;
    t.panel = t.main;
    const preset = ACCENTS.find(a => a.id === look.accent);
    Object.assign(t, preset ? preset[look.mode] : isHex(look.accent) ? customAccentTokens(look.accent, look.mode) : ACCENTS[0][look.mode]);
    t.focus = t["accent-text"];
    return t;
  }

  const member = (v, choices, fallback) => choices.includes(v) ? v : fallback;
  function preferences(value) {
    let v = value && typeof value === "object" ? value : {};
    if (v.version === 2) {
      const mode = v.followComputer ? "system" : v.theme === "custom" ? v.mode : ["dark", "ember"].includes(v.theme) ? "dark" : "light";
      v = { ...v, style: DEFAULTS.style, mode, accent: v.theme === "custom" ? v.accent : DEFAULTS.accent };
    }
    return {
      style: member(v.style, STYLES.map(x => x[0]), DEFAULTS.style),
      mode: member(v.mode, MODES.map(x => x[0]), DEFAULTS.mode),
      accent: isHex(v.accent) ? v.accent : member(v.accent, ACCENTS.map(x => x.id), DEFAULTS.accent),
      motion: member(v.motion, MOTIONS.map(x => x[0]), DEFAULTS.motion),
      navigation: member(v.navigation, ["expanded", "compact"], LAYOUT.navigation),
      conversationWidthPx: Number.isFinite(v.conversationWidthPx) ? Math.max(360, Math.min(560, v.conversationWidthPx)) : LAYOUT.conversationWidthPx,
    };
  }

  function contrast(a, b) {
    const l = h => [1, 3, 5].map(i => parseInt(h.slice(i, i + 2), 16) / 255).map(v => v <= .04045 ? v / 12.92 : ((v + .055) / 1.055) ** 2.4).reduce((s, v, i) => s + v * [.2126, .7152, .0722][i], 0);
    const x = l(a), y = l(b); return (Math.max(x, y) + .05) / (Math.min(x, y) + .05);
  }
  root.Theme = { LIGHT, DARK, ACCENTS, STYLES, STYLE_TOKENS, MODES, MOTIONS, MOTION_HINTS, DEFAULTS, LAYOUT, preferences, contrast, effective, tokens };
  if (typeof document === "undefined") return;

  const LEGACY_KEY = "magnus.appearance";
  const media = q => typeof matchMedia === "function" ? matchMedia(q) : null;
  const darkQ = media("(prefers-color-scheme: dark)"), motionQ = media("(prefers-reduced-motion: reduce)"), forcedQ = media("(forced-colors: active)");
  let key = null, current = preferences();
  let pendingBg = null, changingBg = false, desiredBg = null;
  async function syncBackground(dark) {
    if (dark === desiredBg) return;
    desiredBg = dark; pendingBg = dark;
    if (changingBg) return;
    changingBg = true;
    try {
      while (pendingBg !== null) {
        const next = pendingBg; pendingBg = null;
        try { await root.Magnus.setBackground(next); } catch { /* CSS fallback remains */ }
      }
    } finally { changingBg = false; }
  }
  const parse = k => { try { return JSON.parse(localStorage.getItem(k)); } catch { return null; } };
  Theme.prefs = () => ({ ...current });
  Theme.scope = (appliance, person) => {
    key = person ? `magnus.ui.v2.${appliance}.${person}` : null;
    const saved = key && parse(key);
    current = preferences(saved || (key ? parse(LEGACY_KEY) : null));
    Theme.saved = true;
    if (key && (!saved || "material" in saved)) { try { localStorage.setItem(key, JSON.stringify(current)); } catch { Theme.saved = false; } }
    return Theme.apply();
  };
  Theme.os = () => ({ dark: !!darkQ?.matches, reduceMotion: !!motionQ?.matches, forcedColors: !!forcedQ?.matches });
  Theme.apply = () => {
    const look = effective(current, Theme.os()), el = document.documentElement, t = tokens(look);
    for (const [k, v] of Object.entries(t)) el.style.setProperty(`--${k}`, v);
    el.style.colorScheme = look.mode;
    Object.assign(el.dataset, { style: look.style, mode: look.mode, motion: look.motion, accent: look.accent });
    delete el.dataset.material;
    Theme.look = look;
    for (const f of document.querySelectorAll("iframe.composed")) {
      try { const r = f.contentDocument?.documentElement; if (r) { for (const [k, v] of Object.entries(t)) r.style.setProperty(`--${k}`, v); r.style.colorScheme = look.mode; } } catch { /* frame still loading */ }
    }
    if (root.Magnus?.setBackground) syncBackground(look.mode === "dark");
    root.dispatchEvent(new CustomEvent("magnus-theme", { detail: { look, saved: Theme.saved } }));
    return look;
  };
  Theme.set = change => {
    current = preferences({ ...current, ...change });
    Theme.saved = true;
    if (key) { try { localStorage.setItem(key, JSON.stringify(current)); } catch { Theme.saved = false; } }
    return Theme.apply();
  };
  for (const q of [darkQ, motionQ, forcedQ]) q?.addEventListener?.("change", Theme.apply);
  Theme.apply();
})(typeof window !== "undefined" ? window : globalThis);
