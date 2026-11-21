"use strict";

/* Obsidian 2 Living Titanium-Glass Presence Orb & Sparse 3D Depth Particle Canvas
   Smoothly interpolates velocity, amplitude, and monochrome/indigo temperature across:
   - idle: calm breathing sine-wave in brushed titanium-silver & cool slate
   - listening: audio-reactive ripple pulses in luminous emerald & silver
   - thinking: high-velocity swirl shifting to architectural indigo (#7C8CFF) & white
   - executing: focused steel-blue & titanium-silver light ribbon */

window.MagnusPresence = (function () {
  const STATES = {
    idle:      { speed: 0.72, amp: 0.13, c1: [220, 224, 234], c2: [120, 136, 162], c3: [248, 250, 252] },
    listening: { speed: 1.48, amp: 0.24, c1: [52, 211, 153],  c2: [148, 163, 184], c3: [209, 250, 229] },
    thinking:  { speed: 2.55, amp: 0.26, c1: [124, 140, 255], c2: [216, 220, 230], c3: [232, 236, 255] },
    executing: { speed: 2.15, amp: 0.22, c1: [96, 165, 250],  c2: [228, 228, 235], c3: [219, 234, 254] }
  };

  function lerp(a, b, t) { return a + (b - a) * t; }
  function lerpRgb(a, b, t) {
    return [lerp(a[0], b[0], t), lerp(a[1], b[1], t), lerp(a[2], b[2], t)];
  }
  function rgba(c, alpha) {
    return `rgba(${c[0] | 0},${c[1] | 0},${c[2] | 0},${alpha.toFixed(3)})`;
  }

  function createParticles(fieldCanvas, curRef) {
    if (!fieldCanvas) return () => {};
    const ctx = fieldCanvas.getContext("2d");
    const dpr = Math.min(window.devicePixelRatio || 1, 1.5);
    let w = 1, h = 1;
    let parX = 0, parY = 0, parTx = 0, parTy = 0;

    function spawn(randomZ) {
      return {
        x: (Math.random() * 2 - 1) * 1.15,
        y: (Math.random() * 2 - 1) * 1.15,
        z: randomZ ? 0.18 + Math.random() * 0.82 : 0.96 + Math.random() * 0.08,
        vx: (Math.random() - 0.5) * 0.014,
        vy: (Math.random() - 0.5) * 0.012 - 0.003,
        vz: 0.02 + Math.random() * 0.028,
        size: 0.8 + Math.random() * 1.4,
        tw: Math.random() * Math.PI * 2,
        tint: Math.random() < 0.7 ? 0 : 1
      };
    }

    const pts = [];
    for (let i = 0; i < 42; i++) pts.push(spawn(true));

    function resize() {
      const rect = fieldCanvas.getBoundingClientRect();
      w = Math.max(1, rect.width);
      h = Math.max(1, rect.height);
      fieldCanvas.width = Math.round(w * dpr);
      fieldCanvas.height = Math.round(h * dpr);
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    }
    resize();
    window.addEventListener("resize", resize, { passive: true });
    document.addEventListener("pointermove", (e) => {
      parTx = ((e.clientX / Math.max(1, window.innerWidth)) - 0.5) * 0.07;
      parTy = ((e.clientY / Math.max(1, window.innerHeight)) - 0.5) * 0.07;
    }, { passive: true });

    return function drawField(dt, phase, animate) {
      ctx.clearRect(0, 0, w, h);
      const pb = 1 - Math.exp(-dt * 3);
      parX = lerp(parX, parTx, pb);
      parY = lerp(parY, parTy, pb);
      const cx = w * 0.5, cy = h * 0.45;
      const scale = Math.min(w, h) * 0.86;
      const speedMul = 0.6 + curRef.speed * 0.45;

      for (let i = 0; i < pts.length; i++) {
        const p = pts[i];
        if (animate) {
          p.x += p.vx * dt * speedMul;
          p.y += p.vy * dt * speedMul;
          p.z -= p.vz * dt * speedMul;
          if (p.z < 0.16 || Math.abs(p.x) > 1.4 || Math.abs(p.y) > 1.4) {
            pts[i] = spawn(false);
            continue;
          }
        }
        const depth = 1 - p.z;
        const persp = 0.64 / p.z;
        const sx = cx + (p.x + parX * (0.3 + depth * 0.9)) * persp * scale;
        const sy = cy + (p.y + parY * (0.3 + depth * 0.9)) * persp * scale;
        if (sx < -24 || sx > w + 24 || sy < -24 || sy > h + 24) continue;
        const fade = Math.min(1, (1.02 - p.z) * 4.5) * Math.min(1, (p.z - 0.16) * 5);
        const tw = 0.75 + Math.sin(phase * 1.2 + p.tw) * 0.25;
        const alpha = 0.42 * (0.25 + depth * 0.75) * fade * tw;
        const r = Math.max(0.7, p.size * (0.55 + depth * 1.45));
        const col = p.tint === 0 ? curRef.c1 : curRef.c2;

        if (r > 1.55) {
          const halo = ctx.createRadialGradient(sx, sy, 0, sx, sy, r * 3.4);
          halo.addColorStop(0, rgba(col, alpha * 0.36));
          halo.addColorStop(1, "rgba(0,0,0,0)");
          ctx.fillStyle = halo;
          ctx.beginPath();
          ctx.arc(sx, sy, r * 3.4, 0, Math.PI * 2);
          ctx.fill();
        }
        ctx.fillStyle = rgba(col, alpha);
        ctx.beginPath();
        ctx.arc(sx, sy, r, 0, Math.PI * 2);
        ctx.fill();
      }
    };
  }

  function create(canvas) {
    if (!canvas) return { setState() {} };
    const ctx = canvas.getContext("2d");
    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    const size = 104;
    canvas.width = size * dpr;
    canvas.height = size * dpr;
    ctx.scale(dpr, dpr);

    let target = STATES.idle;
    let cur = {
      speed: target.speed,
      amp: target.amp,
      c1: [...target.c1],
      c2: [...target.c2],
      c3: [...target.c3]
    };
    let phase = 0;
    let last = performance.now();
    const reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    const drawParticles = createParticles(document.getElementById("hero-particles"), cur);

    function draw(now) {
      const dt = Math.min((now - last) / 1000, 0.05);
      last = now;

      const blend = 1 - Math.exp(-dt * 4.5);
      cur.speed = lerp(cur.speed, target.speed, blend);
      cur.amp = lerp(cur.amp, target.amp, blend);
      cur.c1 = lerpRgb(cur.c1, target.c1, blend);
      cur.c2 = lerpRgb(cur.c2, target.c2, blend);
      cur.c3 = lerpRgb(cur.c3, target.c3, blend);

      if (!reduceMotion) phase += dt * cur.speed;
      drawParticles(dt, phase, !reduceMotion);

      ctx.clearRect(0, 0, size, size);
      const cx = size / 2;
      const cy = size / 2;
      const breath = 1 + Math.sin(phase * 1.4) * (cur.amp * 0.26);
      const baseR = 32 * breath;

      // Subtle titanium outer halo
      const glow = ctx.createRadialGradient(cx, cy, baseR * 0.3, cx, cy, size * 0.5);
      glow.addColorStop(0, rgba(cur.c1, 0.34));
      glow.addColorStop(0.55, rgba(cur.c2, 0.14));
      glow.addColorStop(1, "rgba(0,0,0,0)");
      ctx.fillStyle = glow;
      ctx.fillRect(0, 0, size, size);

      // Liquid-titanium organic orb silhouette
      ctx.save();
      ctx.beginPath();
      const steps = 64;
      for (let i = 0; i <= steps; i++) {
        const a = (i / steps) * Math.PI * 2;
        const wobble =
          Math.sin(a * 3 + phase * 1.6) * cur.amp * 4.6 +
          Math.cos(a * 5 - phase * 2.1) * cur.amp * 2.7;
        const r = baseR + wobble;
        const x = cx + Math.cos(a) * r;
        const y = cy + Math.sin(a) * r;
        if (i === 0) ctx.moveTo(x, y);
        else ctx.lineTo(x, y);
      }
      ctx.closePath();
      ctx.clip();

      const core = ctx.createRadialGradient(cx - 10, cy - 12, 4, cx, cy, baseR + 14);
      core.addColorStop(0, rgba(cur.c3, 0.96));
      core.addColorStop(0.45, rgba(cur.c1, 0.86));
      core.addColorStop(0.85, rgba(cur.c2, 0.65));
      core.addColorStop(1, "rgba(10, 10, 14, 0.95)");
      ctx.fillStyle = core;
      ctx.fillRect(0, 0, size, size);

      ctx.globalCompositeOperation = "lighter";
      for (let k = 0; k < 2; k++) {
        const ox = cx + Math.cos(phase * (1.2 + k * 0.6) + k * 2.1) * 13;
        const oy = cy + Math.sin(phase * (1.5 + k * 0.5) + k * 1.4) * 13;
        const rib = ctx.createRadialGradient(ox, oy, 2, ox, oy, 26);
        rib.addColorStop(0, rgba(k === 0 ? cur.c3 : cur.c2, 0.58));
        rib.addColorStop(1, "rgba(0,0,0,0)");
        ctx.fillStyle = rib;
        ctx.fillRect(0, 0, size, size);
      }
      ctx.restore();

      // Razor-crisp specular rim
      ctx.beginPath();
      ctx.arc(cx, cy, baseR - 1, -Math.PI * 0.85, -Math.PI * 0.15);
      ctx.strokeStyle = "rgba(255, 255, 255, 0.50)";
      ctx.lineWidth = 1.4;
      ctx.lineCap = "round";
      ctx.stroke();

      if (!reduceMotion) requestAnimationFrame(draw);
    }

    requestAnimationFrame(draw);

    return {
      setState(name) {
        if (STATES[name]) target = STATES[name];
      }
    };
  }

  return { create };
})();
