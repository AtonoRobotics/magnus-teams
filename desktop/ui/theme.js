// Obsidian & Prism theme system: Dark & Light modes, matte tonal
// hierarchy, 5 contrast-checked presets + custom hex accent picker.
"use strict";

(function (root) {
  const LIGHT = {
    bg: "#F6F7F9", side: "#F1F3F6", panel: "#F1F3F6", main: "#FFFFFF", surface: "#F8F9FB", raised: "#EEF1F6",
    bubble: "#EEF1F6", seg: "#E9ECF2", "seg-on": "#FFFFFF",
    fg: "#111318", fg2: "#4B5563", muted: "#565E6B", faint: "#565E6B",
    line: "#E5E7EB", hair: "#EDF0F4", "line-strong": "#CBD5E1", "btn-border": "#D1D5DB", "button-bg": "#FFFFFF",
    "row-sel": "rgba(15,23,42,0.055)", search: "rgba(15,23,42,0.06)", "group-head": "#F8F9FB", "row-hi": "#EEF2FF",
    ok: "#146C34", "ok-soft": "#E6F4EA", "ok-icon": "#16A34A", "ok-fg": "#FFFFFF", warn: "#9A4A00", "warn-soft": "#FEF3C7", "warn-icon": "#D97706", "warn-fg": "#FFFFFF",
    sch: "#364FC7", "sch-soft": "#EDF2FF", "approval-bg": "#F8F9FB", "approval-border": "#CBD5E1", bad: "#B91C1C", "bad-soft": "#FEE2E2",
    shadow: "0 1px 3px rgba(15,23,42,0.04), 0 8px 24px rgba(15,23,42,0.06)", avatar: "#D8DEE9", chev: "#6B7280", "pdf-bg": "#FEE2E2", "pdf-fg": "#991B1B",
    "t-read": "#565E6B", "t-reversible": "#364FC7", "t-irreversible": "#9A4A00", "t-financial": "#B91C1C",
    "k-question": "#6D28D9", "k-review": "#0F766E", "k-notice": "#9A4A00", "k-proposal": "#4338CA", "k-message": "#565E6B", "k-tool": "#9A3412",
  };
  const DARK = {
    bg: "#000000", side: "#09090B", panel: "#09090B", main: "#0A0A0C", surface: "#121215", raised: "#1A1A1F",
    bubble: "#18181C", seg: "#141418", "seg-on": "#222228",
    fg: "#F4F4F5", fg2: "#B4B4BC", muted: "#8E8E96", faint: "#8E8E96",
    line: "#232329", hair: "#18181D", "line-strong": "#35353E", "btn-border": "#2A2A32", "button-bg": "#121215",
    "row-sel": "rgba(255,255,255,0.06)", search: "rgba(255,255,255,0.055)", "group-head": "#101013", "row-hi": "#1C1C22",
    ok: "#34D399", "ok-soft": "#102A22", "ok-icon": "#34D399", "ok-fg": "#05070E", warn: "#FBBF24", "warn-soft": "#2B210E", "warn-icon": "#FBBF24", "warn-fg": "#05070E",
    sch: "#9BA7FF", "sch-soft": "#1C2136", "approval-bg": "#121215", "approval-border": "#2E2E36", bad: "#F87171", "bad-soft": "#2D1619",
    shadow: "0 8px 28px rgba(0,0,0,0.65)", avatar: "#222228", chev: "#71717A", "pdf-bg": "#2D1619", "pdf-fg": "#FCA5A5",
    "t-read": "#8E8E96", "t-reversible": "#9BA7FF", "t-irreversible": "#FBBF24", "t-financial": "#F87171",
    "k-question": "#C4B5FD", "k-review": "#5EEAD4", "k-notice": "#FBBF24", "k-proposal": "#A5B4FC", "k-message": "#8E8E96", "k-tool": "#FDBA74",
  };
  const NOVA_LIGHT = {
    ...LIGHT,
    bg: "#F4F6FA", side: "#F4F6FA", panel: "#F4F6FA", main: "#FFFFFF", surface: "#F5F7FB", raised: "#EBF0F8",
    bubble: "#F2F5FA", seg: "#E7ECF5", "seg-on": "#FFFFFF",
    fg: "#111827", fg2: "#374151", muted: "#525D70", faint: "#525D70",
    line: "#E2E8F0", hair: "#EDF2F7", "line-strong": "#CBD5E1", "btn-border": "#D1D9E6", "button-bg": "#FFFFFF",
    "row-sel": "rgba(15,23,42,0.055)", search: "rgba(15,23,42,0.055)", "group-head": "#F5F7FB", "row-hi": "#EBF2FF",
    shadow: "0 14px 38px rgba(15,23,42,0.08)",
    "aurora-glass": "rgba(255,255,255,0.82)", "aurora-glass-strong": "rgba(255,255,255,0.94)",
    "aurora-rim": "rgba(15,23,42,0.09)", "aurora-beam-a": "#2563EB", "aurora-beam-b": "#7C3AED",
    "aurora-exec-text": "#1D4ED8", "aurora-sweep": "rgba(37,99,235,0.08)",
    "aurora-spot": "rgba(37,99,235,0.10)",
  };
  const NOVA_DARK = {
    ...DARK,
    bg: "#0B0C0E", side: "#141517", panel: "#141517", main: "#0B0C0E", surface: "#17181B", raised: "#1E2025",
    bubble: "#1B1C1F", seg: "#17181B", "seg-on": "#262932",
    fg: "#F1F3F8", fg2: "#C2C8D6", muted: "#959DB0", faint: "#959DB0",
    line: "#23262E", hair: "#1B1D23", "line-strong": "#363A46", "btn-border": "#2A2D37", "button-bg": "#17181B",
    "row-sel": "rgba(255,255,255,0.06)", search: "rgba(255,255,255,0.055)", "group-head": "#141518", "row-hi": "#1F2330",
    "approval-bg": "#17181B", "approval-border": "#2E323D",
    shadow: "0 18px 48px rgba(0,0,0,0.58)",
    "aurora-glass": "rgba(20,21,25,0.82)", "aurora-glass-strong": "rgba(11,12,14,0.92)",
    "aurora-rim": "rgba(255,255,255,0.085)", "aurora-beam-a": "#38BDF8", "aurora-beam-b": "#818CF8",
    "aurora-exec-text": "#60A5FA", "aurora-sweep": "rgba(56,189,248,0.08)",
    "aurora-spot": "rgba(59,130,246,0.14)",
  };
  const ACCENTS = [
    { id: "graphite", label: "Monochrome (Default)", swatch: "#F5F5F5", light: { accent: "#111111", "accent-fg": "#FFFFFF", "accent-text": "#111111", "accent-soft": "#EBEBEB" }, dark: { accent: "#F5F5F5", "accent-fg": "#000000", "accent-text": "#F5F5F5", "accent-soft": "#242424" } },
    { id: "blue", label: "Indigo", swatch: "#7C8CFF", light: { accent: "#3B5BDB", "accent-fg": "#FFFFFF", "accent-text": "#364FC7", "accent-soft": "#EDF2FF" }, dark: { accent: "#7C8CFF", "accent-fg": "#090A0D", "accent-text": "#9BA7FF", "accent-soft": "#1C2136" } },
    { id: "emerald", label: "Emerald", swatch: "#34D399", light: { accent: "#146C34", "accent-fg": "#FFFFFF", "accent-text": "#146C34", "accent-soft": "#ECFDF3" }, dark: { accent: "#34D399", "accent-fg": "#090A0D", "accent-text": "#6EE7B7", "accent-soft": "#102A22" } },
    { id: "amber", label: "Amber", swatch: "#FBBF24", light: { accent: "#B45309", "accent-fg": "#FFFFFF", "accent-text": "#9A4A00", "accent-soft": "#FEF3C7" }, dark: { accent: "#FBBF24", "accent-fg": "#090A0D", "accent-text": "#FCD34D", "accent-soft": "#2B210E" } },
    { id: "rose", label: "Rose", swatch: "#FB7185", light: { accent: "#BE123C", "accent-fg": "#FFFFFF", "accent-text": "#BE123C", "accent-soft": "#FFF1F2" }, dark: { accent: "#FB7185", "accent-fg": "#090A0D", "accent-text": "#FDA4AF", "accent-soft": "#31161D" } },
  ];
  const DEFAULTS = { style: "nova", mode: "dark", accent: "blue", motion: "full", background: "particles", avatar: "pip" };
  /** Maps retired legacy theme ids to their consolidated first-class replacement. */
  const LEGACY_STYLE_ALIASES = {
    obsidian: "classic",
    "obsidian-2": "classic",
    prism: "classic",
    studio: "classic",
    antigravity: "classic",
    companion: "classic",
    gemini: "classic",
    aurora: "nova",
    "obsidian-aurora": "nova",
  };
  const resolveStyle = s => LEGACY_STYLE_ALIASES[/** @type {keyof typeof LEGACY_STYLE_ALIASES} */ (s)] || s;
  const BASE_FEATURES = {
    stage: false,
    defaultAvatar: null,
    docksSidebar: false,
    topBar: false,
    pageCloseButton: true,
    signedByline: false,
    companionFooterInStage: false,
    hideDockPendingBadge: false,
    railActiveOnlyBadge: false,
    introHeading: null,
  };
  /** @type {Record<string, any>} */
  const THEMES = {};
  const STYLES = [];
  const STYLE_TOKENS = {};
  const STAGE_STYLES = [];

  function register(def) {
    const entry = {
      ...def,
      defaultAccent: def.defaultAccent || "blue",
      panelSurface: def.panelSurface || "main",
      features: { ...BASE_FEATURES, ...(def.features || {}) },
    };
    THEMES[entry.id] = entry;
    const idx = STYLES.findIndex(s => s[0] === entry.id);
    if (idx >= 0) STYLES[idx] = [entry.id, entry.label];
    else STYLES.push([entry.id, entry.label]);
    STYLE_TOKENS[entry.id] = entry.radii;
    const stIdx = STAGE_STYLES.indexOf(entry.id);
    if (entry.features.stage && stIdx < 0) STAGE_STYLES.push(entry.id);
    else if (!entry.features.stage && stIdx >= 0) STAGE_STYLES.splice(stIdx, 1);
    return entry;
  }

  register({
    id: "nova", label: "Nova",
    palettes: { light: NOVA_LIGHT, dark: NOVA_DARK },
    radii: { "r-xs": "8px", "r-sm": "10px", "r-md": "14px", "r-lg": "18px", "r-xl": "24px", "r-pill": "999px", "r-send": "999px", "r-avatar": "50%", "r-composer": "24px", "focus-ring": "0 0 0 2px var(--accent-soft)" },
    defaultAccent: "blue",
    panelSurface: "panel",
    features: {
      stage: true, defaultAvatar: "pip", docksSidebar: true, topBar: true,
      pageCloseButton: false, signedByline: true, companionFooterInStage: true,
      hideDockPendingBadge: true, railActiveOnlyBadge: true, introHeading: "Where should we start?",
    },
  });
  register({
    id: "classic", label: "Classic",
    palettes: { light: LIGHT, dark: DARK },
    radii: { "r-xs": "4px", "r-sm": "6px", "r-md": "8px", "r-lg": "10px", "r-xl": "10px", "r-pill": "6px", "r-send": "6px", "r-avatar": "6px", "r-composer": "12px", "focus-ring": "0 0 0 1px var(--accent)" },
    defaultAccent: "graphite",
  });

  const active = () => resolveStyle((typeof document !== "undefined" && document.documentElement?.dataset?.style) || DEFAULTS.style);
  const get = style => THEMES[resolveStyle(style || active())] || THEMES[DEFAULTS.style];
  const has = (feature, style) => Boolean(get(style)?.features?.[feature]);
  const stage = style => has("stage", style);
  const AVATAR_CHOICES = [["pip", "Pip"], ["mochi", "Mochi"], ["orb", "Luminous Orb"], ["prism", "Prism Sprite"]];
  const avatar = (style, prefAvatar) => {
    const def = get(style)?.features?.defaultAvatar;
    if (!def) return null;
    if (prefAvatar && AVATAR_CHOICES.some(a => a[0] === prefAvatar)) return prefAvatar;
    return def;
  };
  const MODES = [["light", "Light"], ["dark", "Dark"], ["system", "Match computer"]];
  const BACKGROUNDS = [["off", "Off"], ["particles", "Particles"], ["ribbons", "Ribbons"], ["constellation", "Constellation"]];
  const BACKGROUND_HINTS = {
    off: "Soft ambient color wash only.",
    particles: "Sparse translucent 3D depth motes drifting slowly behind the workspace.",
    ribbons: "Slow luminous silk ribbons flowing softly behind the workspace.",
    constellation: "Sparse 3D depth motes linked by faint near-plane filaments.",
  };
  const MOTIONS = [["full", "Full"], ["reduced", "Reduced"], ["off", "Off"]];
  const MOTION_HINTS = {
    full: "Panels glide, cards update smoothly, and finished work gets a quiet moment.",
    reduced: "Quick fades instead of movement.",
    off: "No animation. Everything changes instantly.",
  };
  const LAYOUT_BOUNDS = {
    navExpandedWidthPx: 216,
    navCompactWidthPx: 64,
    mobileMaxWidthPx: 760,
    splitMinAvailablePx: 936,
    autoOpenMinWidthPx: 1200,
    shortViewportMaxHeightPx: 520,
    splitterWidthPx: 16,
    splitterStepPx: 16,
    splitterLargeStepPx: 48,
    conversationMinWidthPx: 560,
    panelMinWidthPx: 300,
    panelDefaultWidthPx: 360,
    panelMaxWidthPx: 560,
  };
  const LAYOUT = { navigation: "expanded", sidebar: "rail", panelWidthPx: LAYOUT_BOUNDS.panelDefaultWidthPx };
  // Stage styles use a rail whose full sidebar slides in when asked
  // for, unless the person pins it (decisions/2026-11-21-aurora-rail.md);
  // other styles keep the navigation the person chose.
  const navigation = p => stage(p.style) ? (p.sidebar === "pinned" ? "expanded" : "compact") : p.navigation;

  const isHex = v => typeof v === "string" && /^#[0-9a-fA-F]{6}$/.test(v);
  const mixHex = (a, b, w) => "#" + [1, 3, 5].map(i => Math.round(parseInt(a.slice(i, i + 2), 16) * (1 - w) + parseInt(b.slice(i, i + 2), 16) * w).toString(16).padStart(2, "0")).join("");
  function customAccentTokens(hex, mode, basePal) {
    const pal = basePal || (mode === "dark" ? DARK : LIGHT);
    const target = mode === "dark" ? "#FFFFFF" : "#111111";
    const bestFg = c => contrast(c, "#FFFFFF") >= contrast(c, "#000000") ? "#FFFFFF" : "#000000";
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
    const resolved = resolveStyle(p.style);
    const style = THEMES[resolved] ? resolved : DEFAULTS.style;
    const accent = isHex(p.accent) ? p.accent : (ACCENTS.find(a => a.id === p.accent) || ACCENTS[0]).id;
    const background = BACKGROUNDS.some(b => b[0] === p.background) ? p.background : DEFAULTS.background;
    const companion = AVATAR_CHOICES.some(a => a[0] === p.avatar) ? p.avatar : DEFAULTS.avatar;
    return { style, mode: dark ? "dark" : "light", motion, accent, background, avatar: companion };
  }

  function tokens(look) {
    const dark = look.mode === "dark";
    const resolved = resolveStyle(look.style);
    const def = get(THEMES[resolved] ? resolved : DEFAULTS.style);
    const basePal = dark ? def.palettes.dark : def.palettes.light;
    const t = { ...basePal, ...def.radii };
    t.shell = t.bg;
    t.panel = def.panelSurface === "panel" ? basePal.panel : t.main;
    const preset = ACCENTS.find(a => a.id === look.accent);
    Object.assign(t, preset ? preset[look.mode] : isHex(look.accent) ? customAccentTokens(look.accent, look.mode, basePal) : ACCENTS[0][look.mode]);
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
    if (typeof v.style === "string") v = { ...v, style: resolveStyle(v.style) };
    // A record from before the Theme choice was written on first load, so its
    // "blue" is the old default (Indigo), not a choice: it takes today's.
    if (value && typeof value === "object" && !("style" in v) && v.accent === "blue") v = { ...v, accent: DEFAULTS.accent };
    const rawPanelWidth = Number.isFinite(v.panelWidthPx) ? v.panelWidthPx : null;
    return {
      style: member(v.style, STYLES.map(x => x[0]), DEFAULTS.style),
      mode: member(v.mode, MODES.map(x => x[0]), DEFAULTS.mode),
      accent: isHex(v.accent) ? v.accent : member(v.accent, ACCENTS.map(x => x.id), DEFAULTS.accent),
      motion: member(v.motion, MOTIONS.map(x => x[0]), DEFAULTS.motion),
      background: member(v.background, BACKGROUNDS.map(x => x[0]), DEFAULTS.background),
      avatar: member(v.avatar, AVATAR_CHOICES.map(x => x[0]), DEFAULTS.avatar),
      navigation: member(v.navigation, ["expanded", "compact"], LAYOUT.navigation),
      sidebar: member(v.sidebar, ["rail", "pinned"], LAYOUT.sidebar),
      panelWidthPx: rawPanelWidth !== null
        ? Math.max(LAYOUT_BOUNDS.panelMinWidthPx, Math.min(LAYOUT_BOUNDS.panelMaxWidthPx, rawPanelWidth))
        : LAYOUT.panelWidthPx,
    };
  }

  function contrast(a, b) {
    const l = h => [1, 3, 5].map(i => parseInt(h.slice(i, i + 2), 16) / 255).map(v => v <= .04045 ? v / 12.92 : ((v + .055) / 1.055) ** 2.4).reduce((s, v, i) => s + v * [.2126, .7152, .0722][i], 0);
    const x = l(a), y = l(b); return (Math.max(x, y) + .05) / (Math.min(x, y) + .05);
  }
  root.Theme = { LIGHT, DARK, ACCENTS, THEMES, STYLES, STYLE_TOKENS, MODES, BACKGROUNDS, BACKGROUND_HINTS, MOTIONS, MOTION_HINTS, AVATAR_CHOICES, DEFAULTS, LAYOUT_BOUNDS, LAYOUT, register, get, has, active, navigation, avatar, preferences, contrast, effective, tokens, stage };
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
  // A record is written whole on any change (a splitter drag, a pin, a mode),
  // so one saved before Nova became the default holds the style that was the
  // default then, not a choice. Such a record (no `look`, or an older one)
  // moves to Nova once; a style chosen after that stays
  // (decisions/2026-11-26-saved-looks-move-to-nova-once.md).
  const LOOK = 2;
  const store = () => {
    if (!key) return;
    try { localStorage.setItem(key, JSON.stringify({ ...current, look: LOOK })); } catch { Theme.saved = false; }
  };
  Theme.scope = (appliance, person) => {
    key = person ? `magnus.ui.v2.${appliance}.${person}` : null;
    const saved = key && parse(key), legacy = key && !saved ? parse(LEGACY_KEY) : null;
    const before = !!saved && typeof saved === "object" && saved.look !== LOOK;
    current = preferences(before ? { ...saved, style: DEFAULTS.style } : saved || legacy);
    Theme.saved = true;
    // Only a choice is stored, or a record being migrated: defaults stay
    // unstored, so a later change of default reaches everyone who never chose.
    const migrate = saved ? before || "material" in saved || !("style" in saved) || "conversationWidthPx" in saved : !!legacy;
    if (migrate) store();
    return Theme.apply();
  };
  Theme.os = () => ({ dark: !!darkQ?.matches, reduceMotion: !!motionQ?.matches, forcedColors: !!forcedQ?.matches });
  Theme.apply = () => {
    const look = effective(current, Theme.os()), el = document.documentElement, t = tokens(look);
    for (const [k, v] of Object.entries(t)) el.style.setProperty(`--${k}`, v);
    el.style.colorScheme = look.mode;
    Object.assign(el.dataset, { style: look.style, mode: look.mode, motion: look.motion, accent: look.accent, background: look.background, avatar: look.avatar });
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
    store();
    return Theme.apply();
  };
  for (const q of [darkQ, motionQ, forcedQ]) q?.addEventListener?.("change", Theme.apply);
  Theme.apply();
})(typeof window !== "undefined" ? window : globalThis);
