// Aurora dynamic theme engine:
// 1. Full-viewport living ambient aurora light canvas behind floating glass islands
// 2. Living liquid-glass presence orbs in sidebar brand, right-sidebar companion avatar, and hero
// 3. Live state detection across voice listening, thinking, tool execution, and human takeover
// 4. Choreographed accordion toggling on completed work cards & masked entrance on streamed answers
// 5. Tactile pointer light-catch across glass cards, nav items, and composer
// Strictly gated on html[data-style="aurora"]; zero runtime cost on other themes.
"use strict";

(function () {
  const AURORA_STATES = {
    idle:      { speed: 0.72, amp: 0.14, c1: [79, 157, 255],  c2: [56, 189, 248],  c3: [147, 197, 253] },
    listening: { speed: 1.55, amp: 0.25, c1: [52, 211, 153],  c2: [56, 189, 248],  c3: [167, 243, 208] },
    thinking:  { speed: 2.55, amp: 0.27, c1: [155, 109, 255], c2: [56, 189, 248],  c3: [216, 180, 254] },
    executing: { speed: 2.20, amp: 0.23, c1: [56, 189, 248],  c2: [129, 140, 248], c3: [94, 234, 212] },
    takeover:  { speed: 1.35, amp: 0.20, c1: [251, 191, 36],  c2: [245, 158, 11],  c3: [254, 243, 199] },
  };
  const OBSIDIAN_STATES = {
    idle:      { speed: 0.68, amp: 0.12, c1: [212, 212, 220], c2: [113, 128, 150], c3: [248, 250, 252] },
    listening: { speed: 1.50, amp: 0.23, c1: [52, 211, 153],  c2: [148, 163, 184], c3: [209, 250, 229] },
    thinking:  { speed: 2.45, amp: 0.25, c1: [124, 140, 255], c2: [212, 212, 220], c3: [224, 231, 255] },
    executing: { speed: 2.15, amp: 0.22, c1: [96, 165, 250],  c2: [228, 228, 235], c3: [186, 230, 253] },
    takeover:  { speed: 1.35, amp: 0.20, c1: [251, 191, 36],  c2: [245, 158, 11],  c3: [254, 243, 199] },
  };

  const lerp = (a, b, t) => a + (b - a) * t;
  const lerpRgb = (a, b, t) => [lerp(a[0], b[0], t), lerp(a[1], b[1], t), lerp(a[2], b[2], t)];
  const rgba = (c, a) => `rgba(${c[0] | 0},${c[1] | 0},${c[2] | 0},${a})`;

  let rafId = 0;
  let lastTs = 0;
  let phase = 0;
  let active = false;
  let currentState = "idle";
  let cur = { speed: 0.72, amp: 0.14, c1: [...AURORA_STATES.idle.c1], c2: [...AURORA_STATES.idle.c2], c3: [...AURORA_STATES.idle.c3] };
  let bgCanvas = null;
  const orbs = new Set();

  function styleName() { return document.documentElement.dataset.style || ""; }
  function isDynamicStyle() { const s = styleName(); return s === "aurora" || s === "obsidian-aurora"; }
  function statePalette() { return styleName() === "obsidian-aurora" ? OBSIDIAN_STATES : AURORA_STATES; }
  function isLight() { return document.documentElement.dataset.theme === "light"; }
  function motionMode() { return document.documentElement.dataset.motion || "full"; }

  function detectState() {
    if (document.querySelector('.voice-in[aria-pressed="true"]')) return "listening";
    if (document.querySelector('.state-pill[data-pill="mine"], .screen.held')) return "takeover";
    if (document.querySelector('.st.working, .comp-dot.run, .tcard[data-status="running"], .tcard[data-status="working"], .state-pill[data-pill="working"], .state-pill[data-pill="waking"]')) return "executing";
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

  function ensureBgCanvas() {
    if (bgCanvas && bgCanvas.isConnected) return;
    bgCanvas = document.createElement("canvas");
    bgCanvas.className = "aurora-bg-canvas";
    bgCanvas.setAttribute("aria-hidden", "true");
    document.body.insertBefore(bgCanvas, document.body.firstChild);
    resizeBgCanvas();
  }

  function resizeBgCanvas() {
    if (!bgCanvas) return;
    const scale = 0.25; // Low-res blurred shader buffer for silky 60fps with minimal GPU cost
    bgCanvas.width = Math.max(160, Math.round(window.innerWidth * scale));
    bgCanvas.height = Math.max(120, Math.round(window.innerHeight * scale));
  }

  function enhanceDynamicDom() {
    if (!active) return;
    ensureBgCanvas();

    // 1. Left sidebar brand orb
    document.querySelectorAll("nav.side .brand .logo").forEach((logo) => {
      if (!logo.querySelector(".aurora-orb-nav")) {
        const c = makeOrb(24, "aurora-orb-nav");
        logo.appendChild(c);
        orbs.add(c);
      }
    });

    // 2. Right sidebar companion avatar orb
    document.querySelectorAll(".companion-avatar").forEach((av) => {
      if (!av.querySelector(".aurora-orb-side")) {
        const c = makeOrb(30, "aurora-orb-side");
        av.appendChild(c);
        orbs.add(c);
      }
    });

    // 3. Discover / New Chat hero presence orb with voice ripples
    document.querySelectorAll(".hello-head").forEach((head) => {
      if (!head.querySelector(".aurora-orb-hero")) {
        const wrap = document.createElement("div");
        wrap.className = "aurora-hero-presence";
        wrap.innerHTML = '<span class="aurora-ripple" aria-hidden="true"></span><span class="aurora-ripple r2" aria-hidden="true"></span>';
        const c = makeOrb(88, "aurora-orb-hero");
        wrap.appendChild(c);
        head.insertBefore(wrap, head.firstChild);
        orbs.add(c);
      }
    });

    // 4. Mark active tool/work containers for rotating conic-gradient execution beams
    document.querySelectorAll(".reqgroup.chatcard, .tcard, .companion-active-card").forEach((el) => {
      const running =
        el.matches(".companion-active-card") ||
        el.matches('.tcard[data-status="running"], .tcard[data-status="working"], .tcard[data-status="queued"]') ||
        !!el.querySelector('.st.working, .comp-dot.run, [data-status="running"], [data-status="working"]');
      el.classList.toggle("aurora-executing", running);
    });

    // 5. Masked entrance on newly rendered answer blocks
    document.querySelectorAll('.entry[data-kind="answer"] .answer-body > *').forEach((node) => {
      if (!node.dataset.auroraSeen) {
        node.dataset.auroraSeen = "1";
        node.classList.add("aurora-stream-in");
      }
    });

    // Prune detached orbs
    for (const c of orbs) if (!c.isConnected) orbs.delete(c);

    const next = detectState();
    if (next !== currentState) {
      currentState = next;
      document.documentElement.dataset.auroraState = next;
    }
  }

  function drawAmbientBg() {
    if (!bgCanvas || !bgCanvas.isConnected) return;
    const ctx = bgCanvas.getContext("2d");
    if (!ctx) return;
    const w = bgCanvas.width;
    const h = bgCanvas.height;
    ctx.clearRect(0, 0, w, h);

    const alphaBoost = isLight() ? 0.22 : 0.34;
    const blobs = [
      { x: 0.20 + Math.sin(phase * 0.38) * 0.12, y: 0.16 + Math.cos(phase * 0.31) * 0.09, r: 0.52, col: cur.c1, a: alphaBoost },
      { x: 0.82 + Math.cos(phase * 0.33) * 0.11, y: 0.22 + Math.sin(phase * 0.42) * 0.12, r: 0.48, col: cur.c2, a: alphaBoost * 0.9 },
      { x: 0.50 + Math.sin(phase * 0.27 + 1.8) * 0.16, y: 0.88 + Math.cos(phase * 0.29) * 0.08, r: 0.50, col: cur.c3, a: alphaBoost * 0.65 },
    ];
    for (const b of blobs) {
      const gx = b.x * w;
      const gy = b.y * h;
      const gr = Math.max(w, h) * b.r;
      const grad = ctx.createRadialGradient(gx, gy, 0, gx, gy, gr);
      grad.addColorStop(0, rgba(b.col, b.a));
      grad.addColorStop(1, "rgba(0,0,0,0)");
      ctx.fillStyle = grad;
      ctx.fillRect(0, 0, w, h);
    }
  }

  function drawOrb(canvas) {
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    const size = Number(canvas.dataset.size || 24);
    const cx = size / 2;
    const cy = size / 2;
    const breath = 1 + Math.sin(phase * 1.45) * (cur.amp * 0.26);
    const baseR = size * 0.35 * breath;

    ctx.clearRect(0, 0, size, size);
    const glow = ctx.createRadialGradient(cx, cy, baseR * 0.25, cx, cy, size * 0.5);
    glow.addColorStop(0, rgba(cur.c1, 0.46));
    glow.addColorStop(0.6, rgba(cur.c2, 0.18));
    glow.addColorStop(1, "rgba(0,0,0,0)");
    ctx.fillStyle = glow;
    ctx.fillRect(0, 0, size, size);

    ctx.save();
    ctx.beginPath();
    const steps = size > 48 ? 60 : 28;
    for (let i = 0; i <= steps; i++) {
      const a = (i / steps) * Math.PI * 2;
      const wobble = (Math.sin(a * 3 + phase * 1.7) * 0.08 + Math.cos(a * 5 - phase * 2.2) * 0.05) * baseR * cur.amp * 4;
      const r = baseR + wobble;
      const x = cx + Math.cos(a) * r;
      const y = cy + Math.sin(a) * r;
      if (i === 0) ctx.moveTo(x, y); else ctx.lineTo(x, y);
    }
    ctx.closePath();
    ctx.clip();

    const core = ctx.createRadialGradient(cx - baseR * 0.28, cy - baseR * 0.32, baseR * 0.1, cx, cy, baseR * 1.25);
    core.addColorStop(0, rgba(cur.c3, 0.96));
    core.addColorStop(0.45, rgba(cur.c1, 0.88));
    core.addColorStop(0.85, rgba(cur.c2, 0.68));
    core.addColorStop(1, "rgba(8,12,24,0.92)");
    ctx.fillStyle = core;
    ctx.fillRect(0, 0, size, size);

    ctx.globalCompositeOperation = "lighter";
    const ox = cx + Math.cos(phase * 1.35) * baseR * 0.36;
    const oy = cy + Math.sin(phase * 1.55) * baseR * 0.36;
    const rib = ctx.createRadialGradient(ox, oy, 1, ox, oy, baseR * 0.8);
    rib.addColorStop(0, rgba(cur.c3, 0.62));
    rib.addColorStop(1, "rgba(0,0,0,0)");
    ctx.fillStyle = rib;
    ctx.fillRect(0, 0, size, size);
    ctx.restore();

    ctx.beginPath();
    ctx.arc(cx, cy, Math.max(1, baseR - 0.8), -Math.PI * 0.85, -Math.PI * 0.15);
    ctx.strokeStyle = "rgba(255,255,255,0.45)";
    ctx.lineWidth = size > 48 ? 1.5 : 0.95;
    ctx.lineCap = "round";
    ctx.stroke();
  }

  function tick(now) {
    if (!active) { rafId = 0; return; }
    const dt = Math.min((now - lastTs) / 1000, 0.05);
    lastTs = now;
    const pal = statePalette();
    const target = pal[currentState] || pal.idle;
    const blend = 1 - Math.exp(-dt * 4.5);
    cur.speed = lerp(cur.speed, target.speed, blend);
    cur.amp = lerp(cur.amp, target.amp, blend);
    cur.c1 = lerpRgb(cur.c1, target.c1, blend);
    cur.c2 = lerpRgb(cur.c2, target.c2, blend);
    cur.c3 = lerpRgb(cur.c3, target.c3, blend);

    if (motionMode() !== "off") phase += dt * cur.speed;
    drawAmbientBg();
    for (const c of orbs) if (c.isConnected) drawOrb(c);

    if (motionMode() === "off") { rafId = 0; return; }
    rafId = requestAnimationFrame(tick);
  }

  // Tactile pointer spotlight across floating glass surfaces
  let pointerQueued = false;
  let lastEv = null;
  function onPointerMove(e) {
    if (!active || motionMode() !== "full") return;
    lastEv = e;
    if (pointerQueued) return;
    pointerQueued = true;
    requestAnimationFrame(() => {
      pointerQueued = false;
      const ev = lastEv;
      if (!ev || !(ev.target instanceof Element)) return;
      const targetEl = ev.target.closest(".hello-card, .explore-card, .card, .say, .companion-active-card, .gbox");
      if (!targetEl) return;
      const rect = targetEl.getBoundingClientRect();
      targetEl.style.setProperty("--aurora-mx", `${((ev.clientX - rect.left) / Math.max(1, rect.width)) * 100}%`);
      targetEl.style.setProperty("--aurora-my", `${((ev.clientY - rect.top) / Math.max(1, rect.height)) * 100}%`);
    });
  }

  function teardown() {
    active = false;
    if (rafId) { cancelAnimationFrame(rafId); rafId = 0; }
    delete document.documentElement.dataset.auroraState;
    if (bgCanvas) { bgCanvas.remove(); bgCanvas = null; }
    document.querySelectorAll(".aurora-hero-presence, .aurora-orb").forEach((el) => el.remove());
    document.querySelectorAll(".aurora-executing").forEach((el) => el.classList.remove("aurora-executing"));
    orbs.clear();
  }

  function reconcile() {
    if (isDynamicStyle()) {
      const wasActive = active;
      active = true;
      document.documentElement.dataset.auroraState = currentState;
      enhanceDynamicDom();
      if (!wasActive || (!rafId && motionMode() !== "off")) {
        lastTs = performance.now();
        if (motionMode() === "off") {
          drawAmbientBg();
          for (const c of orbs) if (c.isConnected) drawOrb(c);
        } else {
          rafId = requestAnimationFrame(tick);
        }
      }
    } else if (active) {
      teardown();
    }
  }

  const htmlObs = new MutationObserver(reconcile);
  htmlObs.observe(document.documentElement, { attributes: true, attributeFilter: ["data-style", "data-motion", "data-theme"] });

  let domTimer = 0;
  const bodyObs = new MutationObserver(() => {
    if (!active || domTimer) return;
    domTimer = setTimeout(() => { domTimer = 0; enhanceDynamicDom(); }, 60);
  });

  function init() {
    bodyObs.observe(document.body, { childList: true, subtree: true, attributes: true, attributeFilter: ["class", "data-pill", "data-status", "aria-pressed"] });
    document.addEventListener("pointermove", onPointerMove, { passive: true });
    document.addEventListener("focusin", () => { if (active) enhanceDynamicDom(); }, true);
    document.addEventListener("focusout", () => { if (active) enhanceDynamicDom(); }, true);
    window.addEventListener("resize", () => { if (active) resizeBgCanvas(); }, { passive: true });
    reconcile();
  }

  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", init);
  else init();
})();
