// Aurora dynamic theme engine: living liquid-glass presence orb, live run-state
// ambient sync, and subtle pointer light-catch across cards and composer.
// Strictly gated on html[data-style="aurora"]; zero runtime cost on other themes.
"use strict";

(function () {
  const STATES = {
    idle:      { speed: 0.75, amp: 0.14, c1: [79, 157, 255],  c2: [56, 189, 248],  c3: [147, 197, 253] },
    thinking:  { speed: 2.55, amp: 0.27, c1: [155, 109, 255], c2: [56, 189, 248],  c3: [216, 180, 254] },
    executing: { speed: 2.15, amp: 0.23, c1: [56, 189, 248],  c2: [129, 140, 248], c3: [94, 234, 212] },
    takeover:  { speed: 1.40, amp: 0.20, c1: [251, 191, 36],  c2: [245, 158, 11],  c3: [254, 243, 199] },
  };

  const lerp = (a, b, t) => a + (b - a) * t;
  const lerpRgb = (a, b, t) => [lerp(a[0], b[0], t), lerp(a[1], b[1], t), lerp(a[2], b[2], t)];
  const rgba = (c, a) => `rgba(${c[0] | 0},${c[1] | 0},${c[2] | 0},${a})`;

  let rafId = 0;
  let lastTs = 0;
  let phase = 0;
  let active = false;
  let currentState = "idle";
  let cur = { speed: 0.75, amp: 0.14, c1: [...STATES.idle.c1], c2: [...STATES.idle.c2], c3: [...STATES.idle.c3] };
  const canvases = new Set();

  function isAurora() { return document.documentElement.dataset.style === "aurora"; }
  function motionMode() { return document.documentElement.dataset.motion || "full"; }

  function detectState() {
    if (document.querySelector('.state-pill[data-pill="mine"], .screen.held')) return "takeover";
    if (document.querySelector('.st.working, .state-pill[data-pill="working"], .state-pill[data-pill="waking"]')) return "executing";
    if (document.querySelector(".say:focus-within")) return "thinking";
    return "idle";
  }

  function makeOrb(size, cls) {
    const c = document.createElement("canvas");
    c.className = `aurora-orb ${cls}`;
    c.setAttribute("aria-hidden", "true");
    c.dataset.size = String(size);
    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    c.width = size * dpr;
    c.height = size * dpr;
    c.style.width = `${size}px`;
    c.style.height = `${size}px`;
    const ctx = c.getContext("2d");
    if (ctx) ctx.scale(dpr, dpr);
    return c;
  }

  function syncOrbs() {
    if (!active) return;
    // Sidebar brand presence orb
    document.querySelectorAll("nav.side .brand .logo").forEach((logo) => {
      if (!logo.querySelector(".aurora-orb-nav")) {
        const c = makeOrb(24, "aurora-orb-nav");
        logo.appendChild(c);
        canvases.add(c);
      }
    });
    // Discover / New Chat hero presence orb
    document.querySelectorAll(".hello-head").forEach((head) => {
      if (!head.querySelector(".aurora-orb-hero")) {
        const wrap = document.createElement("div");
        wrap.className = "aurora-hero-presence";
        const c = makeOrb(76, "aurora-orb-hero");
        wrap.appendChild(c);
        head.insertBefore(wrap, head.firstChild);
        canvases.add(c);
      }
    });
    // Prune detached canvases
    for (const c of canvases) {
      if (!c.isConnected) canvases.delete(c);
    }
    const next = detectState();
    if (next !== currentState) {
      currentState = next;
      document.documentElement.dataset.auroraState = next;
    }
  }

  function drawOne(canvas) {
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    const size = Number(canvas.dataset.size || 24);
    const cx = size / 2;
    const cy = size / 2;
    const breath = 1 + Math.sin(phase * 1.4) * (cur.amp * 0.25);
    const baseR = size * 0.34 * breath;

    ctx.clearRect(0, 0, size, size);
    const glow = ctx.createRadialGradient(cx, cy, baseR * 0.25, cx, cy, size * 0.5);
    glow.addColorStop(0, rgba(cur.c1, 0.44));
    glow.addColorStop(0.6, rgba(cur.c2, 0.16));
    glow.addColorStop(1, "rgba(0,0,0,0)");
    ctx.fillStyle = glow;
    ctx.fillRect(0, 0, size, size);

    ctx.save();
    ctx.beginPath();
    const steps = size > 40 ? 56 : 28;
    for (let i = 0; i <= steps; i++) {
      const a = (i / steps) * Math.PI * 2;
      const wobble = (Math.sin(a * 3 + phase * 1.6) * 0.08 + Math.cos(a * 5 - phase * 2.1) * 0.05) * baseR * cur.amp * 4;
      const r = baseR + wobble;
      const x = cx + Math.cos(a) * r;
      const y = cy + Math.sin(a) * r;
      if (i === 0) ctx.moveTo(x, y); else ctx.lineTo(x, y);
    }
    ctx.closePath();
    ctx.clip();

    const core = ctx.createRadialGradient(cx - baseR * 0.28, cy - baseR * 0.32, baseR * 0.1, cx, cy, baseR * 1.25);
    core.addColorStop(0, rgba(cur.c3, 0.95));
    core.addColorStop(0.45, rgba(cur.c1, 0.88));
    core.addColorStop(0.85, rgba(cur.c2, 0.68));
    core.addColorStop(1, "rgba(8,12,24,0.92)");
    ctx.fillStyle = core;
    ctx.fillRect(0, 0, size, size);

    ctx.globalCompositeOperation = "lighter";
    const ox = cx + Math.cos(phase * 1.3) * baseR * 0.35;
    const oy = cy + Math.sin(phase * 1.5) * baseR * 0.35;
    const rib = ctx.createRadialGradient(ox, oy, 1, ox, oy, baseR * 0.78);
    rib.addColorStop(0, rgba(cur.c3, 0.6));
    rib.addColorStop(1, "rgba(0,0,0,0)");
    ctx.fillStyle = rib;
    ctx.fillRect(0, 0, size, size);
    ctx.restore();

    ctx.beginPath();
    ctx.arc(cx, cy, Math.max(1, baseR - 0.8), -Math.PI * 0.85, -Math.PI * 0.15);
    ctx.strokeStyle = "rgba(255,255,255,0.42)";
    ctx.lineWidth = size > 40 ? 1.4 : 0.9;
    ctx.lineCap = "round";
    ctx.stroke();
  }

  function tick(now) {
    if (!active) { rafId = 0; return; }
    const dt = Math.min((now - lastTs) / 1000, 0.05);
    lastTs = now;
    const target = STATES[currentState] || STATES.idle;
    const blend = 1 - Math.exp(-dt * 4.5);
    cur.speed = lerp(cur.speed, target.speed, blend);
    cur.amp = lerp(cur.amp, target.amp, blend);
    cur.c1 = lerpRgb(cur.c1, target.c1, blend);
    cur.c2 = lerpRgb(cur.c2, target.c2, blend);
    cur.c3 = lerpRgb(cur.c3, target.c3, blend);

    if (motionMode() !== "off") phase += dt * cur.speed;
    for (const c of canvases) if (c.isConnected) drawOne(c);

    if (motionMode() === "off") { rafId = 0; return; }
    rafId = requestAnimationFrame(tick);
  }

  // Subtle ambient pointer light-catch on Aurora interactive surfaces
  let pointerQueued = false;
  let lastPointerEvent = null;
  function onPointerMove(e) {
    if (!active || motionMode() !== "full") return;
    lastPointerEvent = e;
    if (pointerQueued) return;
    pointerQueued = true;
    requestAnimationFrame(() => {
      pointerQueued = false;
      const ev = lastPointerEvent;
      if (!ev || !(ev.target instanceof Element)) return;
      const card = ev.target.closest(".hello-card, .explore-card, .card, .say");
      if (!card) return;
      const rect = card.getBoundingClientRect();
      card.style.setProperty("--aurora-mx", `${((ev.clientX - rect.left) / Math.max(1, rect.width)) * 100}%`);
      card.style.setProperty("--aurora-my", `${((ev.clientY - rect.top) / Math.max(1, rect.height)) * 100}%`);
    });
  }

  function teardown() {
    active = false;
    if (rafId) { cancelAnimationFrame(rafId); rafId = 0; }
    delete document.documentElement.dataset.auroraState;
    document.querySelectorAll(".aurora-hero-presence, .aurora-orb").forEach((el) => el.remove());
    canvases.clear();
  }

  function reconcile() {
    if (isAurora()) {
      const wasActive = active;
      active = true;
      document.documentElement.dataset.auroraState = currentState;
      syncOrbs();
      if (!wasActive || (!rafId && motionMode() !== "off")) {
        lastTs = performance.now();
        if (motionMode() === "off") {
          for (const c of canvases) if (c.isConnected) drawOne(c);
        } else {
          rafId = requestAnimationFrame(tick);
        }
      }
    } else if (active) {
      teardown();
    }
  }

  // Observe style/motion changes on <html> and dynamic DOM mounts on <body>
  const htmlObs = new MutationObserver(reconcile);
  htmlObs.observe(document.documentElement, { attributes: true, attributeFilter: ["data-style", "data-motion"] });

  let domTimer = 0;
  const bodyObs = new MutationObserver(() => {
    if (!active || domTimer) return;
    domTimer = setTimeout(() => { domTimer = 0; syncOrbs(); }, 80);
  });

  function init() {
    bodyObs.observe(document.body, { childList: true, subtree: true, attributes: true, attributeFilter: ["class", "data-pill"] });
    document.addEventListener("pointermove", onPointerMove, { passive: true });
    document.addEventListener("focusin", () => { if (active) syncOrbs(); }, true);
    document.addEventListener("focusout", () => { if (active) syncOrbs(); }, true);
    reconcile();
  }

  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", init);
  else init();
})();
