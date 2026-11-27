// Visual Task Replay: upgrades the step scrubber into a synchronized replay
// stage with Prev / Play-Pause / Next controls, step-indexed VM screenshots
// (from /v1/tasks/:id/timeline `frames`), tool call decisions, and outputs.
"use strict";

const TaskReplay = (() => {
  const PLAY_INTERVAL_MS = 1200;
  const escHtml = s => String(s ?? "").replace(/[&<>"']/g, c => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);

  /** Find the latest captured frame at or before `stepNum`.
   * @param {Array<{ step: number, image_b64: string }>} frames @param {number} stepNum */
  function frameForStep(frames, stepNum) {
    if (!Array.isArray(frames) || !frames.length) return null;
    let best = null;
    for (const f of frames) {
      if (f && typeof f.image_b64 === "string" && f.step <= stepNum) {
        if (!best || f.step >= best.step) best = f;
      }
    }
    return best;
  }

  /** Format up to 3 concise scalar fields from a tool call input object. @param {any} input */
  function summarizeInput(input) {
    if (!input || typeof input !== "object" || Array.isArray(input)) return "";
    const pairs = Object.entries(input)
      .filter(([, v]) => v !== null && v !== undefined && v !== "" && typeof v !== "object")
      .slice(0, 3)
      .map(([k, v]) => `${k.replace(/_/g, " ")}: ${String(v).slice(0, 72)}`);
    return pairs.join(" · ");
  }

  /** Render one replay step's visual stage (screenshot + tool calls + inputs + decisions + verifications).
   * @param {any} stepData @param {{ step: number, image_b64: string } | null} frame @param {any[]} [verifications] */
  function renderStage(stepData, frame, verifications = []) {
    const imgHtml = frame
      ? `<div class="stack" style="margin-top:8px"><img src="data:image/png;base64,${escHtml(frame.image_b64)}" alt="Screen at step ${escHtml(frame.step)}" style="width:100%;max-height:240px;object-fit:contain;border-radius:var(--r-sm);border:1px solid var(--line)"></div>`
      : "";
    const calls = Array.isArray(stepData?.calls) ? stepData.calls : [];
    const passedCount = Array.isArray(verifications) ? verifications.filter(v => v && v.passed).length : 0;
    const metaHtml = (calls.length || passedCount)
      ? `<div class="row small muted" data-replay-meta style="margin-top:6px;gap:8px;flex-wrap:wrap"><span>${calls.length} tool call${calls.length === 1 ? "" : "s"}</span>${passedCount ? `<span class="chip ok" data-replay-verified>✓ ${passedCount} verified</span>` : ""}</div>`
      : "";
    const callsHtml = calls.length
      ? `<div class="stack" style="margin-top:6px;gap:6px">${calls.map(c => {
          const nice = String(c.tool || "tool").replace(/^[^_]+__/, "").replace(/_/g, " ");
          const dec = (c.decisions || []).map(d => `<span class="chip ${d.outcome === "allow" ? "ok" : d.outcome === "block" ? "bad" : "warn"}">${escHtml(d.outcome)}</span>`).join(" ");
          const err = c.is_error ? `<span class="chip bad">Error</span>` : "";
          const inSummary = summarizeInput(c.input);
          const inPreview = inSummary ? `<div class="small muted" data-replay-input style="margin-top:4px">${escHtml(inSummary)}</div>` : "";
          const hasInputObj = c.input && typeof c.input === "object" && Object.keys(c.input).length > 0;
          const inDetails = hasInputObj ? `<details class="small" style="margin-top:4px"><summary class="muted">Input details</summary><pre class="small muted" style="max-height:90px;overflow:auto">${escHtml(JSON.stringify(c.input, null, 2).slice(0, 600))}</pre></details>` : "";
          const out = c.result ? `<pre class="small muted" style="max-height:90px;overflow:auto;margin-top:4px">${escHtml(String(c.result).slice(0, 400))}</pre>` : "";
          return `<div class="card flat"><div class="spread"><strong class="small">${escHtml(nice)}</strong><span class="row" style="gap:4px">${dec}${err}</span></div>${inPreview}${inDetails}${out}</div>`;
        }).join("")}</div>`
      : "";
    const textHtml = stepData?.text
      ? `<p class="small muted" style="margin:6px 0 0">${escHtml(String(stepData.text).slice(0, 280))}</p>`
      : "";
    if (!imgHtml && !metaHtml && !callsHtml && !textHtml) return "";
    return `${imgHtml}${metaHtml}${textHtml}${callsHtml}`;
  }

  /** Detect the currently open task ID from Panel or location.hash. */
  function activeTaskId() {
    if (typeof Panel !== "undefined" && typeof Panel.what === "string" && Panel.what.startsWith("task:")) {
      return Panel.what.slice(5);
    }
    const m = (typeof location !== "undefined" ? location.hash : "").match(/^#\/task\/([^/?#]+)/);
    return m ? decodeURIComponent(m[1]) : "";
  }

  /** Mount interactive Visual Task Replay controls onto `.nova-replay`.
   * @param {HTMLElement} bar @param {HTMLElement[]} steps @param {string} [taskId] */
  function mount(bar, steps, taskId = activeTaskId()) {
    const total = steps.length;
    let idx = total;
    let timer = 0;
    /** @type {{ steps: any[], frames: Array<{ step: number, image_b64: string }> } | null} */
    let timeline = null;

    bar.innerHTML = `<div class="nova-replay-head">
      <strong class="small">Task Replay</strong>
      <div class="row" style="gap:6px">
        <button type="button" class="small" data-replay-prev aria-label="Previous step">←</button>
        <button type="button" class="small" data-replay-play aria-label="Play replay">Play</button>
        <button type="button" class="small" data-replay-next aria-label="Next step">→</button>
        <span class="mono small muted" data-nova-replay-lbl>Step ${total} / ${total}</span>
      </div>
    </div>
    <input type="range" class="nova-replay-range" min="1" max="${total}" value="${total}" aria-label="Scrub task steps">
    <div data-replay-stage></div>`;

    const range = /** @type {HTMLInputElement} */ (bar.querySelector(".nova-replay-range"));
    const lbl = bar.querySelector("[data-nova-replay-lbl]");
    const playBtn = /** @type {?HTMLButtonElement} */ (bar.querySelector("[data-replay-play]"));
    const stage = bar.querySelector("[data-replay-stage]");

    const stopPlay = () => {
      if (timer) { clearInterval(timer); timer = 0; }
      if (playBtn) playBtn.textContent = "Play";
    };

    const apply = nextIdx => {
      idx = Math.max(1, Math.min(total, Number(nextIdx) || 1));
      if (range) range.value = String(idx);
      if (lbl) lbl.textContent = `Step ${idx} / ${total}`;
      steps.forEach((el, i) => {
        el.classList.toggle("nova-step-active", i === idx - 1);
        el.classList.toggle("nova-step-future", i >= idx);
      });
      if (stage && timeline) {
        const st = timeline.steps?.[idx - 1] || null;
        const stepNum = st?.step ?? idx;
        stage.innerHTML = renderStage(st, frameForStep(timeline.frames, stepNum), timeline.verifications);
      }
    };

    range?.addEventListener("input", () => { stopPlay(); apply(Number(range.value)); });
    bar.addEventListener("click", e => {
      const t = /** @type {HTMLElement} */ (e.target);
      if (t.closest?.("[data-replay-prev]")) { stopPlay(); apply(idx - 1); return; }
      if (t.closest?.("[data-replay-next]")) { stopPlay(); apply(idx + 1); return; }
      if (t.closest?.("[data-replay-play]")) {
        if (timer) { stopPlay(); return; }
        if (idx >= total) apply(1);
        if (playBtn) playBtn.textContent = "Pause";
        timer = window.setInterval(() => {
          if (!bar.isConnected) { stopPlay(); return; }
          if (idx >= total) { stopPlay(); return; }
          apply(idx + 1);
        }, PLAY_INTERVAL_MS);
      }
    });

    if (taskId && typeof Server !== "undefined" && Server.tasks?.timeline) {
      Server.tasks.timeline(taskId).then(data => {
        if (!bar.isConnected || !data) return;
        timeline = {
          steps: Array.isArray(data.steps) ? data.steps : [],
          frames: Array.isArray(data.frames) ? data.frames : [],
          verifications: Array.isArray(data.verifications) ? data.verifications : [],
        };
        apply(idx);
      }).catch(() => {});
    }
  }

  return { mount, frameForStep, summarizeInput, renderStage, activeTaskId };
})();
