// AuroraStageView (Tier 3 Phase 4):
// Canvas shell, decision preview formatting, thread history compaction,
// completed-answer canvas annotations, and word-by-word answer streaming.
// Extracted from aurora-stage.js to keep aurora-stage.js below 800 lines.
"use strict";

const AuroraStageView = (() => {
  const STAGE_VIEW_WIDE_CANVAS = new Set(["computer", "file", "task", "schedule", "files", "shared"]);
  const STAGE_VIEW_PREVIEW_HEADER_RE = /^(to|recipient|recipients|cc|bcc|channel|room|destination|url|host|site|file|path|calendar|event|subject|title|summary|amount|total|price|max_usd)$/i;
  const STAGE_VIEW_PREVIEW_BODY_RE = /^(body|message|text|content|note|description|html|draft)$/i;
  const STAGE_VIEW_PREVIEW_ROWS_MAX = 6;
  const STAGE_VIEW_PREVIEW_BODY_MAX_CHARS = 600;
  const STAGE_VIEW_WORD_STEP_MS = 34;
  const STAGE_VIEW_WORDS_STREAMED = 120;
  const STAGE_VIEW_WORD_ANIM_MS = 340;
  const STAGE_VIEW_WORD_UNWRAP_BUFFER_MS = 80;

  /** Marks verbose thread history cards with collapsed details. @param {Element} conv */
  function compactThreadHistory(conv) {
    const thread = conv.querySelector(":scope > .thread");
    if (!thread) return;
    const apply = () => {
      thread.querySelectorAll(".reqgroup.chatcard[data-chat-tasks], .reqgroup.chatcard.sub-agents, .result[data-result]").forEach(el => {
        if (!(el instanceof HTMLElement)) return;
        el.dataset.history = "1";
        el.querySelectorAll("details").forEach(d => {
          if (d.dataset.historyInit) return;
          if (d.querySelector("iframe.composed, .composed-wrap")) return;
          d.dataset.historyInit = "1";
          d.open = false;
        });
      });
      thread.querySelectorAll(".decide").forEach(dec => {
        if (!(dec instanceof HTMLElement)) return;
        const cards = dec.querySelectorAll(".card.item[data-id]");
        if (cards.length && cards.length === dec.children.length) dec.dataset.historyHidden = "1";
        else delete dec.dataset.historyHidden;
      });
    };
    if (typeof ReadingAnchors !== "undefined" && ReadingAnchors.hold) ReadingAnchors.hold(thread, apply);
    else apply();
  }

  /** Adds a quiet "Inspect Canvas →" footer row on completed answers backed by a task or artifact.
   * @param {Element} conv @param {Function} endedFn @param {Function} namedFn @param {Function} firstLineFn @param {Function} htmlFn */
  function annotateAnswers(conv, endedFn, namedFn, firstLineFn, htmlFn) {
    const tasks = typeof App !== "undefined" && Array.isArray(App.tasks) ? App.tasks : [];
    const byId = new Map(tasks.map(t => [t.task_id, t]));
    conv.querySelectorAll('.entry[data-kind="answer"] > .msg.magnus[data-task]').forEach(msg => {
      if (msg.querySelector(":scope > .aurora-canvas-row")) return;
      const id = /** @type {HTMLElement} */ (msg).dataset.task || "";
      if (!id) return;
      const t = byId.get(id);
      const rich = Boolean(msg.querySelector(".shown-frame, .previews, .fchip-inline"));
      if (!t && !rich) return;
      if (t && !endedFn(t) && !rich) return;
      const meta = t ? `${namedFn(t)}${t.summary ? ` · ${firstLineFn(t.summary)}` : ""}` : "Work result";
      const verified = !!(t && (t.verified || t.receipts_count > 0 || t.has_deliverable || (Array.isArray(t.receipts) && t.receipts.length > 0)));
      const badge = verified ? `<span class="aurora-verified-pill">✓ Verified</span>` : "";
      const row = document.createElement("div");
      row.className = "aurora-canvas-row";
      row.innerHTML = `${badge}<span class="aurora-canvas-meta">${htmlFn(meta)}</span><button type="button" class="aurora-canvas-open" data-open="task:${htmlFn(id)}">Inspect Canvas →</button>`;
      msg.append(row);
    });
  }

  /** Extracts a clean read-only preview of human-facing fields from `.ask-content .what .field`.
   * @param {Element} more @param {Function} htmlFn @returns {HTMLElement|null} */
  function decisionPreview(more, htmlFn) {
    const rows = [];
    const seenKeys = new Set();
    let bodyText = "";
    for (const field of more.querySelectorAll(".what > .field")) {
      const label = field.firstElementChild?.textContent?.trim() || "";
      const valEl = field.children[1];
      const val = valEl?.textContent?.trim() || "";
      if (!label || !val || val === "none") continue;
      const lower = label.toLowerCase();
      if (STAGE_VIEW_PREVIEW_BODY_RE.test(lower)) {
        if (!bodyText) bodyText = val.length > STAGE_VIEW_PREVIEW_BODY_MAX_CHARS ? `${val.slice(0, STAGE_VIEW_PREVIEW_BODY_MAX_CHARS).trimEnd()}…` : val;
      } else if (STAGE_VIEW_PREVIEW_HEADER_RE.test(lower) && !seenKeys.has(lower) && rows.length < STAGE_VIEW_PREVIEW_ROWS_MAX) {
        seenKeys.add(lower);
        const nice = label.replace(/_/g, " ");
        rows.push([nice[0].toUpperCase() + nice.slice(1), val]);
      }
    }
    if (!rows.length && !bodyText) return null;
    const card = document.createElement("div");
    card.className = "decision-preview";
    card.setAttribute("role", "region");
    card.setAttribute("aria-label", "Action preview");
    const rowsHtml = rows.length
      ? `<div class="decision-preview-rows">${rows.map(([k, v]) => `<div class="decision-preview-row"><span class="decision-preview-key">${htmlFn(k)}</span><span class="decision-preview-val">${htmlFn(v)}</span></div>`).join("")}</div>`
      : "";
    const bodyHtml = bodyText ? `<p class="decision-preview-body">${htmlFn(bodyText)}</p>` : "";
    card.innerHTML = `${rowsHtml}${bodyHtml}`;
    return card;
  }

  /** Shapes a decision card for the artifact canvas.
   * @param {string} body @param {Function} htmlFn @returns {?{ kind: string, title: string, body: string }} */
  function decision(body, htmlFn) {
    const t = document.createElement("template");
    t.innerHTML = body;
    const card = t.content.querySelector("article.card.item");
    const ask = card?.querySelector(".ask[data-ask]");
    const sentence = ask?.querySelector(".ask-sentence");
    if (!card || !ask || !sentence || !ask.querySelector(".ask-answers")) return null;
    const head = card.querySelector(":scope > .spread");
    const kind = head?.querySelector(".kind")?.textContent?.trim() || "Decision";
    const title = sentence.textContent?.trim() || kind;
    /** @type {HTMLElement} */ (sentence).hidden = true;
    const about = document.createElement("div");
    about.className = "decision-about";
    for (const el of [...card.children]) if (el !== head && el !== ask) about.append(el);
    const meta = head?.querySelector(".small.faint");
    if (meta) { const p = document.createElement("p"); p.className = "small faint decision-meta"; p.innerHTML = meta.innerHTML; about.append(p); }
    head?.remove();
    ask.querySelector(":scope > .ask-preview")?.remove();
    let more = ask.querySelector(":scope > details.ask-content");
    if (!more && about.childNodes.length) {
      more = document.createElement("details");
      more.className = "ask-content";
      const summary = document.createElement("summary");
      summary.textContent = "Details";
      more.append(summary);
      ask.querySelector(":scope > .ask-answers")?.before(more);
    }
    if (more) {
      const preview = decisionPreview(more, htmlFn);
      if (preview) {
        more.before(preview);
        const sum = more.querySelector(":scope > summary");
        if (sum && (sum.textContent === "See exactly what it would do" || sum.textContent === "Details")) sum.textContent = "See exact details";
      }
      if (about.childNodes.length) more.querySelector(":scope > summary")?.after(about);
    }
    return { kind, title, body: card.outerHTML };
  }

  /** Renders the artifact canvas shell.
   * @param {{ kind: string, title: string, head?: string|null, backBtn: string, tabsHtml?: string, body: string, foot?: string }} v @param {Function} htmlFn */
  function shell(v, htmlFn) {
    const size = STAGE_VIEW_WIDE_CANVAS.has(v.kind) ? "wide" : "compact";
    const tabs = v.tabsHtml || "";
    const d = v.kind === "item" ? decision(v.body, htmlFn) : null;
    if (d) {
      return `<section class="stage-artifact stage-decision" data-canvas-size="compact" aria-label="${htmlFn(d.title)}">
      <div class="artifact-top"><div class="artifact-title"><p class="decision-kind">${htmlFn(d.kind)}</p><h2>${htmlFn(d.title)}</h2></div><button type="button" class="artifact-close" data-toggle-inspector>Close</button></div>
      ${tabs}
      <div class="artifact-content detail-body" data-panel="item">${d.body}</div>
    </section>`;
    }
    return `<section class="stage-artifact" data-canvas-size="${size}" aria-label="${htmlFn(v.title)}">
      <div class="artifact-top">${v.backBtn}<div class="artifact-title">${v.head || `<h2>${htmlFn(v.title)}</h2>`}</div><button type="button" class="artifact-close" data-toggle-inspector>Close</button></div>
      ${tabs}
      <div class="artifact-content detail-body" data-panel="${htmlFn(v.kind)}">${v.body}</div>
      ${v.foot || ""}
    </section>`;
  }

  /** Streams a new answer block word by word, then unwraps temporary spans back to plain text.
   * @param {Element} block @param {number} startIndex @param {boolean} active @param {boolean} moving */
  function stream(block, startIndex, active, moving) {
    if (!active || !moving) return startIndex;
    const walk = document.createTreeWalker(block, NodeFilter.SHOW_TEXT);
    const texts = [];
    for (let n = walk.nextNode(); n; n = walk.nextNode()) if (n.textContent.trim() && !n.parentElement?.closest("pre, code, svg")) texts.push(n);
    let i = startIndex;
    /** @type {HTMLSpanElement[]} */
    const wrapped = [];
    for (const n of texts) {
      const frag = document.createDocumentFragment();
      for (const p of n.textContent.split(/(\s+)/)) {
        if (!p) continue;
        if (/^\s+$/.test(p) || i >= STAGE_VIEW_WORDS_STREAMED) { frag.append(p); continue; }
        const w = document.createElement("span");
        w.className = "stream-word";
        w.style.animationDelay = `${i++ * STAGE_VIEW_WORD_STEP_MS}ms`;
        w.textContent = p;
        wrapped.push(w);
        frag.append(w);
      }
      n.replaceWith(frag);
    }
    if (wrapped.length) {
      const waitMs = (i - 1) * STAGE_VIEW_WORD_STEP_MS + STAGE_VIEW_WORD_ANIM_MS + STAGE_VIEW_WORD_UNWRAP_BUFFER_MS;
      setTimeout(() => {
        for (const w of wrapped) if (w.isConnected) w.replaceWith(document.createTextNode(w.textContent || ""));
        if (block.isConnected) block.normalize();
      }, waitMs);
    }
    return i;
  }

  return { compactThreadHistory, annotateAnswers, decisionPreview, decision, shell, stream };
})();
