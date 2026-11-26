// Expressive client-side companion avatars for Magnus (Nova & Aurora):
// - "pip":   glass-ceramic companion with cursor-tracking eyes, natural blink,
//            listening ripples, scanning working eyes, waiting head-tilt, and
//            celebratory done bounce.
// - "prism": faceted luminous 4-point star sprite with orbital shard.
// Both implement AuroraAvatar.define's contract with zero server cost and full
// reduced-motion support (static high-craft frame per state).
"use strict";

const COMPANION_TURN_MS = 480;

(() => {
  if (typeof AuroraAvatar === "undefined") return;

  /** @typedef {[number, number, number]} Rgb */
  /** @typedef {{ c1: Rgb, c2: Rgb, glow: Rgb }} Pal */
  /** @type {Record<string, Pal>} */
  const PALETTES = {
    ready:     { c1: [59, 130, 246],  c2: [56, 189, 248],  glow: [59, 130, 246] },
    listening: { c1: [52, 211, 153],  c2: [56, 189, 248],  glow: [52, 211, 153] },
    thinking:  { c1: [167, 139, 250], c2: [99, 102, 241],  glow: [167, 139, 250] },
    planning:  { c1: [129, 140, 248], c2: [94, 234, 212],  glow: [129, 140, 248] },
    working:   { c1: [56, 189, 248],  c2: [99, 102, 241],  glow: [56, 189, 248] },
    waiting:   { c1: [251, 191, 36],  c2: [245, 158, 11],  glow: [251, 191, 36] },
    control:   { c1: [251, 191, 36],  c2: [148, 163, 184], glow: [251, 191, 36] },
    done:      { c1: [52, 211, 153],  c2: [16, 185, 129],  glow: [52, 211, 153] },
    failed:    { c1: [251, 113, 133], c2: [225, 29, 72],   glow: [251, 113, 133] },
  };

  const moving = () => document.documentElement.dataset.motion === "full";
  const mix = (/** @type {number} */ a, /** @type {number} */ b, /** @type {number} */ t) => a + (b - a) * t;
  const mixRgb = (/** @type {Rgb} */ a, /** @type {Rgb} */ b, /** @type {number} */ t) => /** @type {Rgb} */ ([mix(a[0], b[0], t), mix(a[1], b[1], t), mix(a[2], b[2], t)]);
  const rgba = (/** @type {Rgb} */ c, /** @type {number} */ a) => `rgba(${c[0] | 0},${c[1] | 0},${c[2] | 0},${a.toFixed(3)})`;
  const palOf = (/** @type {string} */ s) => PALETTES[s] || PALETTES.ready;

  /** @type {Set<{ draw: (sec: number) => void }>} */
  const live = new Set();
  let frame = 0;
  let unsubCore = null;

  function tickAll(/** @type {number} */ now) {
    const sec = now / 1000;
    for (const o of live) o.draw(sec);
  }
  function loop(/** @type {number} */ now) {
    frame = 0;
    if (document.hidden || !moving() || !live.size) return;
    tickAll(now);
    frame = requestAnimationFrame(loop);
  }
  const wake = () => {
    if (typeof AuroraCore !== "undefined") {
      if (live.size && !unsubCore) unsubCore = AuroraCore.onFrame(now => tickAll(now));
      else if (!live.size && unsubCore) { unsubCore(); unsubCore = null; }
      AuroraCore.wake();
      return;
    }
    if (!frame && moving() && !document.hidden && live.size) frame = requestAnimationFrame(loop);
  };
  document.addEventListener("visibilitychange", wake);
  window.addEventListener("magnus-theme", () => { wake(); tickAll(performance.now()); });

  /**
   * Shared lifecycle factory for canvas-drawn companions.
   * @param {(ctx: CanvasRenderingContext2D, s: number, t: number, st: string, p: Pal, lx: number, ly: number) => void} render
   */
  function makeCanvasCompanion(render) {
    return (/** @type {HTMLElement} */ host, /** @type {{ size: number }} */ { size }) => {
      const c = document.createElement("canvas");
      c.className = "nova-companion-canvas";
      c.setAttribute("aria-hidden", "true");
      const dpr = Math.min(window.devicePixelRatio || 1, 2);
      c.width = size * dpr;
      c.height = size * dpr;
      c.style.width = `${size}px`;
      c.style.height = `${size}px`;
      host.append(c);
      const ctx = c.getContext("2d");
      ctx?.scale(dpr, dpr);
      let from = palOf("ready"), to = from, since = 0, st = "ready";
      let lx = 0, ly = 0, tx = 0, ty = 0;

      /** @param {number} t seconds */
      function draw(t) {
        if (!ctx) return;
        const k = moving() ? Math.min(1, (performance.now() - since) / COMPANION_TURN_MS) : 1;
        const e = 1 - Math.pow(1 - k, 3);
        const p = { c1: mixRgb(from.c1, to.c1, e), c2: mixRgb(from.c2, to.c2, e), glow: mixRgb(from.glow, to.glow, e) };
        lx = mix(lx, tx, moving() ? 0.1 : 1);
        ly = mix(ly, ty, moving() ? 0.1 : 1);
        render(ctx, size, moving() ? t : 0, st, p, lx, ly);
      }

      const runner = { draw };
      live.add(runner);
      wake();
      draw(performance.now() / 1000);

      return {
        set(next) {
          if (next === st) return;
          from = to;
          to = palOf(next);
          st = next;
          since = performance.now();
          if (!moving()) draw(0);
          else wake();
        },
        look(x, y) { tx = x; ty = y; },
        destroy() {
          live.delete(runner);
          if (!live.size && unsubCore) { unsubCore(); unsubCore = null; }
          c.remove();
        },
      };
    };
  }

  function roundedRect(ctx, x, y, w, h, r) {
    if (typeof ctx.roundRect === "function") { ctx.roundRect(x, y, w, h, r); return; }
    const rad = Math.min(r, w / 2, h / 2);
    ctx.moveTo(x + rad, y);
    ctx.arcTo(x + w, y, x + w, y + h, rad);
    ctx.arcTo(x + w, y + h, x, y + h, rad);
    ctx.arcTo(x, y + h, x, y, rad);
    ctx.arcTo(x, y, x + w, y, rad);
    ctx.closePath();
  }

  AuroraAvatar.define("pip", makeCanvasCompanion((ctx, s, t, st, p, lx, ly) => {
    const cx = s / 2, cy = s / 2;
    const bob = st === "done" ? Math.abs(Math.sin(t * 5)) * -s * 0.065 : Math.sin(t * 2.4) * s * 0.03;
    const tilt = st === "waiting" ? Math.sin(t * 2.1) * 0.12 : lx * 0.1;

    ctx.save();
    ctx.clearRect(0, 0, s, s);
    const aura = ctx.createRadialGradient(cx, cy + bob, s * 0.08, cx, cy + bob, s * 0.48);
    aura.addColorStop(0, rgba(p.glow, 0.36));
    aura.addColorStop(1, "rgba(0,0,0,0)");
    ctx.fillStyle = aura;
    ctx.fillRect(0, 0, s, s);

    ctx.translate(cx + lx * s * 0.04, cy + bob + ly * s * 0.03);
    ctx.rotate(tilt);

    if (st === "listening" && t > 0) {
      const rp = (t * 0.9) % 1;
      ctx.beginPath();
      ctx.arc(0, 0, s * (0.34 + rp * 0.16), 0, Math.PI * 2);
      ctx.strokeStyle = rgba(p.c1, (1 - rp) * 0.55);
      ctx.lineWidth = Math.max(1.2, s * 0.025);
      ctx.stroke();
    }

    // Antenna spark
    ctx.beginPath();
    ctx.moveTo(0, -s * 0.27);
    ctx.lineTo(0, -s * 0.37);
    ctx.strokeStyle = rgba(p.c2, 0.95);
    ctx.lineWidth = Math.max(1.4, s * 0.034);
    ctx.lineCap = "round";
    ctx.stroke();

    ctx.beginPath();
    ctx.arc(0, -s * 0.39, s * (st === "thinking" ? 0.052 + Math.sin(t * 7) * 0.012 : 0.044), 0, Math.PI * 2);
    ctx.fillStyle = rgba(p.c2, 1);
    ctx.fill();

    // Ceramic squircle body
    const bw = s * 0.62, bh = s * 0.54, br = s * 0.22;
    const grad = ctx.createLinearGradient(-bw / 2, -bh / 2, bw / 2, bh / 2);
    grad.addColorStop(0, rgba(p.c1, 1));
    grad.addColorStop(1, rgba(p.c2, 1));
    ctx.beginPath();
    roundedRect(ctx, -bw / 2, -bh / 2, bw, bh, br);
    ctx.fillStyle = grad;
    ctx.fill();

    // Top specular highlight
    ctx.beginPath();
    roundedRect(ctx, -bw * 0.38, -bh * 0.42, bw * 0.76, bh * 0.2, br * 0.6);
    ctx.fillStyle = "rgba(255,255,255,0.28)";
    ctx.fill();

    // Dark glass visor
    const vw = bw * 0.76, vh = bh * 0.54;
    ctx.beginPath();
    roundedRect(ctx, -vw / 2, -vh * 0.42, vw, vh, vh * 0.42);
    ctx.fillStyle = "#0b0d14";
    ctx.fill();

    // Expressive eyes
    const ex = vw * 0.22, ey = vh * 0.04;
    const gx = lx * vw * 0.09, gy = ly * vh * 0.08;
    if (st === "done") {
      ctx.strokeStyle = "#ffffff";
      ctx.lineWidth = Math.max(1.5, s * 0.04);
      ctx.lineCap = "round";
      for (const x of [-ex, ex]) {
        ctx.beginPath();
        ctx.arc(x + gx, ey + s * 0.02, s * 0.05, Math.PI * 1.15, Math.PI * 1.85);
        ctx.stroke();
      }
    } else if (st === "failed") {
      ctx.strokeStyle = "#fecdd3";
      ctx.lineWidth = Math.max(1.5, s * 0.038);
      ctx.lineCap = "round";
      ctx.beginPath();
      ctx.moveTo(-ex - s * 0.04, ey - s * 0.02); ctx.lineTo(-ex + s * 0.04, ey + s * 0.02);
      ctx.moveTo(ex + s * 0.04, ey - s * 0.02); ctx.lineTo(ex - s * 0.04, ey + s * 0.02);
      ctx.stroke();
    } else if (st === "working" || st === "planning") {
      const scan = Math.sin(t * 5) * vw * 0.08;
      ctx.fillStyle = "#7dd3fc";
      for (const x of [-ex, ex]) {
        ctx.beginPath();
        roundedRect(ctx, x + scan - s * 0.045, ey - s * 0.024, s * 0.09, s * 0.048, s * 0.024);
        ctx.fill();
      }
    } else {
      const blink = (t > 0 && Math.sin(t * 0.85) > 0.97 && st === "ready") ? 0.14 : 1;
      ctx.fillStyle = st === "waiting" || st === "control" ? "#fde68a" : "#ffffff";
      for (const [idx, x] of [-ex, ex].entries()) {
        const sc = (st === "waiting" && idx === 1) ? 1.16 : 1;
        ctx.beginPath();
        ctx.ellipse(x + gx, ey + gy, s * 0.045 * sc, Math.max(1, s * 0.055 * blink * sc), 0, 0, Math.PI * 2);
        ctx.fill();
      }
    }

    if (st === "working" || st === "thinking" || st === "planning") {
      ctx.beginPath();
      ctx.arc(0, 0, s * 0.43, t * 3.2, t * 3.2 + Math.PI * 0.65);
      ctx.strokeStyle = rgba(p.c2, 0.9);
      ctx.lineWidth = Math.max(1.4, s * 0.032);
      ctx.lineCap = "round";
      ctx.stroke();
    }
    ctx.restore();
  }));

  AuroraAvatar.define("prism", makeCanvasCompanion((ctx, s, t, st, p, lx, ly) => {
    const cx = s / 2 + lx * s * 0.05;
    const cy = s / 2 + ly * s * 0.05 + Math.sin(t * 2.5) * s * 0.03;
    const r = s * 0.34;
    ctx.save();
    ctx.clearRect(0, 0, s, s);
    const glow = ctx.createRadialGradient(cx, cy, r * 0.1, cx, cy, s * 0.48);
    glow.addColorStop(0, rgba(p.glow, 0.38));
    glow.addColorStop(1, "rgba(0,0,0,0)");
    ctx.fillStyle = glow;
    ctx.fillRect(0, 0, s, s);

    ctx.translate(cx, cy);
    ctx.rotate(st === "working" || st === "thinking" ? t * 1.45 : Math.sin(t * 1.1) * 0.1);
    const g = ctx.createLinearGradient(-r, -r, r, r);
    g.addColorStop(0, "#ffffff");
    g.addColorStop(0.35, rgba(p.c2, 1));
    g.addColorStop(1, rgba(p.c1, 1));
    ctx.beginPath();
    for (let i = 0; i < 8; i++) {
      const a = (i * Math.PI) / 4;
      const rad = i % 2 === 0 ? r : r * 0.42;
      if (i === 0) ctx.moveTo(Math.cos(a) * rad, Math.sin(a) * rad);
      else ctx.lineTo(Math.cos(a) * rad, Math.sin(a) * rad);
    }
    ctx.closePath();
    ctx.fillStyle = g;
    ctx.fill();

    const ox = Math.cos(t * 2.6) * r * 1.04;
    const oy = Math.sin(t * 2.6) * r * 0.54;
    ctx.beginPath();
    ctx.arc(ox, oy, Math.max(1.8, s * 0.042), 0, Math.PI * 2);
    ctx.fillStyle = rgba(p.c2, 0.95);
    ctx.fill();
    ctx.restore();
  }));

  AuroraAvatar.define("mochi", makeCanvasCompanion((ctx, s, t, st, p, lx, ly) => {
    const cx = s / 2, cy = s / 2;
    const bounce = st === "done" ? Math.abs(Math.sin(t * 5.4)) * -s * 0.08 : Math.sin(t * 2.6) * s * 0.032;
    const squashX = st === "done" ? 1 + Math.sin(t * 10.8) * 0.06 : 1 + Math.sin(t * 2.6) * 0.025;
    const squashY = 2 - squashX;
    const tilt = st === "waiting" ? Math.sin(t * 2.0) * 0.14 : lx * 0.09;

    ctx.save();
    ctx.clearRect(0, 0, s, s);
    const aura = ctx.createRadialGradient(cx, cy + bounce, s * 0.1, cx, cy + bounce, s * 0.49);
    aura.addColorStop(0, rgba(p.glow, 0.38));
    aura.addColorStop(1, "rgba(0,0,0,0)");
    ctx.fillStyle = aura;
    ctx.fillRect(0, 0, s, s);

    ctx.translate(cx + lx * s * 0.045, cy + bounce + ly * s * 0.035);
    ctx.rotate(tilt);
    ctx.scale(squashX, squashY);

    if (st === "listening" && t > 0) {
      const rp = (t * 0.95) % 1;
      ctx.beginPath();
      ctx.arc(0, 0, s * (0.36 + rp * 0.15), 0, Math.PI * 2);
      ctx.strokeStyle = rgba(p.c1, (1 - rp) * 0.55);
      ctx.lineWidth = Math.max(1.2, s * 0.025);
      ctx.stroke();
    }

    const earWig = st === "listening" ? Math.sin(t * 8) * 0.12 : st === "waiting" ? 0.18 : Math.sin(t * 2.2) * 0.05;
    const earGrad = ctx.createLinearGradient(0, -s * 0.4, 0, -s * 0.1);
    earGrad.addColorStop(0, rgba(p.c2, 1));
    earGrad.addColorStop(1, rgba(p.c1, 1));
    for (const side of [-1, 1]) {
      ctx.save();
      ctx.translate(side * s * 0.19, -s * 0.21);
      ctx.rotate(side * (0.34 + earWig));
      ctx.beginPath();
      ctx.ellipse(0, -s * 0.07, s * 0.085, s * 0.14, 0, 0, Math.PI * 2);
      ctx.fillStyle = earGrad;
      ctx.fill();
      ctx.beginPath();
      ctx.ellipse(0, -s * 0.06, s * 0.042, s * 0.078, 0, 0, Math.PI * 2);
      ctx.fillStyle = "rgba(255,255,255,0.45)";
      ctx.fill();
      ctx.restore();
    }

    const bodyGrad = ctx.createRadialGradient(-s * 0.1, -s * 0.14, s * 0.04, 0, 0, s * 0.36);
    bodyGrad.addColorStop(0, "#ffffff");
    bodyGrad.addColorStop(0.55, "#eef4ff");
    bodyGrad.addColorStop(1, rgba(p.c2, 1));
    ctx.beginPath();
    ctx.ellipse(0, s * 0.03, s * 0.31, s * 0.27, 0, 0, Math.PI * 2);
    ctx.fillStyle = bodyGrad;
    ctx.fill();
    ctx.strokeStyle = rgba(p.c1, 0.9);
    ctx.lineWidth = Math.max(1, s * 0.018);
    ctx.stroke();

    const gx = lx * s * 0.055, gy = ly * s * 0.04;
    for (const side of [-1, 1]) {
      ctx.beginPath();
      ctx.ellipse(side * s * 0.165 + gx * 0.7, s * 0.095 + gy * 0.7, s * 0.052, s * 0.03, 0, 0, Math.PI * 2);
      ctx.fillStyle = st === "waiting" ? "rgba(251,191,36,0.45)" : "rgba(244,114,182,0.42)";
      ctx.fill();
    }

    const ex = s * 0.105, ey = s * 0.02;
    if (st === "done") {
      ctx.strokeStyle = "#111827";
      ctx.lineWidth = Math.max(1.6, s * 0.036);
      ctx.lineCap = "round";
      for (const x of [-ex, ex]) {
        ctx.beginPath();
        ctx.arc(x + gx, ey + gy + s * 0.015, s * 0.048, Math.PI * 1.15, Math.PI * 1.85);
        ctx.stroke();
      }
    } else if (st === "failed") {
      ctx.strokeStyle = "#881337";
      ctx.lineWidth = Math.max(1.5, s * 0.034);
      ctx.lineCap = "round";
      ctx.beginPath();
      ctx.moveTo(-ex - s * 0.04, ey - s * 0.02); ctx.lineTo(-ex + s * 0.04, ey + s * 0.02);
      ctx.moveTo(ex + s * 0.04, ey - s * 0.02); ctx.lineTo(ex - s * 0.04, ey + s * 0.02);
      ctx.stroke();
    } else {
      const blink = (t > 0 && Math.sin(t * 0.9) > 0.97 && st === "ready") ? 0.15 : 1;
      const scan = (st === "working" || st === "planning") ? Math.sin(t * 5.2) * s * 0.035 : 0;
      for (const x of [-ex, ex]) {
        const rx = s * 0.044;
        const ry = Math.max(1.1, s * 0.054 * blink * ((st === "working" || st === "planning") ? 0.72 : 1));
        ctx.beginPath();
        ctx.ellipse(x + gx + scan, ey + gy, rx, ry, 0, 0, Math.PI * 2);
        ctx.fillStyle = "#111827";
        ctx.fill();
        if (blink > 0.5) {
          ctx.beginPath();
          ctx.arc(x + gx + scan - rx * 0.32, ey + gy - ry * 0.34, Math.max(1, s * 0.018), 0, Math.PI * 2);
          ctx.fillStyle = "#ffffff";
          ctx.fill();
        }
      }
    }

    ctx.strokeStyle = "#111827";
    ctx.lineWidth = Math.max(1.3, s * 0.026);
    ctx.lineCap = "round";
    if (st === "waiting" || st === "control") {
      ctx.beginPath();
      ctx.arc(gx, s * 0.105 + gy, s * 0.024, 0, Math.PI * 2);
      ctx.stroke();
    } else if (st === "working" || st === "planning") {
      ctx.beginPath();
      ctx.moveTo(-s * 0.03 + gx, s * 0.105 + gy);
      ctx.lineTo(s * 0.03 + gx, s * 0.105 + gy);
      ctx.stroke();
    } else {
      ctx.beginPath();
      ctx.arc(gx, s * 0.088 + gy, s * 0.038, 0.2 * Math.PI, 0.8 * Math.PI);
      ctx.stroke();
    }

    if (st === "thinking") {
      const py = -s * 0.34 - Math.sin(t * 4) * s * 0.025;
      ctx.beginPath();
      ctx.arc(s * 0.25, py, s * 0.042, 0, Math.PI * 2);
      ctx.fillStyle = rgba(p.c1, 0.95);
      ctx.fill();
      ctx.beginPath();
      ctx.arc(s * 0.18, py + s * 0.075, s * 0.022, 0, Math.PI * 2);
      ctx.fillStyle = rgba(p.c2, 0.95);
      ctx.fill();
    }
    ctx.restore();
  }));
})();
