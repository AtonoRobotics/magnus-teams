"use strict";

/* Organic Liquid-Glass Aurora Orb & Flowing Ribbon Presence Shader
   Smoothly interpolates velocity, amplitude, and color temperature across:
   - idle: slow breathing sine-wave in cool ambient sapphire/cyan
   - listening: audio-reactive ripple pulses in luminous aqua/mint
   - thinking: high-velocity swirl shifting to vibrant violet/cyan
   - executing: focused energetic cyan/cobalt light ribbon */

window.MagnusPresence = (function () {
  const STATES = {
    idle:      { speed: 0.75, amp: 0.14, c1: [79, 157, 255],  c2: [56, 189, 248],  c3: [147, 197, 253] },
    listening: { speed: 1.45, amp: 0.24, c1: [52, 211, 153],  c2: [56, 189, 248],  c3: [167, 243, 208] },
    thinking:  { speed: 2.65, amp: 0.28, c1: [155, 109, 255], c2: [56, 189, 248],  c3: [216, 180, 254] },
    executing: { speed: 2.15, amp: 0.22, c1: [56, 189, 248],  c2: [129, 140, 248], c3: [94, 234, 212] }
  };

  function lerp(a, b, t) { return a + (b - a) * t; }
  function lerpRgb(a, b, t) {
    return [lerp(a[0], b[0], t), lerp(a[1], b[1], t), lerp(a[2], b[2], t)];
  }
  function rgba(c, alpha) {
    return `rgba(${c[0] | 0},${c[1] | 0},${c[2] | 0},${alpha})`;
  }

  function create(canvas) {
    if (!canvas) return { setState() {} };
    const ctx = canvas.getContext("2d");
    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    const size = 112;
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

      ctx.clearRect(0, 0, size, size);
      const cx = size / 2;
      const cy = size / 2;
      const breath = 1 + Math.sin(phase * 1.4) * (cur.amp * 0.28);
      const baseR = 35 * breath;

      // Ambient outer aura glow
      const glow = ctx.createRadialGradient(cx, cy, baseR * 0.3, cx, cy, size * 0.5);
      glow.addColorStop(0, rgba(cur.c1, 0.42));
      glow.addColorStop(0.55, rgba(cur.c2, 0.16));
      glow.addColorStop(1, "rgba(0,0,0,0)");
      ctx.fillStyle = glow;
      ctx.fillRect(0, 0, size, size);

      // Liquid-glass organic orb silhouette
      ctx.save();
      ctx.beginPath();
      const steps = 64;
      for (let i = 0; i <= steps; i++) {
        const a = (i / steps) * Math.PI * 2;
        const wobble =
          Math.sin(a * 3 + phase * 1.6) * cur.amp * 5.2 +
          Math.cos(a * 5 - phase * 2.1) * cur.amp * 3.1;
        const r = baseR + wobble;
        const x = cx + Math.cos(a) * r;
        const y = cy + Math.sin(a) * r;
        if (i === 0) ctx.moveTo(x, y);
        else ctx.lineTo(x, y);
      }
      ctx.closePath();
      ctx.clip();

      // Deep glass core gradient
      const core = ctx.createRadialGradient(cx - 10, cy - 12, 4, cx, cy, baseR + 14);
      core.addColorStop(0, rgba(cur.c3, 0.92));
      core.addColorStop(0.45, rgba(cur.c1, 0.85));
      core.addColorStop(0.85, rgba(cur.c2, 0.65));
      core.addColorStop(1, "rgba(8, 12, 24, 0.92)");
      ctx.fillStyle = core;
      ctx.fillRect(0, 0, size, size);

      // Flowing internal aurora ribbons
      ctx.globalCompositeOperation = "lighter";
      for (let k = 0; k < 2; k++) {
        const ox = cx + Math.cos(phase * (1.2 + k * 0.6) + k * 2.1) * 14;
        const oy = cy + Math.sin(phase * (1.5 + k * 0.5) + k * 1.4) * 14;
        const rib = ctx.createRadialGradient(ox, oy, 2, ox, oy, 28);
        rib.addColorStop(0, rgba(k === 0 ? cur.c3 : cur.c2, 0.65));
        rib.addColorStop(1, "rgba(0,0,0,0)");
        ctx.fillStyle = rib;
        ctx.fillRect(0, 0, size, size);
      }
      ctx.restore();

      // Specular glass rim highlight
      ctx.beginPath();
      ctx.arc(cx, cy, baseR - 1, -Math.PI * 0.85, -Math.PI * 0.15);
      ctx.strokeStyle = "rgba(255, 255, 255, 0.36)";
      ctx.lineWidth = 1.4;
      ctx.lineCap = "round";
      ctx.stroke();

      requestAnimationFrame(draw);
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
