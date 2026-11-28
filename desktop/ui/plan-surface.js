// Editable Pre-Execution Plan Surface (Tier 2 Phase 5):
// When the user picks "Review plan first" in the Autonomy Leash, opens an
// interactive step-list editor before the run starts so they can inspect,
// edit, reorder, add, or delete steps and see which steps will pause at
// approval gates. Approving hands the finalized plan directly to Magnus.
"use strict";

const PlanSurface = (() => {
  const GATE_RE = /\b(send|email|mail|post|publish|delete|remove|buy|purchase|pay|order|transfer|book|invite|cancel|merge|deploy)\b/i;
  const escHtml = s => String(s ?? "").replace(/[&<>"']/g, c => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);

  /** Whether a step description touches an external, destructive, or financial action. @param {string} step */
  function isGated(step) {
    return GATE_RE.test(String(step || ""));
  }

  /** Derive initial discrete editable steps from a plain-language goal. @param {string} prompt */
  function parseSteps(prompt) {
    const raw = String(prompt || "").trim();
    if (!raw) return [];
    const lines = raw.split(/\n+/).map(l => l.replace(/^\s*(?:[-*•]|\d+[.)])\s*/, "").trim()).filter(Boolean);
    if (lines.length >= 2) return lines;
    const clauses = raw.split(/(?:,\s*then\s+|\s+then\s+|;\s+|\.\s+(?=[A-Z]))/).map(s => s.replace(/\.$/, "").trim()).filter(Boolean);
    if (clauses.length >= 2) return clauses;
    return [
      `Gather context and inspect relevant files/apps for: ${raw}`,
      `Execute the core work and prepare the output`,
      `Verify results and summarize next steps for review`,
    ];
  }

  /** Format the user-approved step list into a clear structured prompt for Magnus.
   * @param {string} originalPrompt @param {string[]} steps @param {string} [commentNotes] */
  function formatApprovedPlan(originalPrompt, steps, commentNotes = "") {
    const clean = (steps || []).map(s => String(s || "").trim()).filter(Boolean);
    const list = clean.map((s, i) => `${i + 1}. ${s}`).join("\n");
    const extra = commentNotes ? `\n\n${commentNotes}` : "";
    return `${originalPrompt}\n\nFollow this approved plan in order:\n${list}${extra}`;
  }

  /** Open the interactive editable Plan Surface modal. Resolves to the approved
   * prompt string, or null if the user cancels.
   * @param {string} prompt @returns {Promise<?string>} */
  function open(prompt) {
    return new Promise(done => {
      if (typeof document === "undefined") { done(formatApprovedPlan(prompt, parseSteps(prompt))); return; }
      let steps = parseSteps(prompt);
      const surfaceId = `plan-modal-${Date.now()}`;
      let activeCommentIdx = -1;
      const d = document.createElement("dialog");
      d.className = "ask";
      d.setAttribute("aria-label", "Review and edit plan before starting");

      const draw = () => {
        const rows = steps.map((st, i) => {
          const key = `step-${i}`;
          const gate = isGated(st) ? `<span class="chip warn" data-plan-gate>Will ask</span>` : `<span class="chip">Auto</span>`;
          const pills = typeof InlineComments !== "undefined" ? InlineComments.pillsHtml(surfaceId, key) : "";
          const editor = activeCommentIdx === i
            ? `<div class="ic-editor" data-plan-comment-box="${i}"><input type="text" data-plan-comment-input="${i}" placeholder="Add inline note for step ${i + 1}…" aria-label="Comment on step ${i + 1}"><button type="button" class="small primary" data-plan-comment-save="${i}">Add</button></div>`
            : "";
          return `<div class="plan-step-wrap" data-plan-row="${i}">
            <div class="row" style="gap:6px;align-items:center">
              <span class="mono small muted" style="min-width:1.6em">${i + 1}.</span>
              <input type="text" data-plan-step="${i}" value="${escHtml(st)}" aria-label="Step ${i + 1}" style="flex:1">
              ${gate}
              <button type="button" class="small" data-plan-comment="${i}" aria-label="Comment on step ${i + 1}">Comment</button>
              <button type="button" class="small" data-plan-up="${i}" ${i === 0 ? "disabled" : ""} aria-label="Move step ${i + 1} up">↑</button>
              <button type="button" class="small" data-plan-down="${i}" ${i === steps.length - 1 ? "disabled" : ""} aria-label="Move step ${i + 1} down">↓</button>
              <button type="button" class="small" data-plan-del="${i}" aria-label="Remove step ${i + 1}">×</button>
            </div>${pills}${editor}</div>`;
        }).join("");
        const footer = typeof InlineComments !== "undefined" ? InlineComments.footerHtml(surfaceId) : "";
        d.innerHTML = `<div class="stack" style="gap:10px;min-width:min(560px, 86vw)">
          <div><h3>Review plan before starting</h3><p class="small muted">Edit, comment inline, reorder, or remove steps. Steps marked <strong>Will ask</strong> pause for your approval before acting.</p></div>
          <div class="stack" data-plan-list style="gap:6px">${rows || `<p class="small muted">Add at least one step to start.</p>`}</div>
          ${footer}
          <div class="row"><button type="button" class="small" data-plan-add>+ Add step</button></div>
          <div class="row" style="justify-content:flex-end"><button type="button" data-plan-cancel>Cancel</button><button type="button" class="primary" data-plan-start ${steps.some(s => s.trim()) ? "" : "disabled"}>Start plan</button></div>
        </div>`;
        if (activeCommentIdx >= 0) {
          const inp = /** @type {?HTMLInputElement} */ (d.querySelector(`[data-plan-comment-input="${activeCommentIdx}"]`));
          inp?.focus();
        }
      };

      const syncInputs = () => {
        steps = [...d.querySelectorAll("[data-plan-step]")].map(inp => /** @type {HTMLInputElement} */ (inp).value);
      };

      const saveActiveComment = i => {
        const inp = /** @type {?HTMLInputElement} */ (d.querySelector(`[data-plan-comment-input="${i}"]`));
        const val = inp?.value?.trim() || "";
        if (val && typeof InlineComments !== "undefined") InlineComments.add(surfaceId, `step-${i}`, val);
        activeCommentIdx = -1;
        draw();
      };

      d.addEventListener("input", e => {
        const inp = /** @type {?HTMLInputElement} */ (/** @type {HTMLElement} */ (e.target).closest?.("[data-plan-step]"));
        if (!inp) return;
        const i = Number(inp.dataset.planStep);
        steps[i] = inp.value;
        const row = inp.closest("[data-plan-row]");
        const chip = row?.querySelector(".chip");
        if (chip) {
          const gated = isGated(inp.value);
          chip.className = gated ? "chip warn" : "chip";
          chip.textContent = gated ? "Will ask" : "Auto";
        }
        const startBtn = /** @type {?HTMLButtonElement} */ (d.querySelector("[data-plan-start]"));
        if (startBtn) startBtn.disabled = !steps.some(s => s.trim());
      });

      d.addEventListener("keydown", e => {
        const inp = /** @type {?HTMLInputElement} */ (/** @type {HTMLElement} */ (e.target).closest?.("[data-plan-comment-input]"));
        if (!inp) return;
        if (e.key === "Enter") { e.preventDefault(); saveActiveComment(Number(inp.dataset.planCommentInput)); }
        if (e.key === "Escape") { e.preventDefault(); activeCommentIdx = -1; draw(); }
      });

      d.addEventListener("click", e => {
        const t = /** @type {HTMLElement} */ (e.target);
        if (t.closest?.("[data-plan-add]")) {
          syncInputs();
          steps.push("");
          draw();
          const last = /** @type {?HTMLInputElement} */ (d.querySelector(`[data-plan-step="${steps.length - 1}"]`));
          last?.focus();
          return;
        }
        const cbtn = t.closest?.("[data-plan-comment]");
        if (cbtn) {
          syncInputs();
          const i = Number(cbtn.dataset.planComment);
          activeCommentIdx = activeCommentIdx === i ? -1 : i;
          draw();
          return;
        }
        const csave = t.closest?.("[data-plan-comment-save]");
        if (csave) { syncInputs(); saveActiveComment(Number(csave.dataset.planCommentSave)); return; }
        if (t.closest?.("[data-ic-clear]")) {
          if (typeof InlineComments !== "undefined") InlineComments.clear(surfaceId);
          draw();
          return;
        }
        if (t.closest?.("[data-ic-revise]")) {
          syncInputs();
          d.close("revise");
          return;
        }
        const del = t.closest?.("[data-plan-del]");
        if (del) { syncInputs(); steps.splice(Number(del.dataset.planDel), 1); draw(); return; }
        const up = t.closest?.("[data-plan-up]");
        if (up) {
          syncInputs();
          const i = Number(up.dataset.planUp);
          if (i > 0) [steps[i - 1], steps[i]] = [steps[i], steps[i - 1]];
          draw();
          return;
        }
        const down = t.closest?.("[data-plan-down]");
        if (down) {
          syncInputs();
          const i = Number(down.dataset.planDown);
          if (i < steps.length - 1) [steps[i], steps[i + 1]] = [steps[i + 1], steps[i]];
          draw();
          return;
        }
        if (t.closest?.("[data-plan-cancel]")) { d.close("cancel"); return; }
        if (t.closest?.("[data-plan-start]")) {
          syncInputs();
          if (!steps.some(s => s.trim())) return;
          d.close("start");
        }
      });

      d.addEventListener("close", () => {
        const rv = d.returnValue;
        const labels = Object.fromEntries(steps.map((s, i) => [`step-${i}`, `Step ${i + 1}: ${s}`]));
        const notes = typeof InlineComments !== "undefined" ? InlineComments.formatRevision(surfaceId, labels) : "";
        if (typeof InlineComments !== "undefined") InlineComments.clear(surfaceId);
        d.remove();
        if (rv === "revise" && notes) { done(formatApprovedPlan(prompt, steps, notes)); return; }
        done(rv === "start" ? formatApprovedPlan(prompt, steps, notes) : null);
      });

      draw();
      document.body.append(d);
      d.showModal();
    });
  }

  const LIVE_PLANS_KEY = "magnus.livePlans.v1";
  /** @type {Map<string, string[]>} */
  const livePlans = new Map();
  try {
    if (typeof sessionStorage !== "undefined") {
      const saved = JSON.parse(sessionStorage.getItem(LIVE_PLANS_KEY) || "null");
      if (saved && typeof saved === "object") {
        for (const [k, v] of Object.entries(saved)) if (Array.isArray(v) && v.length) livePlans.set(k, v);
      }
    }
  } catch { /* private mode */ }
  function persistLivePlans() {
    try {
      if (typeof sessionStorage !== "undefined") {
        sessionStorage.setItem(LIVE_PLANS_KEY, JSON.stringify(Object.fromEntries(livePlans)));
      }
    } catch { /* private mode */ }
  }

  /** Register approved plan steps for a started task so its live card tracks progress.
   * @param {string} taskId @param {string[]} steps */
  function trackTask(taskId, steps) {
    if (!taskId || !Array.isArray(steps) || !steps.length) return;
    livePlans.set(taskId, steps.map(s => String(s || "").trim()).filter(Boolean));
    persistLivePlans();
    syncLiveCards();
  }

  /** Render an inline live Plan Card whose steps tick off from UIStore task state.
   * @param {string} taskId */
  function renderLiveCard(taskId) {
    const steps = livePlans.get(taskId);
    if (!steps || !steps.length) return "";
    const task = typeof UIStore !== "undefined" ? UIStore.getTask(taskId) : null;
    const units = Array.isArray(task?.plan?.units) ? task.plan.units : [];
    const ended = task && ["completed", "failed", "cancelled"].includes(task.status);
    const items = steps.map((st, i) => {
      const u = units[i];
      const state = ended && task.status === "completed" ? "done"
        : u?.status === "done" ? "done"
        : u?.status === "active" ? "active"
        : i === 0 && task && !ended ? "active" : "todo";
      const mark = state === "done" ? "✓" : state === "active" ? "…" : "○";
      return `<li data-plan-unit-state="${state}"><span class="mono" aria-hidden="true">${mark}</span> ${escHtml(st)}</li>`;
    }).join("");
    return `<div class="card flat" data-live-plan="${escHtml(taskId)}" style="margin:6px 0"><div class="spread"><strong class="small">Approved plan</strong><span class="small muted">${ended ? escHtml(task.status) : "In progress"}</span></div><ol class="stack small" style="margin:6px 0 0 18px;gap:4px">${items}</ol></div>`;
  }

  function syncLiveCards() {
    if (typeof document === "undefined") return;
    let pruned = false;
    for (const [taskId] of [...livePlans]) {
      const task = typeof UIStore !== "undefined" ? UIStore.getTask(taskId) : null;
      const host = document.querySelector(`[data-live-plan="${CSS.escape(taskId)}"]`);
      if (host && host.parentElement) {
        const tmp = document.createElement("div");
        tmp.innerHTML = renderLiveCard(taskId);
        if (tmp.firstElementChild) host.replaceWith(tmp.firstElementChild);
      } else if (task && ["completed", "failed", "cancelled"].includes(task.status)) {
        livePlans.delete(taskId);
        pruned = true;
      }
    }
    if (pruned) persistLivePlans();
  }

  if (typeof UIStore !== "undefined") UIStore.subscribe("tasksById", syncLiveCards);

  return { parseSteps, isGated, formatApprovedPlan, open, trackTask, renderLiveCard, livePlans };
})();
