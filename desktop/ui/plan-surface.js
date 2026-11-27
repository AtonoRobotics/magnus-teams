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
   * @param {string} originalPrompt @param {string[]} steps */
  function formatApprovedPlan(originalPrompt, steps) {
    const clean = (steps || []).map(s => String(s || "").trim()).filter(Boolean);
    const list = clean.map((s, i) => `${i + 1}. ${s}`).join("\n");
    return `${originalPrompt}\n\nFollow this approved plan in order:\n${list}`;
  }

  /** Open the interactive editable Plan Surface modal. Resolves to the approved
   * prompt string, or null if the user cancels.
   * @param {string} prompt @returns {Promise<?string>} */
  function open(prompt) {
    return new Promise(done => {
      if (typeof document === "undefined") { done(formatApprovedPlan(prompt, parseSteps(prompt))); return; }
      let steps = parseSteps(prompt);
      const d = document.createElement("dialog");
      d.className = "ask";
      d.setAttribute("aria-label", "Review and edit plan before starting");

      const draw = () => {
        const rows = steps.map((st, i) => {
          const gate = isGated(st) ? `<span class="chip warn" data-plan-gate>Will ask</span>` : `<span class="chip">Auto</span>`;
          return `<div class="row" data-plan-row="${i}" style="gap:6px;align-items:center">
            <span class="mono small muted" style="min-width:1.6em">${i + 1}.</span>
            <input type="text" data-plan-step="${i}" value="${escHtml(st)}" aria-label="Step ${i + 1}" style="flex:1">
            ${gate}
            <button type="button" class="small" data-plan-up="${i}" ${i === 0 ? "disabled" : ""} aria-label="Move step ${i + 1} up">↑</button>
            <button type="button" class="small" data-plan-down="${i}" ${i === steps.length - 1 ? "disabled" : ""} aria-label="Move step ${i + 1} down">↓</button>
            <button type="button" class="small" data-plan-del="${i}" aria-label="Remove step ${i + 1}">×</button>
          </div>`;
        }).join("");
        d.innerHTML = `<div class="stack" style="gap:10px;min-width:min(560px, 86vw)">
          <div><h3>Review plan before starting</h3><p class="small muted">Edit, reorder, or remove steps. Steps marked <strong>Will ask</strong> pause for your approval before acting.</p></div>
          <div class="stack" data-plan-list style="gap:6px">${rows || `<p class="small muted">Add at least one step to start.</p>`}</div>
          <div class="row"><button type="button" class="small" data-plan-add>+ Add step</button></div>
          <div class="row" style="justify-content:flex-end"><button type="button" data-plan-cancel>Cancel</button><button type="button" class="primary" data-plan-start ${steps.some(s => s.trim()) ? "" : "disabled"}>Start plan</button></div>
        </div>`;
      };

      const syncInputs = () => {
        steps = [...d.querySelectorAll("[data-plan-step]")].map(inp => /** @type {HTMLInputElement} */ (inp).value);
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
        const ok = d.returnValue === "start";
        d.remove();
        done(ok ? formatApprovedPlan(prompt, steps) : null);
      });

      draw();
      document.body.append(d);
      d.showModal();
    });
  }

  /** @type {Map<string, string[]>} */
  const livePlans = new Map();

  /** Register approved plan steps for a started task so its live card tracks progress.
   * @param {string} taskId @param {string[]} steps */
  function trackTask(taskId, steps) {
    if (!taskId || !Array.isArray(steps) || !steps.length) return;
    livePlans.set(taskId, steps.map(s => String(s || "").trim()).filter(Boolean));
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
    for (const [taskId] of livePlans) {
      const host = document.querySelector(`[data-live-plan="${CSS.escape(taskId)}"]`);
      if (host && host.parentElement) {
        const tmp = document.createElement("div");
        tmp.innerHTML = renderLiveCard(taskId);
        if (tmp.firstElementChild) host.replaceWith(tmp.firstElementChild);
      }
    }
  }

  if (typeof UIStore !== "undefined") UIStore.subscribe("tasksById", syncLiveCards);

  return { parseSteps, isGated, formatApprovedPlan, open, trackTask, renderLiveCard, livePlans };
})();
