// The orb: Aurora's avatar for Magnus (decisions/2026-11-21-aurora-avatar.md),
// one avatar among any that meet aurora-avatar.js's contract. A living glass
// orb whose colour and shape follow the state Magnus is in:
//   ready      blue, round, breathing slowly
//   listening  green, ripples going out
//   thinking   violet, a light swirling inside
//   planning   indigo, three points going round a dashed ring
//   working    cyan, a bright arc running round its rim
//   waiting    amber, a steady halo: it waits on the person
//   control    amber, hollow: the person has the controls
//   done       green, round and still, with a tick
//   failed     rose, flattened and dim, with a bar
// It changes from one state to the next over a moment, and leans toward the
// pointer. With reduced motion or none it is drawn once for each change.
"use strict";

/** How long the orb takes to turn from one state's look to the next. */
const ORB_TURN_MS = 600;

(() => {
  if (typeof AuroraAvatar === "undefined") return;
  /** @typedef {[number, number, number]} Rgb */
  /** @typedef {{ c1: Rgb, c2: Rgb, c3: Rgb, speed: number, amp: number, mark: string }} Look */
  /** @type {Record<string, Look>} */
  const AURORA = {
    ready: { c1: [79, 157, 255], c2: [56, 189, 248], c3: [167, 139, 250], speed: 0.9, amp: 0.16, mark: "" },
    listening: { c1: [52, 211, 153], c2: [56, 189, 248], c3: [167, 243, 208], speed: 1.6, amp: 0.24, mark: "ripples" },
    thinking: { c1: [155, 109, 255], c2: [56, 189, 248], c3: [216, 180, 254], speed: 2.6, amp: 0.28, mark: "swirl" },
    planning: { c1: [129, 140, 248], c2: [94, 234, 212], c3: [199, 210, 254], speed: 1.8, amp: 0.2, mark: "dots" },
    working: { c1: [56, 189, 248], c2: [129, 140, 248], c3: [94, 234, 212], speed: 2.3, amp: 0.24, mark: "arc" },
    waiting: { c1: [251, 191, 36], c2: [245, 158, 11], c3: [254, 243, 199], speed: 1.1, amp: 0.18, mark: "halo" },
    control: { c1: [251, 191, 36], c2: [148, 163, 184], c3: [254, 243, 199], speed: 0.7, amp: 0.1, mark: "hollow" },
    done: { c1: [52, 211, 153], c2: [16, 185, 129], c3: [209, 250, 229], speed: 0.6, amp: 0.08, mark: "tick" },
    failed: { c1: [251, 113, 133], c2: [225, 29, 72], c3: [254, 205, 211], speed: 0.5, amp: 0.06, mark: "bar" },
  };
  const look = (/** @type {string} */ s) => ({ ...(AURORA[s] || AURORA.ready) });
  const moving = () => document.documentElement.dataset.motion === "full";
  const mix = (/** @type {number} */ a, /** @type {number} */ b, /** @type {number} */ t) => a + (b - a) * t;
  const mixRgb = (/** @type {Rgb} */ a, /** @type {Rgb} */ b, /** @type {number} */ t) => /** @type {Rgb} */ ([mix(a[0], b[0], t), mix(a[1], b[1], t), mix(a[2], b[2], t)]);
  const rgba = (/** @type {Rgb} */ c, /** @type {number} */ a) => `rgba(${c[0] | 0},${c[1] | 0},${c[2] | 0},${a.toFixed(3)})`;

  /** @type {Set<{ draw: (t: number) => void }>} */
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

  AuroraAvatar.define("orb", (host, { size }) => {
    const c = document.createElement("canvas");
    c.className = "aurora-orb-avatar";
    c.setAttribute("aria-hidden", "true");
    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    c.width = size * dpr;
    c.height = size * dpr;
    c.style.width = `${size}px`;
    c.style.height = `${size}px`;
    host.append(c);
    const ctx = c.getContext("2d");
    ctx?.scale(dpr, dpr);
    let from = look("ready"), to = from, since = 0, named = "ready";
    let lx = 0, ly = 0, tx = 0, ty = 0;

    /** @param {number} t seconds */
    function draw(t) {
      if (!ctx) return;
      const k = moving() ? Math.min(1, (performance.now() - since) / ORB_TURN_MS) : 1;
      const e = 1 - Math.pow(1 - k, 3);
      const p = { c1: mixRgb(from.c1, to.c1, e), c2: mixRgb(from.c2, to.c2, e), c3: mixRgb(from.c3, to.c3, e), speed: mix(from.speed, to.speed, e), amp: mix(from.amp, to.amp, e) };
      const ph = moving() ? t * p.speed : 0;
      lx = mix(lx, tx, moving() ? 0.08 : 1); ly = mix(ly, ty, moving() ? 0.08 : 1);
      const cx = size / 2 + lx * size * 0.06, cy = size / 2 + ly * size * 0.06;
      const squash = to.mark === "bar" ? 0.86 : 1;
      const r = size * 0.33 * (1 + Math.sin(ph * 1.55) * p.amp * 0.28);
      ctx.clearRect(0, 0, size, size);
      const glow = ctx.createRadialGradient(cx, cy, r * 0.25, cx, cy, size * 0.5);
      glow.addColorStop(0, rgba(p.c1, to.mark === "bar" ? 0.28 : 0.48));
      glow.addColorStop(0.6, rgba(p.c2, 0.2));
      glow.addColorStop(1, "rgba(0,0,0,0)");
      ctx.fillStyle = glow;
      ctx.fillRect(0, 0, size, size);
      // The body: a wobbling round, its edge moving with how busy it is.
      ctx.save();
      ctx.beginPath();
      const steps = size > 48 ? 60 : 28;
      for (let i = 0; i <= steps; i++) {
        const a = (i / steps) * Math.PI * 2;
        const w = (Math.sin(a * 3 + ph * 1.8) * 0.085 + Math.cos(a * 5 - ph * 2.3) * 0.055) * r * p.amp * 4;
        const x = cx + Math.cos(a) * (r + w), y = cy + Math.sin(a) * (r + w) * squash;
        if (i === 0) ctx.moveTo(x, y); else ctx.lineTo(x, y);
      }
      ctx.closePath();
      if (to.mark === "hollow") {
        ctx.lineWidth = Math.max(1.5, size * 0.07);
        ctx.strokeStyle = rgba(p.c1, 0.9);
        ctx.stroke();
        ctx.restore();
        return;
      }
      ctx.clip();
      const core = ctx.createRadialGradient(cx - r * 0.28 + lx * r * 0.3, cy - r * 0.32 + ly * r * 0.3, r * 0.1, cx, cy, r * 1.25);
      core.addColorStop(0, rgba(p.c3, 0.96));
      core.addColorStop(0.45, rgba(p.c1, 0.88));
      core.addColorStop(0.85, rgba(p.c2, 0.68));
      core.addColorStop(1, "rgba(8,12,24,0.92)");
      ctx.fillStyle = core;
      ctx.fillRect(0, 0, size, size);
      // Dual counter-orbiting internal light ribbons (matching magnus-website.html optical depth).
      if (to.mark !== "bar" && to.mark !== "tick") {
        ctx.globalCompositeOperation = "lighter";
        const spin = to.mark === "swirl" ? 1.8 : 1.2;
        for (let k2 = 0; k2 < 2; k2++) {
          const ox = cx + Math.cos(ph * (spin + k2 * 0.55) + k2 * 2.1) * r * 0.4;
          const oy = cy + Math.sin(ph * (spin * 1.15 + k2 * 0.45) + k2 * 1.4) * r * 0.4;
          const rib = ctx.createRadialGradient(ox, oy, 1, ox, oy, r * 0.82);
          rib.addColorStop(0, rgba(k2 === 0 ? p.c3 : p.c2, to.mark === "swirl" ? 0.72 : 0.56));
          rib.addColorStop(1, "rgba(0,0,0,0)");
          ctx.fillStyle = rib;
          ctx.fillRect(0, 0, size, size);
        }
      }
      ctx.restore();
      // The shine, then the state's own mark.
      ctx.beginPath();
      ctx.arc(cx, cy, Math.max(1, r - 0.8), -Math.PI * 0.85, -Math.PI * 0.15);
      ctx.strokeStyle = `rgba(255,255,255,${to.mark === "bar" ? 0.2 : 0.48})`;
      ctx.lineWidth = size > 48 ? 1.5 : 0.95;
      ctx.lineCap = "round";
      ctx.stroke();
      const line = Math.max(1.2, size * 0.05);
      ctx.lineWidth = line;
      if (to.mark === "arc") {
        const a0 = ph * 2.4;
        ctx.beginPath();
        ctx.arc(cx, cy, r + line * 1.6, a0, a0 + Math.PI * 0.7);
        ctx.strokeStyle = rgba(p.c3, 0.95);
        ctx.stroke();
      } else if (to.mark === "dots") {
        ctx.setLineDash([line, line * 1.6]);
        ctx.beginPath();
        ctx.arc(cx, cy, r + line * 1.6, 0, Math.PI * 2);
        ctx.strokeStyle = rgba(p.c1, 0.5);
        ctx.stroke();
        ctx.setLineDash([]);
        for (let i = 0; i < 3; i++) {
          const a = ph * 1.2 + (i * Math.PI * 2) / 3;
          ctx.beginPath();
          ctx.arc(cx + Math.cos(a) * (r + line * 1.6), cy + Math.sin(a) * (r + line * 1.6), line * 1.1, 0, Math.PI * 2);
          ctx.fillStyle = rgba(p.c3, 1);
          ctx.fill();
        }
      } else if (to.mark === "halo" || to.mark === "ripples") {
        const n = to.mark === "ripples" ? 2 : 1;
        for (let i = 0; i < n; i++) {
          const g = moving() ? ((ph * 0.5 + i / n) % 1) : 0.5;
          ctx.beginPath();
          ctx.arc(cx, cy, r + line + g * size * 0.12, 0, Math.PI * 2);
          ctx.strokeStyle = rgba(p.c1, to.mark === "halo" ? 0.85 - g * 0.4 : 0.7 * (1 - g));
          ctx.stroke();
        }
      } else if (to.mark === "tick" && size >= 24) {
        ctx.beginPath();
        ctx.moveTo(cx - r * 0.38, cy + r * 0.02);
        ctx.lineTo(cx - r * 0.08, cy + r * 0.3);
        ctx.lineTo(cx + r * 0.42, cy - r * 0.3);
        ctx.strokeStyle = "rgba(255,255,255,0.92)";
        ctx.lineWidth = Math.max(1.6, size * 0.07);
        ctx.stroke();
      } else if (to.mark === "bar" && size >= 24) {
        ctx.beginPath();
        ctx.moveTo(cx - r * 0.36, cy);
        ctx.lineTo(cx + r * 0.36, cy);
        ctx.strokeStyle = "rgba(255,255,255,0.85)";
        ctx.lineWidth = Math.max(1.6, size * 0.07);
        ctx.stroke();
      }
    }

    const me = { draw };
    live.add(me);
    return {
      set(/** @type {string} */ s) {
        // The same state again: its look may have changed with the style.
        if (s === named && since) { to = look(s); from = to; draw(performance.now() / 1000); return; }
        from = since ? to : look(s);
        to = look(s);
        named = s;
        since = performance.now();
        c.dataset.state = s;
        draw(performance.now() / 1000);
        wake();
      },
      look(/** @type {number} */ x, /** @type {number} */ y) { tx = x; ty = y; wake(); },
      destroy() { live.delete(me); c.remove(); },
    };
  });
})();
