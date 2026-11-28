// Unified slash workflow (/) and typed context (@) registry for chat-composer.js.
// Works in both personal chats and shared team conversations with one code path.

/**
 * @typedef {Object} ComposerTrigger
 * @property {"/" | "@"} kind
 * @property {string} query
 * @property {number} start
 * @property {number} end
 */

/**
 * @typedef {Object} ComposerChoice
 * @property {string} token
 * @property {string} label
 * @property {string} hint
 * @property {"command" | "person" | "project" | "file" | "app" | "thread"} kind
 * @property {boolean} [inline]
 * @property {boolean} [agent]
 * @property {string} [prefix]
 */

const ComposerCommands = (() => {
  const MAX_CHOICES = 10;

  /** @type {Array<{ token: string, label: string, hint: string, prefix: string }>} */
  const SLASH_WORKFLOWS = [
    { token: "/plan", label: "/plan", hint: "Draft an editable step-by-step plan before acting", prefix: "Create an editable step-by-step plan first and wait for approval before taking external actions." },
    { token: "/schedule", label: "/schedule", hint: "Turn this request into a recurring background schedule", prefix: "Set this up as a recurring background schedule and confirm cadence before activating." },
    { token: "/computer", label: "/computer", hint: "Execute inside an isolated Magnus sandbox computer", prefix: "Use the sandbox computer to complete and verify this task." },
    { token: "/research", label: "/research", hint: "Deep multi-source research with cited evidence", prefix: "Research thoroughly across connected sources and summarize with citations." },
    { token: "/review", label: "/review", hint: "Audit staged drafts and pending approvals", prefix: "Review my pending approvals and staged drafts and recommend what needs attention." },
  ];

  /**
   * Detect an active `/` or `@` trigger immediately preceding the caret.
   * @param {string} text
   * @param {number} caret
   * @returns {ComposerTrigger | null}
   */
  function triggerAt(text, caret) {
    const pos = Math.max(0, Math.min(Number(caret ?? text.length), text.length));
    const head = text.slice(0, pos);
    const m = head.match(/(?:^|\s)([/@])([\w.:-]*)$/);
    if (!m) return null;
    const raw = m[0];
    const lead = raw.startsWith("/") || raw.startsWith("@") ? 0 : 1;
    const start = pos - raw.length + lead;
    return { kind: /** @type {"/" | "@"} */ (m[1]), query: m[2] || "", start, end: pos };
  }

  /**
   * Collect live `@` context items from App state and optional conversation people.
   * @param {string} query
   * @param {{ mentions?: (q: string) => Array<{ name: string, agent?: boolean }> }} [conv]
   * @returns {ComposerChoice[]}
   */
  function contextChoices(query, conv) {
    const q = (query || "").toLowerCase();
    const matches = s => !q || String(s || "").toLowerCase().includes(q);
    /** @type {ComposerChoice[]} */
    const out = [];

    if (typeof conv?.mentions === "function") {
      for (const p of conv.mentions(query) || []) {
        out.push({
          token: `@${p.name}`,
          label: p.name,
          hint: p.agent ? "Your Magnus" : "Person",
          kind: "person",
          inline: true,
          agent: !!p.agent,
        });
      }
    }

    const app = typeof App !== "undefined" ? App : null;
    for (const p of (app?.projects || []).slice(0, 8)) {
      const name = p.name || p.title || "";
      if (name && matches(name) || matches(`project:${name}`)) {
        out.push({ token: `@project:${name}`, label: `@project:${name}`, hint: "Project", kind: "project" });
      }
    }
    for (const f of (app?.files || []).slice(0, 8)) {
      const name = f.name || f.path || "";
      if (name && (matches(name) || matches(`file:${name}`))) {
        out.push({ token: `@file:${name}`, label: `@file:${name}`, hint: "File", kind: "file" });
      }
    }
    for (const c of (app?.connectors || app?.apps || []).slice(0, 8)) {
      const name = c.title || c.name || "";
      if (name && (matches(name) || matches(`app:${name}`))) {
        out.push({ token: `@app:${name}`, label: `@app:${name}`, hint: c.state === "connected" ? "Connected app" : "App", kind: "app" });
      }
    }
    for (const t of (app?.threads || []).slice(0, 8)) {
      const name = typeof displayTitle === "function" ? displayTitle(t.title || "") : (t.title || "");
      if (name && (matches(name) || matches(`thread:${name}`))) {
        out.push({ token: `@thread:${name}`, label: `@thread:${name}`, hint: "Thread", kind: "thread" });
      }
    }
    return out.slice(0, MAX_CHOICES);
  }

  /**
   * Return matching choices for a trigger.
   * @param {ComposerTrigger | null} trig
   * @param {{ mentions?: (q: string) => Array<{ name: string, agent?: boolean }> }} [conv]
   * @returns {ComposerChoice[]}
   */
  function items(trig, conv) {
    if (!trig) return [];
    const q = (trig.query || "").toLowerCase();
    if (trig.kind === "/") {
      return SLASH_WORKFLOWS
        .filter(w => !q || w.token.slice(1).includes(q) || w.hint.toLowerCase().includes(q))
        .map(w => ({ token: w.token, label: w.label, hint: w.hint, kind: "command", prefix: w.prefix }))
        .slice(0, MAX_CHOICES);
    }
    return contextChoices(trig.query, conv);
  }

  /**
   * Remove the trigger substring from text, inserting an inline token if requested.
   * @param {string} text
   * @param {ComposerTrigger} trig
   * @param {ComposerChoice} choice
   * @returns {{ text: string, caret: number, chip: ComposerChoice | null }}
   */
  function apply(text, trig, choice) {
    const before = text.slice(0, trig.start);
    const after = text.slice(trig.end);
    if (choice.inline) {
      const inserted = `${choice.token} `;
      return { text: before + inserted + after, caret: before.length + inserted.length, chip: null };
    }
    const cleanAfter = after.startsWith(" ") ? after.slice(1) : after;
    return { text: before + cleanAfter, caret: before.length, chip: choice };
  }

  /**
   * Serialize attached workflow/context chips with the user's message text.
   * @param {string} text
   * @param {ComposerChoice[]} chips
   * @returns {string}
   */
  function composePayload(text, chips) {
    const clean = String(text || "").trim();
    if (!chips || !chips.length) return clean;
    const workflows = chips.filter(c => c.kind === "command");
    const contexts = chips.filter(c => c.kind !== "command");
    const lines = [];
    if (workflows.length) lines.push(workflows.map(w => `[${w.token}] ${w.prefix || ""}`.trim()).join("\n"));
    if (contexts.length) lines.push(`Context: ${contexts.map(c => c.token).join(", ")}`);
    if (clean) lines.push(clean);
    return lines.join("\n\n");
  }

  return { SLASH_WORKFLOWS, triggerAt, items, apply, composePayload };
})();
