// Themes (U7, amended): one component set, its tokens swapped by mode
// (light, dark, match computer) × material (solid, glass) + accent, from
// the Magnus Workspace design. Glass is for chrome only (sidebar, panel,
// floating cards); reading surfaces (the conversation, messages, results,
// decision cards) stay solid in every look. The tint of each glass surface
// is chosen so its text reaches 4.5:1 over any backdrop, black or white;
// `desktop/tests/theme-check.mjs` checks every look.
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
    shadow: "0 1px 3px rgba(15,23,42,0.04), 0 8px 24px rgba(15,23,42,0.06)", avatar: "#D8DEE9", chev: "#6B7280", "pdf-bg": "#FEE2E2", "pdf-fg": "#991B1B", "panel-edge": "#E5E7EB", "main-edge": "transparent",
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
    shadow: "0 8px 28px rgba(0,0,0,0.45)", avatar: "#242834", chev: "#6E788C", "pdf-bg": "#2D1619", "pdf-fg": "#FCA5A5", "panel-edge": "#222631", "main-edge": "transparent",
    "t-read": "#8892A6", "t-reversible": "#9BA7FF", "t-irreversible": "#FBBF24", "t-financial": "#F87171",
    "k-question": "#C4B5FD", "k-review": "#5EEAD4", "k-notice": "#FBBF24", "k-proposal": "#A5B4FC", "k-message": "#8892A6", "k-tool": "#FDBA74",
  };
  // Glass: the chrome's tint (the design canvas's), its blur, the drawn
  // backdrop for systems with no window material, and `system`, the tint
  // Magnus gives a material it chooses itself (Acrylic on Windows 10;
  // Windows 11's Mica keeps its own, within that tint's range). Text on
  // glass is a step stronger than on solid.
  const GLASS = {
    light: {
      system: "rgba(248,248,246,0.86)",
      side: "rgba(241,243,246,0.68)", panel: "rgba(241,243,246,0.74)", search: "rgba(255,255,255,0.58)", seg: "rgba(0,0,0,0.06)",
      muted: "#4B5563", faint: "#4B5563", "panel-edge": "rgba(255,255,255,0.6)", "main-edge": "rgba(0,0,0,0.05)",
      "main-shadow": "0 10px 40px rgba(15,23,42,0.10)",
      backdrop: "radial-gradient(60% 50% at 12% 18%, #DDE3F0 0%, rgba(221,227,240,0) 70%), radial-gradient(55% 55% at 88% 12%, #E2E8F0 0%, rgba(226,232,240,0) 70%), #EEF1F6",
    },
    dark: {
      system: "rgba(13,15,19,0.90)",
      side: "rgba(13,15,19,0.68)", panel: "rgba(13,15,19,0.74)", search: "rgba(255,255,255,0.08)", seg: "rgba(255,255,255,0.07)",
      "panel-edge": "rgba(255,255,255,0.08)", "main-edge": "rgba(255,255,255,0.06)",
      "main-shadow": "0 10px 40px rgba(0,0,0,0.5)",
      "search-muted": "#A6B0C3",
      backdrop: "radial-gradient(60% 50% at 12% 18%, #1C2136 0%, rgba(28,33,54,0) 70%), radial-gradient(55% 55% at 88% 12%, #131B29 0%, rgba(19,27,41,0) 70%), #090A0D",
    },
  };
  // Each accent: the fill of primary buttons with the text on it, and the
  // color of accent text (links, selected labels) on each mode's surfaces.
  const ACCENTS = [
    { id: "blue", label: "Indigo (Default)", swatch: "#7C8CFF", light: { accent: "#3B5BDB", "accent-fg": "#FFFFFF", "accent-text": "#364FC7", "accent-soft": "#EDF2FF" }, dark: { accent: "#7C8CFF", "accent-fg": "#090A0D", "accent-text": "#9BA7FF", "accent-soft": "#1C2136" } },
    { id: "emerald", label: "Emerald", swatch: "#34D399", light: { accent: "#146C34", "accent-fg": "#FFFFFF", "accent-text": "#146C34", "accent-soft": "#ECFDF3" }, dark: { accent: "#34D399", "accent-fg": "#090A0D", "accent-text": "#6EE7B7", "accent-soft": "#102A22" } },
    { id: "amber", label: "Amber", swatch: "#FBBF24", light: { accent: "#B45309", "accent-fg": "#FFFFFF", "accent-text": "#9A4A00", "accent-soft": "#FEF3C7" }, dark: { accent: "#FBBF24", "accent-fg": "#090A0D", "accent-text": "#FCD34D", "accent-soft": "#2B210E" } },
    { id: "rose", label: "Rose", swatch: "#FB7185", light: { accent: "#BE123C", "accent-fg": "#FFFFFF", "accent-text": "#BE123C", "accent-soft": "#FFF1F2" }, dark: { accent: "#FB7185", "accent-fg": "#090A0D", "accent-text": "#FDA4AF", "accent-soft": "#31161D" } },
    { id: "graphite", label: "Monochrome", swatch: "#ECEFF4", light: { accent: "#111318", "accent-fg": "#FFFFFF", "accent-text": "#111318", "accent-soft": "#E9ECF2" }, dark: { accent: "#ECEFF4", "accent-fg": "#090A0D", "accent-text": "#ECEFF4", "accent-soft": "#242834" } },
  ];
  const MODES = [["light", "Light"], ["dark", "Dark"], ["system", "Match computer"]];
  const MATERIALS = [["solid", "Solid", "Clean, opaque surfaces"], ["glass", "Glass", "Frosted panels over your wallpaper"]];
  const MOTIONS = [["full", "Full"], ["reduced", "Reduced"], ["off", "Off"]];
  const MOTION_HINTS = {
    full: "Panels glide, cards update smoothly, and finished work gets a quiet moment.",
    reduced: "Quick fades instead of movement.",
    off: "No animation. Everything changes instantly.",
  };
  const DEFAULTS = { mode: "dark", material: "solid", accent: "blue", motion: "full" };
  // Which chrome surfaces may be glass; every other surface is solid.
  const GLASS_SURFACES = ["side", "panel"];

  const isHex = v => typeof v === "string" && /^#[0-9a-fA-F]{6}$/.test(v);
  const mixHex = (a, b, w) => "#" + [1, 3, 5].map(i => Math.round(parseInt(a.slice(i, i + 2), 16) * (1 - w) + parseInt(b.slice(i, i + 2), 16) * w).toString(16).padStart(2, "0")).join("");
  function customAccentTokens(hex, mode) {
    const pal = mode === "dark" ? DARK : LIGHT;
    const target = mode === "dark" ? "#FFFFFF" : "#111318";
    let accent = hex;
    for (let step = 1; step <= 12 && contrast(accent, pal.main) < 3.0; step++) accent = mixHex(hex, target, step * 0.08);
    const fg = contrast(accent, "#FFFFFF") >= contrast(accent, "#090A0D") ? "#FFFFFF" : "#090A0D";
    let text = hex;
    for (let step = 1; step <= 12 && (contrast(text, pal.main) < 4.5 || contrast(text, pal.surface) < 4.5); step++) text = mixHex(hex, target, step * 0.08);
    return { accent, "accent-fg": fg, "accent-text": text, "accent-soft": mixHex(accent, pal.main, mode === "dark" ? 0.86 : 0.90) };
  }

  // The look in effect: the person's choices, as far as the system allows.
  // `os`: { dark, reduceMotion, reduceTransparency }.
  function effective(prefs, os) {
    const p = { ...DEFAULTS, ...(prefs || {}) };
    const dark = p.mode === "dark" || (p.mode === "system" && !!os.dark);
    const order = ["off", "reduced", "full"];
    const motion = os.reduceMotion && order.indexOf(p.motion) > 1 ? "reduced" : p.motion;
    const accent = isHex(p.accent) ? p.accent : (ACCENTS.find(a => a.id === p.accent) || ACCENTS[0]).id;
    return { mode: dark ? "dark" : "light", material: "solid", motion, accent };
  }

  // Every token of a look, by CSS variable name (without the dashes).
  function tokens(look) {
    const dark = look.mode === "dark";
    const t = { ...(dark ? DARK : LIGHT) };
    // Solid: the conversation fills its column. Glass: it floats, inset,
    // over the backdrop the chrome is frosted from.
    t.blur = "none"; t["main-gap"] = "0px"; t["main-radius"] = "0px"; t["main-shadow"] = "none"; t.backdrop = t.bg;
    // What glass looks like in this mode, for the preview in Appearance.
    t["glass-backdrop"] = GLASS[look.mode].backdrop; t["glass-side"] = GLASS[look.mode].side; t["glass-edge"] = GLASS[look.mode]["panel-edge"];
    if (look.material === "glass") {
      Object.assign(t, GLASS[look.mode]);
      t.blur = "blur(40px) saturate(170%)"; t["main-gap"] = "10px"; t["main-radius"] = "18px";
    }
    const preset = ACCENTS.find(a => a.id === look.accent);
    Object.assign(t, preset ? preset[look.mode] : isHex(look.accent) ? customAccentTokens(look.accent, look.mode) : ACCENTS[0][look.mode]);
    // The search field's placeholder and icon: muted, unless the look lifts it.
    t["search-muted"] = (look.material === "glass" && GLASS[look.mode]["search-muted"]) || t.muted;
    return t;
  }


  // Layout choices kept beside the look: the navigation width (labeled
  // sidebar or icon rail) and the conversation's width in the split view.
  const LAYOUT = { navigation: "expanded", conversationWidthPx: 420 };
  const member = (v, choices, fallback) => choices.includes(v) ? v : fallback;
  // A person's saved choices, validated. A record saved by the four-preset
  // build (version 2) reads back as the nearest mode, so no one's choice is
  // lost: a named theme becomes its mode, Follow computer becomes Match
  // computer, and a custom combination keeps its own mode and accent.
  function preferences(value) {
    let v = value && typeof value === "object" ? value : {};
    if (v.version === 2) {
      const mode = v.followComputer ? "system" : v.theme === "custom" ? v.mode : ["dark", "ember"].includes(v.theme) ? "dark" : "light";
      v = { ...v, mode, accent: v.theme === "custom" ? v.accent : DEFAULTS.accent };
    }
    return {
      mode: member(v.mode, MODES.map(x => x[0]), DEFAULTS.mode),
      material: "solid",
      accent: isHex(v.accent) ? v.accent : member(v.accent, ACCENTS.map(x => x.id), DEFAULTS.accent),
      motion: member(v.motion, MOTIONS.map(x => x[0]), DEFAULTS.motion),
      navigation: member(v.navigation, ["expanded", "compact"], LAYOUT.navigation),
      conversationWidthPx: Number.isFinite(v.conversationWidthPx) ? Math.max(360, Math.min(560, v.conversationWidthPx)) : LAYOUT.conversationWidthPx,
    };
  }
  // The computer can also rule glass out: reduced transparency, no blur
  // support, or forced colors all draw Solid without changing the choice.
  const baseEffective = effective;
  function effectiveLook(prefs, os = {}) {
    const look = baseEffective(prefs, os);
    if (os.blur === false || os.forcedColors) look.material = "solid";
    return look;
  }
  const baseTokens = tokens;
  function lookTokens(look) {
    const t = baseTokens(look);
    t.focus = t["accent-text"]; t.shell = t.bg;
    // The work panel now holds the deliverable itself, so it is a reading
    // surface: solid in every look. Glass stays on the navigation chrome.
    t.panel = t.main;
    return t;
  }
  function contrast(a, b) {
    const l = h => [1, 3, 5].map(i => parseInt(h.slice(i, i + 2), 16) / 255).map(v => v <= .04045 ? v / 12.92 : ((v + .055) / 1.055) ** 2.4).reduce((s, v, i) => s + v * [.2126, .7152, .0722][i], 0);
    const x = l(a), y = l(b); return (Math.max(x, y) + .05) / (Math.min(x, y) + .05);
  }
  root.Theme = { LIGHT, DARK, GLASS, ACCENTS, MODES, MATERIALS, MOTIONS, MOTION_HINTS, DEFAULTS, LAYOUT, GLASS_SURFACES, preferences, contrast, effective: effectiveLook, tokens: lookTokens };
  if (typeof document === "undefined") return;

  // In the page: each person's choices are kept on this device under their
  // appliance and identity, applied before anything is drawn, and again
  // when the system's settings change.
  const LEGACY_KEY = "magnus.appearance";
  const media = q => typeof matchMedia === "function" ? matchMedia(q) : null;
  const darkQ = media("(prefers-color-scheme: dark)"), motionQ = media("(prefers-reduced-motion: reduce)"), clearQ = media("(prefers-reduced-transparency: reduce)"), forcedQ = media("(forced-colors: active)");
  let system = {}, key = null, current = preferences();
  // Native material changes run one at a time; a burst of choices sends
  // only the latest, and an older result never replaces a newer one (#485).
  let pendingMaterial = null, changingMaterial = false, desiredMaterial = null;
  async function changeMaterial(request) {
    request.key = JSON.stringify(request.args);
    if (request.key === desiredMaterial) return;
    desiredMaterial = request.key; pendingMaterial = request;
    if (changingMaterial) return;
    changingMaterial = true;
    try {
      while (pendingMaterial) {
        const next = pendingMaterial; pendingMaterial = null;
        try {
          const result = await root.Magnus.setMaterial(...next.args);
          if (next.key === desiredMaterial) document.documentElement.dataset.backdrop = next.glass && result?.native ? "native" : "drawn";
          if (pendingMaterial?.key === next.key) pendingMaterial = null;
        } catch {
          if (next.key === desiredMaterial) document.documentElement.dataset.backdrop = "drawn";
        }
      }
    } finally { changingMaterial = false; }
  }
  const parse = k => { try { return JSON.parse(localStorage.getItem(k)); } catch { return null; } };
  Theme.prefs = () => ({ ...current });
  Theme.scope = (appliance, person) => {
    key = person ? `magnus.ui.v2.${appliance}.${person}` : null;
    const saved = key && parse(key);
    current = preferences(saved || (key ? parse(LEGACY_KEY) : null));
    Theme.saved = true;
    if (key && (!saved || saved.material !== "solid")) { try { localStorage.setItem(key, JSON.stringify(current)); } catch { Theme.saved = false; } }
    return Theme.apply();
  };
  Theme.os = () => ({ dark: !!darkQ?.matches, reduceMotion: !!motionQ?.matches, reduceTransparency: !!clearQ?.matches || !!system.reduce_transparency,
    forcedColors: !!forcedQ?.matches, blur: !root.CSS?.supports || CSS.supports("backdrop-filter", "blur(20px)") || CSS.supports("-webkit-backdrop-filter", "blur(20px)") });
  Theme.apply = () => {
    const look = effectiveLook(current, Theme.os()), el = document.documentElement, t = lookTokens(look);
    for (const [k, v] of Object.entries(t)) el.style.setProperty(`--${k}`, v);
    el.style.colorScheme = look.mode;
    Object.assign(el.dataset, { mode: look.mode, material: look.material, motion: look.motion, accent: look.accent });
    Theme.look = look;
    // Sandboxed same-origin frames contain no scripts. Update their variables
    // in place: preserve data, selection and scroll without fetching again.
    for (const f of document.querySelectorAll("iframe.composed")) {
      try { const r = f.contentDocument?.documentElement; if (r) { for (const [k, v] of Object.entries(t)) r.style.setProperty(`--${k}`, v); r.style.colorScheme = look.mode; } } catch { /* frame still loading */ }
    }
    // The system's own material behind the window where it has one; the
    // page's drawn backdrop where it has none.
    const glass = look.material === "glass", tint = GLASS[look.mode].system.match(/[\d.]+/g).map(Number);
    if (root.Magnus?.setMaterial) changeMaterial({ glass, args: [glass, look.mode === "dark", [...tint.slice(0, 3), Math.round(tint[3] * 255)]] });
    else el.dataset.backdrop = "drawn";
    root.dispatchEvent(new CustomEvent("magnus-theme", { detail: { look, saved: Theme.saved } }));
    return look;
  };
  Theme.set = change => {
    current = preferences({ ...current, ...change });
    Theme.saved = true;
    if (key) { try { localStorage.setItem(key, JSON.stringify(current)); } catch { Theme.saved = false; } }
    return Theme.apply();
  };
  for (const q of [darkQ, motionQ, clearQ, forcedQ]) q?.addEventListener?.("change", Theme.apply);
  Theme.apply();
  addEventListener("DOMContentLoaded", async () => {
    if (root.Magnus?.systemLook) { try { system = await root.Magnus.systemLook(); Theme.apply(); } catch { /* CSS fallback remains usable */ } }
  });
})(typeof window !== "undefined" ? window : globalThis);
