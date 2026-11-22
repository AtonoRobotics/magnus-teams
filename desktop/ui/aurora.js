// Aurora & Obsidian 2 Aurora dynamic theme engine:
// 1. Soft ambient aurora shader canvas + selectable 3D motion background
//    (Particles, Ribbons, Constellation, or Off) behind floating bento islands
// 2. Living liquid-glass presence orbs (sidebar, companion, mobile bar, hero)
// 3. Real-time state engine (idle, listening, thinking, executing, takeover, needs)
// 4. Choreographed view, tab, bento-card, and masked-stream entrances
// 5. Tactile pointer/touch spotlights and gentle 3D depth parallax
// Strictly gated on html[data-style="aurora"|"obsidian-aurora"]; zero cost elsewhere.
"use strict";

(function () {
  const AURORA_STATES = {
    idle:      { speed: 0.92, amp: 0.16, c1: [79, 157, 255],  c2: [56, 189, 248],  c3: [167, 139, 250] },
    ready:     { speed: 0.92, amp: 0.16, c1: [79, 157, 255],  c2: [56, 189, 248],  c3: [167, 139, 250] },
    listening: { speed: 1.65, amp: 0.26, c1: [52, 211, 153],  c2: [56, 189, 248],  c3: [167, 243, 208] },
    thinking:  { speed: 2.65, amp: 0.28, c1: [155, 109, 255], c2: [56, 189, 248],  c3: [216, 180, 254] },
    planning:  { speed: 1.80, amp: 0.20, c1: [129, 140, 248], c2: [94, 234, 212],  c3: [199, 210, 254] },
    executing: { speed: 2.30, amp: 0.24, c1: [56, 189, 248],  c2: [129, 140, 248], c3: [94, 234, 212] },
    working:   { speed: 2.30, amp: 0.24, c1: [56, 189, 248],  c2: [129, 140, 248], c3: [94, 234, 212] },
    takeover:  { speed: 1.45, amp: 0.21, c1: [251, 191, 36],  c2: [245, 158, 11],  c3: [254, 243, 199] },
    control:   { speed: 1.45, amp: 0.21, c1: [251, 191, 36],  c2: [245, 158, 11],  c3: [254, 243, 199] },
    needs:     { speed: 1.10, amp: 0.18, c1: [251, 191, 36],  c2: [79, 157, 255],  c3: [253, 230, 138] },
    waiting:   { speed: 1.10, amp: 0.18, c1: [251, 191, 36],  c2: [79, 157, 255],  c3: [253, 230, 138] },
    done:      { speed: 0.72, amp: 0.12, c1: [52, 211, 153],  c2: [16, 185, 129],  c3: [209, 250, 229] },
    failed:    { speed: 0.65, amp: 0.10, c1: [251, 113, 133], c2: [225, 29, 72],   c3: [254, 205, 211] },
  };
  const OBSIDIAN_STATES = {
    idle:      { speed: 0.86, amp: 0.14, c1: [220, 224, 234], c2: [120, 136, 162], c3: [248, 250, 252] },
    ready:     { speed: 0.86, amp: 0.14, c1: [220, 224, 234], c2: [120, 136, 162], c3: [248, 250, 252] },
    listening: { speed: 1.58, amp: 0.24, c1: [52, 211, 153],  c2: [148, 163, 184], c3: [209, 250, 229] },
    thinking:  { speed: 2.55, amp: 0.26, c1: [124, 140, 255], c2: [216, 220, 230], c3: [224, 231, 255] },
    planning:  { speed: 1.75, amp: 0.19, c1: [124, 140, 255], c2: [148, 163, 184], c3: [224, 231, 255] },
    executing: { speed: 2.25, amp: 0.23, c1: [96, 165, 250],  c2: [228, 230, 238], c3: [186, 230, 253] },
    working:   { speed: 2.25, amp: 0.23, c1: [96, 165, 250],  c2: [228, 230, 238], c3: [186, 230, 253] },
    takeover:  { speed: 1.40, amp: 0.20, c1: [251, 191, 36],  c2: [245, 158, 11],  c3: [254, 243, 199] },
    control:   { speed: 1.40, amp: 0.20, c1: [251, 191, 36],  c2: [245, 158, 11],  c3: [254, 243, 199] },
    needs:     { speed: 1.05, amp: 0.17, c1: [251, 191, 36],  c2: [148, 163, 184], c3: [253, 230, 138] },
    waiting:   { speed: 1.05, amp: 0.17, c1: [251, 191, 36],  c2: [148, 163, 184], c3: [253, 230, 138] },
    done:      { speed: 0.70, amp: 0.11, c1: [52, 211, 153],  c2: [148, 163, 184], c3: [209, 250, 229] },
    failed:    { speed: 0.62, amp: 0.10, c1: [251, 113, 133], c2: [148, 163, 184], c3: [254, 205, 211] },
  };

  const PARTICLE_COUNT_DESKTOP = 48;
  const PARTICLE_COUNT_MOBILE = 30;
  const MAX_FIELD_DPR = 1.5;
  const STAGGER_STEP_MS = 52;
  const MAX_STAGGER_ITEMS = 8;

  const lerp = (a, b, t) => a + (b - a) * t;
  const lerpRgb = (a, b, t) => [lerp(a[0], b[0], t), lerp(a[1], b[1], t), lerp(a[2], b[2], t)];
  const rgba = (c, a) => `rgba(${c[0] | 0},${c[1] | 0},${c[2] | 0},${a.toFixed(3)})`;

  let rafId = 0;
  let unsubFrame = null;
  let lastTs = 0;
  let phase = 0;
  let active = false;
  let currentState = "ready";
  let cur = { speed: 0.92, amp: 0.16, c1: [...AURORA_STATES.idle.c1], c2: [...AURORA_STATES.idle.c2], c3: [...AURORA_STATES.idle.c3] };
  let bgCanvas = null;
  let fieldCanvas = null;
  let fieldW = 0;
  let fieldH = 0;
  let particles = [];
  let parTargetX = 0;
  let parTargetY = 0;
  let parX = 0;
  let parY = 0;

  function styleName() { return document.documentElement.dataset.style || ""; }
  function isDynamicStyle() { const s = styleName(); return s === "aurora" || s === "obsidian-aurora"; }
  function statePalette() { return styleName() === "obsidian-aurora" ? OBSIDIAN_STATES : AURORA_STATES; }
  function isLight() { return document.documentElement.dataset.mode === "light"; }
  function motionMode() { return document.documentElement.dataset.motion || "full"; }
  function bgStyle() { return document.documentElement.dataset.background || "particles"; }

  function detectState() {
    if (typeof AuroraCore !== "undefined") return AuroraCore.refreshState();
    if (document.querySelector('.voice-in[aria-pressed="true"], .room-video-listening')) return "listening";
    if (document.querySelector('.state-pill[data-pill="mine"], .screen.held')) return "takeover";
    if (document.querySelector('.st.need, .stage-tile[data-tone="needs"], .stage-decision, .detail-body[data-panel="needs"] .ask-answers, .aurora-reason[data-phase="needs"]')) return "needs";
    if (document.querySelector('.st.working, .comp-dot.run, .tcard[data-status="running"], .tcard[data-status="working"], .state-pill[data-pill="working"], .state-pill[data-pill="waking"], .aurora-reason[data-phase="working"]')) return "executing";
    // An answer being thought out or written (aurora-reasoning.js).
    if (document.querySelector('.aurora-reason:is([data-phase="thinking"], [data-phase="writing"])')) return "thinking";
    return "idle";
  }

  function spawnParticle(randomZ) {
    const tint = Math.random();
    return {
      x: (Math.random() * 2 - 1) * 1.18,
      y: (Math.random() * 2 - 1) * 1.18,
      z: randomZ ? 0.16 + Math.random() * 0.84 : 0.96 + Math.random() * 0.08,
      vx: (Math.random() - 0.5) * 0.018,
      vy: (Math.random() - 0.5) * 0.014 - 0.004,
      vz: 0.022 + Math.random() * 0.032,
      size: 0.9 + Math.random() * 1.7,
      twinkle: Math.random() * Math.PI * 2,
      tint: tint < 0.45 ? 0 : tint < 0.8 ? 1 : 2,
    };
  }

  function ensureParticles() {
    const desired = window.innerWidth < 760 ? PARTICLE_COUNT_MOBILE : PARTICLE_COUNT_DESKTOP;
    if (particles.length === desired) return;
    particles = [];
    for (let i = 0; i < desired; i++) particles.push(spawnParticle(true));
  }

  function ensureCanvases() {
    if (!bgCanvas || !bgCanvas.isConnected) {
      bgCanvas = document.createElement("canvas");
      bgCanvas.className = "aurora-bg-canvas";
      bgCanvas.setAttribute("aria-hidden", "true");
      document.body.insertBefore(bgCanvas, document.body.firstChild);
    }
    const wantField = bgStyle() !== "off" && motionMode() !== "off";
    if (wantField) {
      if (!fieldCanvas || !fieldCanvas.isConnected) {
        fieldCanvas = document.createElement("canvas");
        fieldCanvas.className = "aurora-field-canvas";
        fieldCanvas.setAttribute("aria-hidden", "true");
        if (bgCanvas.nextSibling) document.body.insertBefore(fieldCanvas, bgCanvas.nextSibling);
        else document.body.appendChild(fieldCanvas);
      }
      ensureParticles();
    } else if (fieldCanvas) {
      fieldCanvas.remove();
      fieldCanvas = null;
    }
    resizeCanvases();
  }

  function resizeCanvases() {
    if (bgCanvas) {
      const scale = 0.25;
      const bw = Math.max(160, Math.round(window.innerWidth * scale));
      const bh = Math.max(120, Math.round(window.innerHeight * scale));
      if (bgCanvas.width !== bw || bgCanvas.height !== bh) {
        bgCanvas.width = bw;
        bgCanvas.height = bh;
      }
    }
    if (fieldCanvas) {
      const dpr = Math.min(window.devicePixelRatio || 1, MAX_FIELD_DPR);
      const cw = Math.max(1, window.innerWidth);
      const ch = Math.max(1, window.innerHeight);
      const pw = Math.round(cw * dpr);
      const ph = Math.round(ch * dpr);
      if (fieldCanvas.width !== pw || fieldCanvas.height !== ph) {
        fieldCanvas.width = pw;
        fieldCanvas.height = ph;
        fieldW = cw;
        fieldH = ch;
        const ctx = fieldCanvas.getContext("2d");
        if (ctx) ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      }
      ensureParticles();
    }
  }

  // Entrances play once per thing the person sees arrive, never per repaint.
  // The app redraws a region by replacing its markup (app.js paint), so every
  // redraw makes new nodes: an entrance keyed on the node replayed the whole
  // view, panel or conversation on each live update, which read as the window
  // flashing. Each entrance is keyed on what is shown instead: the route, the
  // panel, and an answer's own blocks.
  let viewKey = null;
  let panelKey = null;
  let heroKey = null;
  const answerSeen = new Set();
  const ANSWER_SEEN_MAX = 5000;
  const CARD_SELECTOR = ".hello-card, .explore-card, .hello-caps > *, .teams-page .team-list a, .tiles > div";

  function stagger(root) {
    root.querySelectorAll(CARD_SELECTOR).forEach((el, idx) => {
      el.style.setProperty("--aurora-delay", `${Math.min(idx, MAX_STAGGER_ITEMS) * STAGGER_STEP_MS}ms`);
      el.classList.add("aurora-stagger");
    });
  }

  function markEntrances() {
    if (!active) return;
    const route = location.hash || "#/";
    // A view arrives when the route changes, not when the route repaints.
    const main = document.querySelector("#main > *:not(section.conv)");
    if (main) {
      const key = `${route}|${main.id || (main.getAttribute("class") || "").split(/\s+/)[0]}`;
      if (key !== viewKey) {
        viewKey = key;
        main.classList.add("aurora-view-in");
        stagger(main);
      }
    }
    // The new-conversation hero arrives once per visit to its route.
    const hero = document.querySelector(".hello-head");
    if (hero && heroKey !== route) {
      heroKey = route;
      hero.classList.add("aurora-view-in");
      const thread = hero.closest(".thread");
      if (thread) stagger(thread);
    } else if (!hero) {
      heroKey = null;
    }
    // A panel arrives when what it shows changes (side-panel.js data-panel).
    const body = document.querySelector("aside.detail:not([hidden]) .detail-body");
    const pk = body ? body.dataset.panel || "" : null;
    if (body && pk !== panelKey) {
      for (const child of body.children) child.classList.add("aurora-view-in");
    }
    panelKey = pk;
    // An answer's blocks enter as they stream in; blocks already shown, and
    // every block of an answer met for the first time, are only recorded.
    if (answerSeen.size > ANSWER_SEEN_MAX) answerSeen.clear();
    document.querySelectorAll('.entry[data-kind="answer"][data-entry]').forEach((entry) => {
      const id = entry.dataset.entry;
      if (!id) return;
      const known = answerSeen.has(id) || entry.classList.contains("enter");
      answerSeen.add(id);
      let wordCursor = 0;
      entry.querySelectorAll(".answer-body > *").forEach((node, idx) => {
        const k = `${id}:${idx}`;
        if (answerSeen.has(k)) return;
        answerSeen.add(k);
        if (known) {
          node.classList.add("aurora-stream-in");
          if (globalThis.AuroraStage?.stream) wordCursor = globalThis.AuroraStage.stream(node, wordCursor);
        }
      });
    });
  }

  function enhanceDynamicDom() {
    if (!active) return;
    ensureCanvases();

    // Mark active tool/work containers for rotating conic-gradient execution beams
    // (skip borderless inline task lists inside the thread; their bento box carries the beam).
    document.querySelectorAll(".tcard:not(.reqgroup.chatcard .tcard), .companion-active-card").forEach((el) => {
      const running =
        el.matches(".companion-active-card") ||
        el.matches('.tcard[data-status="running"], .tcard[data-status="working"], .tcard[data-status="queued"]') ||
        !!el.querySelector('.st.working, .comp-dot.run, [data-status="running"], [data-status="working"]');
      el.classList.toggle("aurora-executing", running);
    });

    // Entrances, keyed on what is shown (see markEntrances)
    markEntrances();
    watchDetail();
    noteDetailRect();

    const next = detectState();
    if (next !== currentState) {
      currentState = next;
      if (typeof AuroraCore === "undefined") document.documentElement.dataset.auroraState = next;
    }
  }

  function drawAmbientBg() {
    if (!bgCanvas || !bgCanvas.isConnected) return;
    const ctx = bgCanvas.getContext("2d");
    if (!ctx) return;
    const w = bgCanvas.width;
    const h = bgCanvas.height;
    ctx.clearRect(0, 0, w, h);

    const isObs = styleName() === "obsidian-aurora";
    const alphaBoost = isLight() ? (isObs ? 0.24 : 0.34) : (isObs ? 0.40 : 0.56);
    const blobs = [
      { x: 0.22 + Math.sin(phase * 0.52) * 0.17, y: 0.16 + Math.cos(phase * 0.44) * 0.13, r: 0.58, col: cur.c1, a: alphaBoost },
      { x: 0.80 + Math.cos(phase * 0.46) * 0.16, y: 0.24 + Math.sin(phase * 0.54) * 0.15, r: 0.54, col: cur.c2, a: alphaBoost * 0.92 },
      { x: 0.50 + Math.sin(phase * 0.38 + 1.8) * 0.20, y: 0.84 + Math.cos(phase * 0.40) * 0.13, r: 0.56, col: cur.c3, a: alphaBoost * 0.76 },
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

  function fieldColor(tintIdx, light, isObs) {
    if (light) {
      if (isObs) return tintIdx === 0 ? [55, 65, 81] : tintIdx === 1 ? [71, 85, 105] : [59, 91, 219];
      return tintIdx === 0 ? [37, 99, 235] : tintIdx === 1 ? [14, 116, 144] : [109, 40, 217];
    }
    return tintIdx === 0 ? cur.c1 : tintIdx === 1 ? cur.c2 : cur.c3;
  }

  function drawParticlesField(ctx, dt, animate, withLines) {
    const w = fieldW;
    const h = fieldH;
    const cx = w * 0.5;
    const cy = h * 0.5;
    const scale = Math.min(w, h) * 0.82;
    const light = isLight();
    const isObs = styleName() === "obsidian-aurora";
    const speedMul = 0.65 + cur.speed * 0.55;
    const projected = [];

    for (let i = 0; i < particles.length; i++) {
      const p = particles[i];
      if (animate) {
        p.x += p.vx * dt * speedMul;
        p.y += p.vy * dt * speedMul;
        p.z -= p.vz * dt * speedMul;
        if (p.z < 0.14 || Math.abs(p.x) > 1.45 || Math.abs(p.y) > 1.45) {
          particles[i] = spawnParticle(false);
          continue;
        }
      }
      const depth = 1 - p.z;
      const persp = 0.64 / p.z;
      const sx = cx + (p.x + parX * (0.35 + depth * 0.9)) * persp * scale;
      const sy = cy + (p.y + parY * (0.35 + depth * 0.9)) * persp * scale;
      if (sx < -40 || sx > w + 40 || sy < -40 || sy > h + 40) continue;

      const fadeFar = Math.min(1, (1.02 - p.z) * 4.5);
      const fadeNear = Math.min(1, (p.z - 0.14) * 5.0);
      const twinkle = 0.78 + Math.sin(phase * 1.4 + p.twinkle) * 0.22;
      const baseAlpha = light ? (isObs ? 0.36 : 0.42) : (isObs ? 0.62 : 0.74);
      const alpha = Math.max(0, Math.min(0.92, baseAlpha * (0.28 + depth * 0.72) * fadeFar * fadeNear * twinkle));
      const r = Math.max(0.8, p.size * (0.55 + depth * 1.65));
      const col = fieldColor(p.tint, light, isObs);
      projected.push({ sx, sy, z: p.z, r, alpha, col });
    }

    if (withLines) {
      const maxDist = Math.min(w, h) * 0.22;
      const maxDistSq = maxDist * maxDist;
      const lineBase = light ? (isObs ? 0.16 : 0.20) : (isObs ? 0.22 : 0.28);
      ctx.lineWidth = 0.85;
      for (let i = 0; i < projected.length; i++) {
        const a = projected[i];
        if (a.z > 0.80) continue;
        for (let j = i + 1; j < projected.length; j++) {
          const b = projected[j];
          if (b.z > 0.80 || Math.abs(a.z - b.z) > 0.28) continue;
          const dx = a.sx - b.sx;
          const dy = a.sy - b.sy;
          const d2 = dx * dx + dy * dy;
          if (d2 > maxDistSq) continue;
          const k = 1 - Math.sqrt(d2) / maxDist;
          ctx.strokeStyle = rgba(a.col, lineBase * k * Math.min(a.alpha, b.alpha));
          ctx.beginPath();
          ctx.moveTo(a.sx, a.sy);
          ctx.lineTo(b.sx, b.sy);
          ctx.stroke();
        }
      }
    }

    for (const pt of projected) {
      if (pt.r > 1.7) {
        const haloR = pt.r * 3.6;
        const halo = ctx.createRadialGradient(pt.sx, pt.sy, 0, pt.sx, pt.sy, haloR);
        halo.addColorStop(0, rgba(pt.col, pt.alpha * 0.42));
        halo.addColorStop(1, "rgba(0,0,0,0)");
        ctx.fillStyle = halo;
        ctx.beginPath();
        ctx.arc(pt.sx, pt.sy, haloR, 0, Math.PI * 2);
        ctx.fill();
      }
      ctx.fillStyle = rgba(pt.col, pt.alpha);
      ctx.beginPath();
      ctx.arc(pt.sx, pt.sy, pt.r, 0, Math.PI * 2);
      ctx.fill();
    }
  }

  function drawRibbonsField(ctx) {
    const w = fieldW;
    const h = fieldH;
    const light = isLight();
    const isObs = styleName() === "obsidian-aurora";
    const ribbons = [
      { y: 0.24, amp: 0.085, freq: 1.85, speed: 0.62, width: 2.2, col: fieldColor(0, light, isObs), a: light ? 0.26 : (isObs ? 0.34 : 0.46) },
      { y: 0.52, amp: 0.105, freq: 1.45, speed: -0.48, width: 2.6, col: fieldColor(1, light, isObs), a: light ? 0.24 : (isObs ? 0.30 : 0.42) },
      { y: 0.78, amp: 0.075, freq: 2.20, speed: 0.52, width: 1.8, col: fieldColor(2, light, isObs), a: light ? 0.20 : (isObs ? 0.26 : 0.36) },
    ];
    const segs = 44;
    for (let r = 0; r < ribbons.length; r++) {
      const rb = ribbons[r];
      const baseY = h * rb.y + parY * h * (0.04 + r * 0.02);
      const ampPx = h * rb.amp;
      const pts = [];
      for (let i = 0; i <= segs; i++) {
        const u = i / segs;
        const x = -24 + u * (w + 48);
        const wave =
          Math.sin(u * Math.PI * 2 * rb.freq + phase * rb.speed + r * 1.4) * 0.68 +
          Math.cos(u * Math.PI * 3.1 - phase * rb.speed * 0.72 + r * 2.1) * 0.32;
        pts.push([x, baseY + wave * ampPx]);
      }
      const grad = ctx.createLinearGradient(0, baseY - ampPx, w, baseY + ampPx);
      grad.addColorStop(0, "rgba(0,0,0,0)");
      grad.addColorStop(0.18, rgba(rb.col, rb.a));
      grad.addColorStop(0.50, rgba(rb.col, Math.min(0.85, rb.a * 1.35)));
      grad.addColorStop(0.82, rgba(rb.col, rb.a));
      grad.addColorStop(1, "rgba(0,0,0,0)");

      // Soft luminous outer silk pass + crisp luminous core line
      for (const pass of [
        { width: rb.width * 10, alpha: 0.20 },
        { width: rb.width * 3.5, alpha: 0.45 },
        { width: rb.width, alpha: 1.0 },
      ]) {
        ctx.save();
        ctx.globalAlpha = pass.alpha;
        ctx.strokeStyle = grad;
        ctx.lineWidth = pass.width;
        ctx.lineCap = "round";
        ctx.beginPath();
        for (let i = 0; i < pts.length; i++) {
          if (i === 0) ctx.moveTo(pts[i][0], pts[i][1]);
          else ctx.lineTo(pts[i][0], pts[i][1]);
        }
        ctx.stroke();
        ctx.restore();
      }
    }
  }

  function drawField(dt, animate) {
    if (!fieldCanvas || !fieldCanvas.isConnected) return;
    const ctx = fieldCanvas.getContext("2d");
    if (!ctx) return;
    ctx.clearRect(0, 0, fieldW, fieldH);
    const kind = bgStyle();
    if (kind === "off") return;
    if (kind === "ribbons") drawRibbonsField(ctx);
    else drawParticlesField(ctx, dt, animate, kind === "constellation");
  }

  function renderFrame(dt) {
    const pal = statePalette();
    const target = pal[currentState] || pal.idle;
    const blend = 1 - Math.exp(-dt * 4.5);
    cur.speed = lerp(cur.speed, target.speed, blend);
    cur.amp = lerp(cur.amp, target.amp, blend);
    cur.c1 = lerpRgb(cur.c1, target.c1, blend);
    cur.c2 = lerpRgb(cur.c2, target.c2, blend);
    cur.c3 = lerpRgb(cur.c3, target.c3, blend);

    const pBlend = 1 - Math.exp(-dt * 3.5);
    parX = lerp(parX, parTargetX, pBlend);
    parY = lerp(parY, parTargetY, pBlend);

    const mm = motionMode();
    if (mm === "full") phase += dt * cur.speed;
    drawAmbientBg();
    drawField(dt, mm === "full");
  }

  function tick(now) {
    if (!active || document.hidden) { rafId = 0; return; }
    const dt = Math.min((now - lastTs) / 1000, 0.05);
    lastTs = now;
    renderFrame(dt);
    if (motionMode() !== "full") { rafId = 0; return; }
    rafId = requestAnimationFrame(tick);
  }

  // Tactile pointer/touch spotlight + gentle 3D background parallax
  let pointerQueued = false;
  let lastEv = null;
  let lastSpotEl = null;
  function onPointerMove(e) {
    if (!active || motionMode() !== "full") return;
    lastEv = e;
    if (pointerQueued) return;
    pointerQueued = true;
    requestAnimationFrame(() => {
      pointerQueued = false;
      const ev = lastEv;
      if (!ev) return;
      parTargetX = ((ev.clientX / Math.max(1, window.innerWidth)) - 0.5) * 0.09;
      parTargetY = ((ev.clientY / Math.max(1, window.innerHeight)) - 0.5) * 0.09;
      const targetEl = ev.target instanceof Element ? ev.target.closest(".hello-card, .explore-card, .card, .say, .stage-tile, .companion-active-card, .gbox") : null;
      if (lastSpotEl && lastSpotEl !== targetEl) {
        lastSpotEl.style.removeProperty("--aurora-mx");
        lastSpotEl.style.removeProperty("--aurora-my");
      }
      lastSpotEl = targetEl;
      if (!targetEl) return;
      const rect = targetEl.getBoundingClientRect();
      targetEl.style.setProperty("--aurora-mx", `${((ev.clientX - rect.left) / Math.max(1, rect.width)) * 100}%`);
      targetEl.style.setProperty("--aurora-my", `${((ev.clientY - rect.top) / Math.max(1, rect.height)) * 100}%`);
    });
  }

  function teardown() {
    active = false;
    if (rafId) { cancelAnimationFrame(rafId); rafId = 0; }
    if (unsubFrame) { unsubFrame(); unsubFrame = null; }
    if (typeof AuroraCore === "undefined") delete document.documentElement.dataset.auroraState;
    if (bgCanvas) { bgCanvas.remove(); bgCanvas = null; }
    if (fieldCanvas) { fieldCanvas.remove(); fieldCanvas = null; }
    document.querySelectorAll(".aurora-executing").forEach((el) => el.classList.remove("aurora-executing"));
    document.querySelectorAll(".aurora-detail-ghost").forEach((el) => el.remove());
    if (detailObs) { detailObs.disconnect(); detailObs = null; }
    detailEl = null;
    detailRect = null;
  }

  function startLoop() {
    if (!active || document.hidden) return;
    if (typeof AuroraCore !== "undefined") {
      if (!unsubFrame) unsubFrame = AuroraCore.onFrame((_now, dt) => { if (active) renderFrame(dt); });
      AuroraCore.wake();
      return;
    }
    lastTs = performance.now();
    if (motionMode() === "full") {
      if (!rafId) rafId = requestAnimationFrame(tick);
    } else {
      if (rafId) { cancelAnimationFrame(rafId); rafId = 0; }
      drawAmbientBg();
      drawField(0, false);
    }
  }

  // ---------- The stage: the panel slides in and out like the mockup's ----------
  // The shell's columns animate (aurora.css). Opening, the panel keeps its
  // width and slides in as its column grows. Closing, app.js hides and
  // empties the panel at once; its last frame is kept where it was and slides
  // out while the column folds.
  const RESIZE_SETTLE_MS = 160;
  const GHOST_FALLBACK_MS = 900;
  const STAGE_PIN_MS = 700;
  let detailEl = null;
  let detailObs = null;
  let detailRect = null;
  let resizeTimer = 0;

  function noteDetailRect() {
    if (!detailEl || detailEl.hidden) return;
    const r = detailEl.getBoundingClientRect();
    if (r.width > 0 && r.height > 0) detailRect = r;
  }

  function watchDetail() {
    const el = document.getElementById("detail");
    if (!el || el === detailEl) return;
    if (detailObs) detailObs.disconnect();
    detailEl = el;
    detailRect = null;
    detailObs = new MutationObserver(onDetailChange);
    detailObs.observe(el, { attributes: true, attributeFilter: ["hidden"], childList: true });
  }

  function onDetailChange(records) {
    if (!active || !detailEl || !detailEl.hidden) return;
    const rect = detailRect;
    detailRect = null;
    if (motionMode() !== "full" || !rect || !records.some((r) => r.type === "attributes")) return;
    const removed = records.flatMap((r) => (r.type === "childList" ? [...r.removedNodes] : []));
    if (!removed.length) return;
    const ghost = document.createElement("aside");
    ghost.className = "detail aurora-detail-ghost";
    ghost.setAttribute("aria-hidden", "true");
    ghost.inert = true;
    for (const n of removed) ghost.appendChild(n);
    // A frame put back in the document would load again: keep its space only.
    ghost.querySelectorAll("iframe").forEach((f) => {
      const keep = document.createElement("div");
      keep.style.cssText = f.style.cssText;
      keep.className = f.className;
      f.replaceWith(keep);
    });
    ghost.querySelectorAll("[id]").forEach((n) => n.removeAttribute("id"));
    Object.assign(ghost.style, { left: `${rect.left}px`, top: `${rect.top}px`, width: `${rect.width}px`, height: `${rect.height}px` });
    const done = () => ghost.remove();
    ghost.addEventListener("animationend", done, { once: true });
    setTimeout(done, GHOST_FALLBACK_MS);
    document.body.appendChild(ghost);
  }

  // A conversation read at its end stays at its end while its column changes width.
  function onStageTransition(e) {
    if (!active || e.propertyName !== "grid-template-columns" || !(e.target instanceof Element) || !e.target.matches(".shell")) return;
    const t = document.getElementById("thread");
    if (!t || t.scrollHeight - t.scrollTop - t.clientHeight > 80) return;
    const until = performance.now() + STAGE_PIN_MS;
    const pin = () => {
      if (!t.isConnected) return;
      t.scrollTop = t.scrollHeight;
      if (performance.now() < until) requestAnimationFrame(pin);
    };
    requestAnimationFrame(pin);
  }

  function onResize() {
    if (!active) return;
    resizeCanvases();
    document.documentElement.dataset.auroraResizing = "";
    clearTimeout(resizeTimer);
    resizeTimer = setTimeout(() => { delete document.documentElement.dataset.auroraResizing; }, RESIZE_SETTLE_MS);
  }

  function reconcile() {
    if (isDynamicStyle()) {
      active = true;
      if (typeof AuroraCore === "undefined") document.documentElement.dataset.auroraState = currentState === "ready" ? "idle" : currentState;
      enhanceDynamicDom();
      startLoop();
    } else if (active) {
      teardown();
    }
  }

  const htmlObs = new MutationObserver(reconcile);
  htmlObs.observe(document.documentElement, {
    attributes: true,
    attributeFilter: ["data-style", "data-motion", "data-mode", "data-background"],
  });

  let domTimer = 0;
  function handleDom(records) {
    if (!active) return;
    // Entrances are marked in this callback, before the browser draws the
    // new nodes: marked a timer later, a node was drawn, hidden, then faded
    // back in.
    if (records.some((r) => r.addedNodes.length)) markEntrances();
    if (domTimer) return;
    domTimer = setTimeout(() => { domTimer = 0; enhanceDynamicDom(); }, 45);
  }

  function init() {
    if (typeof AuroraCore !== "undefined") {
      AuroraCore.onDom(handleDom);
      AuroraCore.onState((s) => { currentState = s; });
    } else {
      new MutationObserver(handleDom).observe(document.body, { childList: true, subtree: true, attributes: true, attributeFilter: ["class", "data-pill", "data-status", "aria-pressed", "data-phase", "data-tone"] });
    }
    window.addEventListener("magnus-theme", reconcile);
    document.addEventListener("visibilitychange", () => {
      if (!active) return;
      if (document.hidden && rafId) { cancelAnimationFrame(rafId); rafId = 0; }
      else if (!document.hidden) startLoop();
    });
    document.addEventListener("pointermove", onPointerMove, { passive: true });
    document.addEventListener("pointerdown", onPointerMove, { passive: true });
    window.addEventListener("resize", onResize, { passive: true });
    document.addEventListener("transitionrun", onStageTransition);
    // The panel's place, taken as the person reaches to close it.
    document.addEventListener("pointerdown", () => { if (active) noteDetailRect(); }, { capture: true, passive: true });
    document.addEventListener("keydown", () => { if (active) noteDetailRect(); }, true);
    reconcile();
  }

  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", init);
  else init();
})();
