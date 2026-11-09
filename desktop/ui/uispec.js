// Compiles the line-based specs the agent writes into the components Compose
// draws. The catalog comes from the appliance, so there is one definition of
// what each element is; unknown elements are refused, not guessed. Specs
// arrive as text streams, so what is written so far is drawn line by line.
"use strict";

const UiSpec = (() => {
  let catalog = null;
  const setCatalog = c => { catalog = c; };
  const named = name => catalog?.elements.find(e => e.name === name);

  function wordsOf(line) {
    const out = []; let i = 0;
    while (true) {
      while (line[i] === " " || line[i] === "\t") i++;
      if (i >= line.length) return out;
      let key = null, parts = [], current = "", quoted = false, flags = [];
      while (i < line.length && line[i] !== " " && line[i] !== "\t") {
        const c = line[i];
        if (c === '"') {
          i++; quoted = true;
          for (;;) {
            if (i >= line.length) throw new Error(`a quote is not closed in: ${line}`);
            const q = line[i++];
            if (q === '"') break;
            if (q === "\\") { if (i >= line.length) throw new Error(`a quote is not closed in: ${line}`); const e = line[i++]; current += e === "n" ? "\n" : e; }
            else current += q;
          }
        } else if (c === "=" && key === null && !quoted && current && !parts.length) { i++; key = current; current = ""; }
        else if (c === "," && (key !== null || parts.length || current)) { i++; parts.push(current); flags.push(quoted); current = ""; quoted = false; }
        else { i++; current += c; }
      }
      parts.push(current); flags.push(quoted);
      out.push({ key, parts, quoted: flags });
    }
  }
  const toNumber = (raw, what) => {
    const t = String(raw).trim(), n = t === "" ? NaN : Number(t);
    if (!Number.isFinite(n)) throw new Error(`${what} must be a number, not ${JSON.stringify(raw)}`);
    return n;
  };
  const loose = (raw, quoted) => { const t = String(raw).trim(), n = t === "" ? NaN : Number(t); return !quoted && Number.isFinite(n) ? n : raw; };

  function fill(shape, words, into) {
    const lead = [];
    for (const w of words) {
      if (w.key === null) {
        const bare = w.parts.join(",");
        if (!w.quoted.some(Boolean) && shape.flags.includes(bare)) into[bare] = true;
        else if (shape.lead) lead.push(bare);
        else throw new Error(`${JSON.stringify(bare)} needs a name, as key=value`);
        continue;
      }
      const k = w.key;
      if (shape.number_lists.includes(k)) into[k] = w.parts.map(p => toNumber(p, k));
      else if (shape.lists.includes(k)) into[k] = w.parts.map((p, n) => loose(p, w.quoted[n]));
      else if (shape.numbers.includes(k)) into[k] = toNumber(w.parts.join(","), k);
      else if (shape.loose.includes(k)) into[k] = loose(w.parts.join(","), w.quoted.some(Boolean));
      else into[k] = w.parts.join(",");
    }
    if (shape.lead && lead.length) into[shape.lead] = lead.join(" ");
  }

  function childValue(child, rest, words) {
    if (child.takes === "text") { const t = rest.trim(); return t.length >= 2 && t.startsWith('"') && t.endsWith('"') ? t.slice(1, -1) : t; }
    if (child.takes === "row") return wordsOf(rest).flatMap(w => w.parts.map((p, n) => loose(p, w.quoted[n])));
    const o = {}; fill(child.shape, words, o); return o;
  }

  // Throws with why, as the appliance's compiler does.
  function compile(text) {
    if (!catalog) throw new Error("the catalog has not loaded");
    const out = []; let open = null;
    for (const raw of String(text).split("\n")) {
      const line = raw.trimEnd();
      if (!line.trim() || line.trimStart().startsWith("#")) continue;
      const trimmed = line.trimStart(), cut = trimmed.search(/[ \t]/);
      const first = cut < 0 ? trimmed : trimmed.slice(0, cut), rest = cut < 0 ? "" : trimmed.slice(cut + 1);
      if (line.length !== trimmed.length) {
        if (!open) throw new Error(`nothing to indent under: ${trimmed}`);
        if (!open.child) throw new Error(`${open.name} takes no indented lines`);
        if (first !== open.child.line) throw new Error(`${open.name} takes \`${open.child.line}\` lines, not ${JSON.stringify(first)}`);
        out[out.length - 1][open.child.into].push(childValue(open.child, rest, wordsOf(rest)));
        continue;
      }
      const element = named(first);
      if (!element) throw new Error(`unknown element ${JSON.stringify(first)}: use one of ${catalog.elements.map(e => e.name).join(", ")}`);
      const c = { type: element.name };
      fill(element.shape, wordsOf(rest), c);
      if (element.child) c[element.child.into] = [];
      out.push(c);
      open = element;
    }
    if (!out.length) throw new Error("the spec is empty");
    return out;
  }

  // What can be drawn of a spec still being written: the lines finished so far.
  function compilePartial(text) {
    const lines = String(text).split("\n");
    lines.pop();
    while (lines.length) {
      try { return compile(lines.join("\n")); } catch { lines.pop(); }
    }
    return [];
  }

  const FENCE = /^(`{3,})(magnus-(ui|html))(?:\s+(?:id=)?(\S+))?\s*$/;
  // The text in order: prose, and each block with whether it has closed.
  function split(text) {
    const parts = []; let prose = [], open = null;
    const flush = () => { if (prose.length) parts.push({ kind: "text", text: prose.join("\n") }); prose = []; };
    for (const line of String(text ?? "").replace(/\r\n?/g, "\n").split("\n")) {
      if (open) {
        if (line.trim() === open.fence) { parts.push({ kind: open.kind, id: open.id, body: open.body.join("\n") + "\n", closed: true }); open = null; }
        else open.body.push(line);
        continue;
      }
      const m = line.match(FENCE);
      if (m) { flush(); open = { fence: m[1], kind: m[3], id: m[4], body: [] }; }
      else prose.push(line);
    }
    flush();
    if (open) parts.push({ kind: open.kind, id: open.id, body: open.body.join("\n"), closed: false });
    return parts;
  }

  return { setCatalog, compile, compilePartial, split, get catalog() { return catalog; } };
})();
if (typeof module !== "undefined") module.exports = UiSpec;
