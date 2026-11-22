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
  const PRISM_LIGHT = {
    ...LIGHT,
    bg: "#FFFFFF", side: "#F0F4F9", panel: "#F0F4F9", main: "#FFFFFF", surface: "#F0F4F9", raised: "#E9EEF6",
    bubble: "#E9EEF6", seg: "#E1E5EA", "seg-on": "#FFFFFF",
    fg: "#1F1F1F", fg2: "#444746", muted: "#5A5E66", faint: "#5A5E66",
    line: "#DDE3EA", hair: "#E8EDF2", "line-strong": "#C4C7C5", "btn-border": "#C4C7C5", "button-bg": "#F0F4F9",
    "row-sel": "rgba(31,31,31,0.055)", search: "rgba(31,31,31,0.06)", "group-head": "#F0F4F9", "row-hi": "#E8F0FE",
    shadow: "0 2px 12px rgba(15,23,42,0.06)",
  };
  const PRISM_DARK = {
    ...DARK,
    bg: "#131314", side: "#1E1F20", panel: "#1E1F20", main: "#131314", surface: "#1E1F20", raised: "#282A2C",
    bubble: "#282A2C", seg: "#282A2C", "seg-on": "#333537",
    fg: "#E3E3E3", fg2: "#C4C7C5", muted: "#9AA0A6", faint: "#9AA0A6",
    line: "#2E3033", hair: "#242628", "line-strong": "#444746", "btn-border": "#3C4043", "button-bg": "#1E1F20",
    "row-sel": "rgba(227,227,227,0.07)", search: "rgba(227,227,227,0.07)", "group-head": "#1E1F20", "row-hi": "#282A2C",
    shadow: "0 4px 20px rgba(0,0,0,0.36)",
  };
  const OBSIDIAN2_LIGHT = {
    ...LIGHT,
    bg: "#F1F3F6", side: "#F1F3F6", panel: "#F1F3F6", main: "#FFFFFF", surface: "#F6F7F9", raised: "#EAECEF",
    bubble: "#EAECEF", seg: "#E5E8EE", "seg-on": "#FFFFFF",
    line: "#E2E5EA", hair: "#ECEFF4", "line-strong": "#CBD5E1", "btn-border": "#D1D5DB", "button-bg": "#FFFFFF",
    "row-sel": "rgba(15,23,42,0.055)", search: "rgba(15,23,42,0.06)", "group-head": "#F6F7F9", "row-hi": "#E8EEF8",
  };
  const OBSIDIAN2_DARK = {
    ...DARK,
    bg: "#121215", side: "#121215", panel: "#121215", main: "#000000", surface: "#121215", raised: "#1C1C21",
    bubble: "#18181C", seg: "#141418", "seg-on": "#222227",
    line: "#1F1F24", hair: "#18181D", "line-strong": "#2E2E36", "btn-border": "#27272E", "button-bg": "#18181C",
    "row-sel": "rgba(255,255,255,0.055)", search: "rgba(255,255,255,0.055)", "group-head": "#141418", "row-hi": "#222227",
    shadow: "0 8px 28px rgba(0,0,0,0.7)",
  };
  const STUDIO_LIGHT = {
    ...LIGHT,
    bg: "#F8F9FA", side: "#F1F3F4", panel: "#FFFFFF", main: "#FFFFFF", surface: "#F8F9FA", raised: "#E8EAED",
    bubble: "#F1F3F4", seg: "#E8EAED", "seg-on": "#FFFFFF",
    fg: "#202124", fg2: "#444746", muted: "#5F6368", faint: "#5F6368",
    line: "#DADCE0", hair: "#E8EAED", "line-strong": "#BDC1C6", "btn-border": "#DADCE0", "button-bg": "#FFFFFF",
    "row-sel": "rgba(32,33,36,0.055)", search: "rgba(32,33,36,0.06)", "group-head": "#F8F9FA", "row-hi": "#E8F0FE",
    shadow: "0 2px 12px rgba(15,23,42,0.06)",
  };
  const STUDIO_DARK = {
    ...DARK,
    bg: "#181819", side: "#141415", panel: "#181819", main: "#181819", surface: "#232325", raised: "#2C2D30",
    bubble: "#232325", seg: "#1E1F22", "seg-on": "#2C2D30",
    fg: "#E8EAED", fg2: "#C4C7C5", muted: "#9AA0A6", faint: "#9AA0A6",
    line: "#2E3034", hair: "#232326", "line-strong": "#3C3D40", "btn-border": "#3C3D40", "button-bg": "#232325",
    "row-sel": "rgba(232,234,237,0.06)", search: "rgba(232,234,237,0.06)", "group-head": "#1E1F22", "row-hi": "#26272A",
    shadow: "0 4px 20px rgba(0,0,0,0.40)",
  };
  const AURORA_LIGHT = {
    ...LIGHT,
    bg: "#F4F7FE", side: "#EEF3FC", panel: "#F6F9FF", main: "#FAFBFD", surface: "#FFFFFF", raised: "#EAF0FB",
    bubble: "#EDF3FD", seg: "#E5ECF9", "seg-on": "#FFFFFF",
    fg: "#0D1526", fg2: "#3B4A66", muted: "#586885", faint: "#586885",
    line: "#DCE5F4", hair: "#E9EFFA", "line-strong": "#B8C7E2", "btn-border": "#CBD7EC", "button-bg": "#FFFFFF",
    "row-sel": "rgba(37,99,235,0.07)", search: "rgba(37,99,235,0.06)", "group-head": "#F1F5FD", "row-hi": "#E6EFFD",
    shadow: "0 12px 36px rgba(15,35,75,0.08)",
    "aurora-glass": "rgba(255,255,255,0.74)", "aurora-glass-strong": "rgba(255,255,255,0.88)",
    "aurora-rim": "rgba(37,99,235,0.14)", "aurora-beam-a": "#38BDF8", "aurora-beam-b": "#8B5CF6",
    "aurora-exec-text": "#0369A1", "aurora-sweep": "rgba(56,189,248,0.10)",
    "aurora-spot": "rgba(59,130,246,0.13)",
  };
  const AURORA_DARK = {
    ...DARK,
    bg: "#05070E", side: "#080C18", panel: "#090E1C", main: "#060913", surface: "#0E1424", raised: "#151E34",
    bubble: "#121A2E", seg: "#0E1526", "seg-on": "#1B2640",
    fg: "#F4F7FC", fg2: "#BBC6DC", muted: "#8393B2", faint: "#8393B2",
    line: "#1C263D", hair: "#131B2E", "line-strong": "#2E3D5E", "btn-border": "#263350", "button-bg": "#0F1628",
    "row-sel": "rgba(96,165,250,0.10)", search: "rgba(96,165,250,0.08)", "group-head": "#0C1221", "row-hi": "#16213A",
    shadow: "0 18px 48px rgba(0,4,16,0.62)",
    "aurora-glass": "rgba(13,19,34,0.62)", "aurora-glass-strong": "rgba(15,22,40,0.82)",
    "aurora-rim": "rgba(186,215,255,0.12)", "aurora-beam-a": "#38BDF8", "aurora-beam-b": "#9B6DFF",
    "aurora-exec-text": "#38BDF8", "aurora-sweep": "rgba(186,230,253,0.08)",
    "aurora-spot": "rgba(79,157,255,0.20)",
  };
  const OBSIDIAN_AURORA_LIGHT = {
    ...OBSIDIAN2_LIGHT,
    "aurora-glass": "rgba(246,247,249,0.78)", "aurora-glass-strong": "rgba(255,255,255,0.92)",
    "aurora-rim": "rgba(15,23,42,0.11)", "aurora-beam-a": "#3B5BDB", "aurora-beam-b": "#64748B",
    "aurora-exec-text": "#3B5BDB", "aurora-sweep": "rgba(59,91,219,0.08)",
    "aurora-spot": "rgba(59,91,219,0.09)",
  };
  const OBSIDIAN_AURORA_DARK = {
    ...OBSIDIAN2_DARK,
    "aurora-glass": "rgba(16,16,20,0.72)", "aurora-glass-strong": "rgba(22,22,27,0.90)",
    "aurora-rim": "rgba(255,255,255,0.11)", "aurora-beam-a": "#E4E4E7", "aurora-beam-b": "#7C8CFF",
    "aurora-exec-text": "#E4E4E7", "aurora-sweep": "rgba(228,228,231,0.06)",
    "aurora-spot": "rgba(255,255,255,0.09)",
  };
  const ACCENTS = [
    { id: "graphite", label: "Monochrome (Default)", swatch: "#F5F5F5", light: { accent: "#111111", "accent-fg": "#FFFFFF", "accent-text": "#111111", "accent-soft": "#EBEBEB" }, dark: { accent: "#F5F5F5", "accent-fg": "#000000", "accent-text": "#F5F5F5", "accent-soft": "#242424" } },
    { id: "blue", label: "Indigo", swatch: "#7C8CFF", light: { accent: "#3B5BDB", "accent-fg": "#FFFFFF", "accent-text": "#364FC7", "accent-soft": "#EDF2FF" }, dark: { accent: "#7C8CFF", "accent-fg": "#090A0D", "accent-text": "#9BA7FF", "accent-soft": "#1C2136" } },
    { id: "emerald", label: "Emerald", swatch: "#34D399", light: { accent: "#146C34", "accent-fg": "#FFFFFF", "accent-text": "#146C34", "accent-soft": "#ECFDF3" }, dark: { accent: "#34D399", "accent-fg": "#090A0D", "accent-text": "#6EE7B7", "accent-soft": "#102A22" } },
    { id: "amber", label: "Amber", swatch: "#FBBF24", light: { accent: "#B45309", "accent-fg": "#FFFFFF", "accent-text": "#9A4A00", "accent-soft": "#FEF3C7" }, dark: { accent: "#FBBF24", "accent-fg": "#090A0D", "accent-text": "#FCD34D", "accent-soft": "#2B210E" } },
    { id: "rose", label: "Rose", swatch: "#FB7185", light: { accent: "#BE123C", "accent-fg": "#FFFFFF", "accent-text": "#BE123C", "accent-soft": "#FFF1F2" }, dark: { accent: "#FB7185", "accent-fg": "#090A0D", "accent-text": "#FDA4AF", "accent-soft": "#31161D" } },
  ];
  const STYLES = [["obsidian", "Obsidian"], ["obsidian-2", "Obsidian 2"], ["obsidian-aurora", "Obsidian 2 Aurora"], ["prism", "Prism"], ["studio", "Studio"], ["aurora", "Aurora"]];
  // Styles that draw the column beside the conversation as a stage of cards
  // with a canvas for what is opened (aurora-stage.js).
  const STAGE_STYLES = ["aurora", "obsidian-aurora"];
  const stage = style => STAGE_STYLES.includes(style);
  // Each style names the avatar that stands for Magnus (aurora-avatar.js);
  // a style that names none draws its own mark.
  const AVATARS = { aurora: "orb", "obsidian-aurora": "orb" };
  const avatar = style => AVATARS[style] || null;
  const STYLE_TOKENS = {
    obsidian: {
      "r-xs": "4px", "r-sm": "6px", "r-md": "8px", "r-lg": "10px", "r-xl": "10px",
      "r-pill": "6px", "r-send": "6px", "r-avatar": "6px", "r-composer": "12px",
      "focus-ring": "0 0 0 1px var(--accent)",
    },
    "obsidian-2": {
      "r-xs": "5px", "r-sm": "7px", "r-md": "10px", "r-lg": "12px", "r-xl": "14px",
      "r-pill": "8px", "r-send": "50%", "r-avatar": "6px", "r-composer": "14px",
      "focus-ring": "0 0 0 1px var(--accent)",
    },
    "obsidian-aurora": {
      "r-xs": "5px", "r-sm": "7px", "r-md": "10px", "r-lg": "12px", "r-xl": "14px",
      "r-pill": "8px", "r-send": "50%", "r-avatar": "6px", "r-composer": "14px",
      "focus-ring": "0 0 0 1px var(--accent)",
    },
    prism: {
      "r-xs": "8px", "r-sm": "12px", "r-md": "16px", "r-lg": "20px", "r-xl": "24px",
      "r-pill": "999px", "r-send": "50%", "r-avatar": "50%", "r-composer": "24px",
      "focus-ring": "0 0 0 3px var(--accent-soft)",
    },
    studio: {
      "r-xs": "8px", "r-sm": "10px", "r-md": "14px", "r-lg": "16px", "r-xl": "22px",
      "r-pill": "999px", "r-send": "999px", "r-avatar": "50%", "r-composer": "22px",
      "focus-ring": "0 0 0 2px var(--accent-soft)",
    },
    aurora: {
      "r-xs": "8px", "r-sm": "12px", "r-md": "16px", "r-lg": "22px", "r-xl": "26px",
      "r-pill": "999px", "r-send": "999px", "r-avatar": "50%", "r-composer": "24px",
      "focus-ring": "0 0 0 3px var(--accent-soft)",
    },
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
  const DEFAULTS = { style: "obsidian", mode: "dark", accent: "graphite", motion: "full", background: "particles" };
  const LAYOUT_BOUNDS = {
    navExpandedWidthPx: 228,
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
  // Aurora's navigation is a rail whose full sidebar slides in when asked
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
    const style = STYLES.some(s => s[0] === p.style) ? p.style : DEFAULTS.style;
    const accent = isHex(p.accent) ? p.accent : (ACCENTS.find(a => a.id === p.accent) || ACCENTS[0]).id;
    const background = BACKGROUNDS.some(b => b[0] === p.background) ? p.background : DEFAULTS.background;
    return { style, mode: dark ? "dark" : "light", motion, accent, background };
  }

  function tokens(look) {
    const dark = look.mode === "dark";
    const style = STYLES.some(s => s[0] === look.style) ? look.style : DEFAULTS.style;
    const basePal = style === "prism" ? (dark ? PRISM_DARK : PRISM_LIGHT)
      : style === "obsidian-2" ? (dark ? OBSIDIAN2_DARK : OBSIDIAN2_LIGHT)
      : style === "obsidian-aurora" ? (dark ? OBSIDIAN_AURORA_DARK : OBSIDIAN_AURORA_LIGHT)
      : style === "studio" ? (dark ? STUDIO_DARK : STUDIO_LIGHT)
      : style === "aurora" ? (dark ? AURORA_DARK : AURORA_LIGHT)
      : (dark ? DARK : LIGHT);
    const t = { ...basePal };
    Object.assign(t, STYLE_TOKENS[style]);
    t.shell = t.bg;
    t.panel = t.main;
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
    if (v.style === "antigravity") v = { ...v, style: "obsidian" };
    else if (v.style === "companion" || v.style === "gemini") v = { ...v, style: "prism" };
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
  root.Theme = { LIGHT, DARK, ACCENTS, STYLES, STYLE_TOKENS, MODES, BACKGROUNDS, BACKGROUND_HINTS, MOTIONS, MOTION_HINTS, DEFAULTS, LAYOUT_BOUNDS, LAYOUT, navigation, avatar, preferences, contrast, effective, tokens, stage };
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
    const saved = key && parse(key), legacy = key && !saved ? parse(LEGACY_KEY) : null;
    current = preferences(saved || legacy);
    Theme.saved = true;
    // Only a choice is stored, or a record being migrated: defaults stay
    // unstored, so a later change of default reaches everyone who never chose.
    const migrate = saved ? "material" in saved || !("style" in saved) || "conversationWidthPx" in saved : !!legacy;
    if (key && migrate) { try { localStorage.setItem(key, JSON.stringify(current)); } catch { Theme.saved = false; } }
    return Theme.apply();
  };
  Theme.os = () => ({ dark: !!darkQ?.matches, reduceMotion: !!motionQ?.matches, forcedColors: !!forcedQ?.matches });
  Theme.apply = () => {
    const look = effective(current, Theme.os()), el = document.documentElement, t = tokens(look);
    for (const [k, v] of Object.entries(t)) el.style.setProperty(`--${k}`, v);
    el.style.colorScheme = look.mode;
    Object.assign(el.dataset, { style: look.style, mode: look.mode, motion: look.motion, accent: look.accent, background: look.background });
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
