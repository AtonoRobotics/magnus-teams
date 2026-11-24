// Draws an outcome the agent composed from the fixed components (U8). The
// agent chooses which components and what they say; this code decides how
// they look. The result is a document for a sandboxed frame that may not run
// scripts, so nothing the agent wrote can execute.
"use strict";

const Compose = (() => {
  const esc = s => String(s ?? "").replace(/[&<>"']/g, c => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
  // A secret the server hid when someone typed it into chat (spec, Typed
  // secrets) shows as a masked value, with what it was for screen readers.
  /** @param {string} s @returns {string} HTML */
  const maskedText = s => esc(s).replace(/\[hidden ([A-Za-z ]+?)(?: ending (\d{4}))?\]/g, (_, what, last) => `<span class="masked-secret" title="Hidden ${what}: it is not kept, and Magnus never sees it">••••${last ? ` ${last}` : ""}<span class="sr"> (hidden ${what})</span></span>`);
  const fmt = v => typeof v === "number" ? (Math.abs(v) >= 1000 ? v.toLocaleString() : String(Math.round(v * 1000) / 1000)) : esc(v);

  function frame(c, inner) {
    const title = c.title ? `<h3>${esc(c.title)}</h3>` : "";
    const cap = c.caption ? `<p class="cap">${esc(c.caption)}</p>` : "";
    return `<figure>${title}${inner}${cap}</figure>`;
  }

  function bar(c) {
    const vals = c.values.map(Number), max = Math.max(0, ...vals), min = Math.min(0, ...vals);
    const span = max - min || 1, rowH = 28, labelW = 120, w = 560, plotW = w - labelW - 70;
    const zero = labelW + ((0 - min) / span) * plotW;
    const rows = c.labels.map((l, i) => {
      const v = vals[i], x0 = labelW + ((Math.min(0, v) - min) / span) * plotW, bw = (Math.abs(v) / span) * plotW, y = i * rowH;
      const tip = `${l}: ${fmt(v)}${c.unit ? " " + c.unit : ""}`;
      return `<g class="chart-row" data-chart-tip="${esc(tip)}"><text x="${labelW - 8}" y="${y + 18}" text-anchor="end" class="lbl">${esc(l)}</text>
        <rect x="${x0}" y="${y + 5}" width="${Math.max(bw, 1)}" height="${rowH - 10}" rx="3" class="bar"/>
        <text x="${x0 + Math.max(bw, 1) + 6}" y="${y + 18}" class="val">${fmt(v)}${c.unit ? " " + esc(c.unit) : ""}</text></g>`;
    }).join("");
    const h = c.labels.length * rowH;
    return frame(c, `<svg viewBox="0 0 ${w} ${h + 4}" role="img" aria-label="${esc(c.title || "bar chart")}"><line x1="${zero}" y1="0" x2="${zero}" y2="${h}" class="axis"/>${rows}</svg>`);
  }

  function line(c) {
    const all = c.series.flatMap(s => s.values.map(Number)), max = Math.max(...all), min = Math.min(...all);
    const span = max - min || 1, w = 560, h = 220, l = 48, r = 12, t = 14, b = 30;
    const x = i => l + (c.labels.length === 1 ? 0 : (i / (c.labels.length - 1)) * (w - l - r));
    const y = v => t + (1 - (v - min) / span) * (h - t - b);
    const ticks = [min, (min + max) / 2, max].map(v => `<text x="${l - 6}" y="${y(v) + 4}" text-anchor="end" class="val">${fmt(v)}</text><line x1="${l}" x2="${w - r}" y1="${y(v)}" y2="${y(v)}" class="grid"/>`).join("");
    const step = Math.ceil(c.labels.length / 8);
    const xl = c.labels.map((lb, i) => i % step === 0 ? `<text x="${x(i)}" y="${h - 10}" text-anchor="middle" class="lbl">${esc(lb)}</text>` : "").join("");
    const paths = c.series.map((s, k) => `<polyline class="ln s${k}" points="${s.values.map((v, i) => `${x(i)},${y(Number(v))}`).join(" ")}"/>`).join("");
    const dots = c.series.map((s, k) => s.values.map((v, i) => {
      const tip = `${s.name} · ${c.labels[i] ?? i}: ${fmt(Number(v))}${c.unit ? " " + c.unit : ""}`;
      return `<circle cx="${x(i)}" cy="${y(Number(v))}" r="4.5" class="pt s${k}" data-chart-tip="${esc(tip)}"/>`;
    }).join("")).join("");
    const legend = c.series.length > 1 ? `<p class="legend">${c.series.map((s, k) => `<span class="key s${k}"></span>${esc(s.name)}`).join(" ")}</p>` : "";
    return frame(c, `<svg viewBox="0 0 ${w} ${h}" role="img" aria-label="${esc(c.title || "line chart")}">${ticks}${xl}${paths}${dots}</svg>${legend}${c.unit ? `<p class="cap">Values in ${esc(c.unit)}</p>` : ""}`);
  }

  // Places by latitude and longitude on a plain projection. The frame has no
  // network, so there are no map tiles beneath them.
  function map(c) {
    const ps = c.places.map(p => ({ ...p, lat: Number(p.lat), lon: Number(p.lon) }));
    const lats = ps.map(p => p.lat), lons = ps.map(p => p.lon);
    const mid = (Math.min(...lats) + Math.max(...lats)) / 2, k = Math.max(Math.cos(mid * Math.PI / 180), 0.2);
    const pad = d => Math.max(d * 0.15, 0.5);
    const [x0, x1] = [Math.min(...lons), Math.max(...lons)], [y0, y1] = [Math.min(...lats), Math.max(...lats)];
    const [lo0, lo1, la0, la1] = [x0 - pad(x1 - x0), x1 + pad(x1 - x0), y0 - pad(y1 - y0), y1 + pad(y1 - y0)];
    const w = 560, sx = w / ((lo1 - lo0) * k), h = Math.min(Math.max((la1 - la0) * sx, 160), 420), sy = h / (la1 - la0);
    const x = lon => (lon - lo0) * k * sx, y = lat => (la1 - lat) * sy;
    const dots = ps.map(p => `<circle cx="${x(p.lon)}" cy="${y(p.lat)}" r="5" class="pt"/><text x="${x(p.lon) + 8}" y="${y(p.lat) + 4}" class="lbl">${esc(p.name)}</text>`).join("");
    const corner = (lat, lon) => `${Math.abs(lat).toFixed(1)}°${lat < 0 ? "S" : "N"} ${Math.abs(lon).toFixed(1)}°${lon < 0 ? "W" : "E"}`;
    const notes = ps.filter(p => p.note).map(p => `<li><b>${esc(p.name)}</b>: ${esc(p.note)}</li>`).join("");
    return frame(c, `<svg viewBox="-2 -2 ${w + 4} ${h + 22}" role="img" aria-label="${esc(c.title || "map")}: ${esc(ps.map(p => p.name).join(", "))}">
      <rect x="0" y="0" width="${w}" height="${h}" class="area"/>${dots}
      <text x="0" y="${h + 16}" class="val">${corner(la0, lo0)}</text><text x="${w}" y="${h + 16}" text-anchor="end" class="val">${corner(la1, lo1)}</text></svg>${notes ? `<ul>${notes}</ul>` : ""}`);
  }

  // One row per event: when (its first part strong, the rest under it, as
  // "Day 1 · Sat"), what, and its note.
  function timeline(c) {
    const when = w => { const [a, ...b] = String(w ?? "").split(" · "); return `${esc(a)}${b.length ? `<span class="sub">${esc(b.join(" · "))}</span>` : ""}`; };
    return frame(c, `<ol class="tl">${c.events.map(e => `<li><span class="when">${when(e.when)}</span><span class="what"><span class="lbl">${esc(e.label)}</span>${e.note ? `<span class="note">${esc(e.note)}</span>` : ""}</span></li>`).join("")}</ol>`);
  }

  // Options side by side, one row per aspect; the option that wins an aspect
  // is marked.
  function comparison(c) {
    const head = `<tr><th></th>${c.options.map(o => `<th>${esc(o)}</th>`).join("")}</tr>`;
    const rows = c.aspects.map(a => `<tr><th scope="row">${esc(a.name)}</th>${a.values.map((v, i) => `<td class="${typeof v === "number" ? "num" : ""}${a.best === i ? " best" : ""}">${fmt(v)}${a.best === i ? ` <span class="sr">(best)</span>` : ""}</td>`).join("")}</tr>`).join("");
    return frame(c, `<div class="tw"><table class="cmp"><thead>${head}</thead><tbody>${rows}</tbody></table></div>`);
  }

  // A delivered file shown in full, drawn from the same components: Markdown
  // as headings, paragraphs, lists and tables; CSV as a table; other text as
  // text. `limit` is the appliance's ({chars, rows}); the rest is in the file.
  function table(columns, rows, limit) {
    const t = { type: "table", columns: columns.map(c => c || " "), rows: rows.slice(0, limit.rows).map(r => columns.map((_, k) => r[k] ?? "")) };
    if (rows.length > limit.rows) t.caption = `The first ${limit.rows} of ${rows.length} rows; the rest are in the file.`;
    return t;
  }

  // A conservative Markdown renderer shared by conversation and file previews.
  // Only our generated tags enter the DOM; source HTML is always escaped.
  function safeUrl(value) {
    try {
      let u;
      try { u=new URL(value); } catch { u=new URL(value,location.origin); }
      return ["http:","https:"].includes(u.protocol) ? u.href : null;
    } catch { return null; }
  }
  function localImagePath(raw) {
    try {
      let url;
      if(raw.startsWith("/v1/files/"))url=new URL(raw,"https://appliance.invalid");
      else {const safe=safeUrl(raw);if(!safe)return null;url=new URL(safe);if(url.origin!==location.origin)return null;}
      return /^\/v1\/files\/[^/?#]+$/.test(url.pathname) ? url.pathname : null;
    } catch {return null;}
  }
  function codeBlock(text, language = "") {
    const highlighted = typeof RichText !== "undefined" ? RichText.highlightCode(text, language) : esc(text);
    return `<div class="code-block"><div class="code-head"><span>${esc(language)}</span><button type="button" data-code-copy aria-label="Copy code" data-tooltip="Copy code">Copy code</button></div><pre class="code"><code>${highlighted}</code></pre></div>`;
  }
  const FILE_EXT = {
    js: ["js", "JS"], mjs: ["js", "JS"], ts: ["ts", "TS"], tsx: ["ts", "TS"], jsx: ["js", "JS"],
    html: ["html", "<>"], htm: ["html", "<>"], rs: ["rs", "RS"], css: ["css", "{}"],
    md: ["md", "M↓"], json: ["json", "{}"], py: ["py", "PY"], sh: ["sh", ">_"],
    yml: ["yml", "YML"], yaml: ["yml", "YML"], toml: ["toml", "TOML"], sql: ["sql", "SQL"],
    pdf: ["pdf", "PDF"], pptx: ["pptx", "PPTX"], ppt: ["pptx", "PPT"],
    docx: ["docx", "DOC"], doc: ["docx", "DOC"], xlsx: ["xlsx", "XLS"], xls: ["xlsx", "XLS"], csv: ["csv", "CSV"],
  };
  function inlineCode(raw) {
    const clean = raw.replace(/\n/g, " ");
    const fm = !raw.includes("\n") && !raw.includes(" ") && raw.match(/^(?:[\w./-]+\/)?([\w.-]+\.([a-z0-9]{1,5}))$/i);
    const meta = fm && FILE_EXT[fm[2].toLowerCase()];
    if (meta) return `<code class="fchip-inline" data-ext="${meta[0]}"><span class="fchip-tag" aria-hidden="true">${meta[1]}</span>${esc(clean)}</code>`;
    return `<code>${esc(clean)}</code>`;
  }
  function markdownInline(text, depth = 0) {
    if (depth > 8) return maskedText(text);
    const pattern = /(`+)([\s\S]*?)\1|(!?)\[([^\]\n]*)\]\(([^\s]*?)(?:\s+"[^"\n]*")?\)|\*\*([^*\n]+)\*\*|__([^_\n]+)__|~~([^~\n]+)~~|\*([^*\n]+)\*|_([^_\n]+)_|<(https?:\/\/[^>\s]+)>|\[(\d{1,2})\](?!\()/g;
    let out = "", at = 0;
    for (const m of text.matchAll(pattern)) {
      out += maskedText(text.slice(at, m.index)); at = m.index + m[0].length;
      if (m[1]) out += inlineCode(m[2]);
      else if (m[4] !== undefined || m[11]) {
        const label = m[4] ?? m[11], raw = m[5] ?? m[11], url = safeUrl(raw);
        const localImage = m[3] && localImagePath(raw);
        if (localImage) out += `<img data-prose-image="${esc(localImage)}" alt="${esc(label)}" loading="lazy">`;
        else out += url ? `<a href="${esc(url)}" data-prose-link rel="noopener noreferrer">${markdownInline(label, depth + 1)}</a>` : esc(label);
      } else if (m[6] || m[7]) out += `<strong>${markdownInline(m[6] || m[7], depth + 1)}</strong>`;
      else if (m[8]) out += `<del>${markdownInline(m[8], depth + 1)}</del>`;
      else if (m[9] || m[10]) out += `<em>${markdownInline(m[9] || m[10], depth + 1)}</em>`;
      else if (m[12]) out += `<sup class="cite-pill">${esc(m[12])}</sup>`;
    }
    return out + maskedText(text.slice(at));
  }
  function renderList(lines, startIdx) {
    const parseItem = line => line.match(/^(\s*)([-*+]|\d+[.)])\s+(.*)$/);
    const first = parseItem(lines[startIdx]);
    if (!first) return ["", startIdx];
    const baseIndent = first[1].length, ordered = /\d/.test(first[2]);
    const tag = ordered ? "ol" : "ul", startAttr = ordered && parseInt(first[2], 10) !== 1 ? ` start="${parseInt(first[2], 10)}"` : "";
    const items = [];
    let i = startIdx;
    while (i < lines.length) {
      const m = parseItem(lines[i]);
      if (!m || m[1].length < baseIndent) break;
      if (m[1].length > baseIndent) {
        const [nested, nextI] = renderList(lines, i);
        if (items.length) items[items.length - 1] = items[items.length - 1].replace(/<\/li>$/, `${nested}</li>`);
        i = nextI;
        continue;
      }
      if (/\d/.test(m[2]) !== ordered) break;
      i++;
      const chk = !ordered && m[3].match(/^\[([ xX])\]\s+(.*)$/);
      const parts = [chk ? chk[2] : m[3]];
      while (i < lines.length && lines[i].trim() && !parseItem(lines[i]) && lines[i].match(/^(\s*)/)[1].length > baseIndent) {
        parts.push(lines[i++].trim());
      }
      const inner = markdownInline(parts.join("\n"));
      items.push(chk ? `<li class="task-item"><input type="checkbox" disabled${chk[1].toLowerCase() === "x" ? " checked" : ""}><span>${inner}</span></li>` : `<li>${inner}</li>`);
    }
    return [`<${tag}${startAttr}>${items.join("")}</${tag}>`, i];
  }
  function markdown(source, limit) {
    const maxRows = Number.isFinite(limit?.rows) ? Math.max(0, Math.floor(limit.rows)) : Infinity;
    const lines = String(source || "").replace(/\r\n?/g, "\n").split("\n"), out = [];
    const rawCells = line => line.trim().replace(/^\|/, "").replace(/\|$/, "").split(/(?<!\\)\|/).map(c => c.trim().replace(/\\\|/g, "|"));
    const isNum = s => /^[-+]?[$€£]?[\d,.]+[KMBkmb%]?$/.test(s.trim());
    const tableRule = /^\s*\|?\s*:?-{3,}:?\s*(?:\|\s*:?-{3,}:?\s*)*\|?\s*$/;
    const block = line => /^\s*$|^\s{0,3}(?:#{1,6}\s|>|[-*+]\s|\d+[.)]\s|`{3,}|~{3,})/.test(line);
    for (let i = 0; i < lines.length;) {
      const line = lines[i]; let m;
      if (!line.trim()) { i++; continue; }
      if ((m = line.match(/^\s{0,3}(`{3,}|~{3,})([^\s]*)(?:\s+id=\S+)?\s*$/))) {
        const fence = m[1], lang = m[2], body = [];
        for (i++; i < lines.length && !new RegExp(`^\\s{0,3}${fence[0]}{${fence.length},}\\s*$`).test(lines[i]); i++) body.push(lines[i]);
        const closed = i < lines.length;
        if (closed) i++;
        out.push(/^magnus-(ui|html)$/.test(lang) ? `<div class="ui-block" data-ui-kind="${lang.slice(7)}" data-ui-closed="${closed ? 1 : 0}" data-ui-body="${esc(body.join("\n"))}"></div>` : codeBlock(body.join("\n"), lang));
      } else if ((m = line.match(/^\s{0,3}(#{1,6})\s+(.*?)\s*#*\s*$/))) { out.push(`<h${m[1].length}>${markdownInline(m[2])}</h${m[1].length}>`); i++; }
      else if (/^\s{0,3}([-*_])(\s*\1){2,}\s*$/.test(line)) { out.push("<hr>"); i++; }
      else if (line.includes("|") && tableRule.test(lines[i + 1] || "")) {
        const headRaw = rawCells(line), head = headRaw.map(c => markdownInline(c)), rows = []; let total = 0; i += 2;
        while (i < lines.length && lines[i].trim() && lines[i].includes("|")) { if (rows.length < maxRows) rows.push(rawCells(lines[i])); total++; i++; }
        const ths = head.map((c, idx) => `<th scope="col" data-sort-col="${idx}" aria-sort="none">${c}</th>`).join("");
        const trs = rows.map(row => `<tr>${head.map((_, n) => { const raw = row[n] || ""; return `<td${n > 0 && isNum(raw) ? ' class="num"' : ""}>${markdownInline(raw)}</td>`; }).join("")}</tr>`).join("");
        out.push(`<div class="markdown-table" tabindex="0" role="region" aria-label="Table"><table><thead><tr>${ths}</tr></thead><tbody>${trs}</tbody></table><div class="table-bar">${total > rows.length ? `<span class="cap">The first ${rows.length} of ${total} rows; the rest are in the file.</span>` : "<span></span>"}<button type="button" class="table-csv-btn" data-table-csv>Copy CSV</button></div></div>`);
      } else if (/^\s{0,3}>/.test(line)) {
        const quote = []; while (i < lines.length && /^\s{0,3}>/.test(lines[i])) quote.push(lines[i++].replace(/^\s{0,3}>\s?/, ""));
        const callout = quote[0]?.match(/^\[!(NOTE|TIP|IMPORTANT|WARNING|CAUTION)\]\s*(.*)$/i);
        if (callout) {
          const kind = callout[1].toLowerCase();
          const label = callout[2] || (kind[0].toUpperCase() + kind.slice(1));
          const rest = quote.slice(1).map(x => markdownInline(x)).join("<br>");
          out.push(`<div class="md-callout tone-${esc(kind)}"><div class="md-callout-title">${esc(label)}</div>${rest ? `<div class="md-callout-body">${rest}</div>` : ""}</div>`);
        } else {
          out.push(`<blockquote>${quote.map(x => markdownInline(x)).join("<br>")}</blockquote>`);
        }
      } else if (/^\s*([-*+]|\d+[.)])\s+/.test(line)) {
        const [listHtml, nextI] = renderList(lines, i);
        out.push(listHtml);
        i = nextI;
      } else {
        const parts = [line]; i++;
        while (i < lines.length && !block(lines[i]) && !(lines[i].includes("|") && tableRule.test(lines[i + 1] || ""))) parts.push(lines[i++]);
        out.push(`<p>${markdownInline(parts.join("\n"))}</p>`);
      }
    }
    return `<div class="markdown">${out.join("\n")}</div>`;
  }
  function fromMarkdown(md, limit) {
    return [{ type: "markdown", text: md.slice(0, limit.chars) }];
  }

  function fromCsv(text, limit) {
    const rows = [];
    let row = [], cell = "", quoted = false;
    for (let i = 0; i < text.length; i++) {
      const c = text[i];
      if (quoted) {
        if (c !== '"') cell += c;
        else if (text[i + 1] === '"') { cell += '"'; i++; }
        else quoted = false;
      } else if (c === '"') quoted = true;
      else if (c === ",") { row.push(cell); cell = ""; }
      else if (c === "\n" || c === "\r") { if (c === "\r" && text[i + 1] === "\n") i++; row.push(cell); rows.push(row); row = []; cell = ""; }
      else cell += c;
    }
    if (cell || row.length) { row.push(cell); rows.push(row); }
    const [columns, ...body] = rows.filter(r => r.some(x => x.trim()));
    return columns ? [table(columns, body, limit)] : [];
  }

  const TEXT = /^(text\/|application\/(json|xml|csv|x-ndjson|yaml))/;
  function fileParts(name, f, limit) {
    if (f?.image) return [{ type: "image", file: name }];
    if (typeof f?.text !== "string") return [{ type: "text", text: `${name} cannot be shown here; download it from Files.` }];
    const cut = f.text.length > limit.chars, text = cut ? f.text.slice(0, limit.chars) : f.text;
    const md = /\.(md|markdown)$/i.test(name) || f.media_type === "text/markdown";
    const csv = /\.csv$/i.test(name) || /csv/.test(f.media_type);
    const parts = md ? fromMarkdown(text, limit) : csv ? fromCsv(text, limit) : text.trim() ? [{ type: "text", text }] : [];
    if (cut) parts.push({ type: "text", text: "The file goes on; the rest is in the file.", muted: true });
    return parts;
  }

  function slide(s) {
    const items = s.items?.map(i => `<li>${esc(i)}</li>`).join("") || "";
    const body = s.layout === "bullets" ? `<ul>${items}</ul>` : s.layout === "quote" ? `<blockquote>${esc(s.text)}</blockquote><p class="cap">${esc(s.subtitle)}</p>` : s.layout === "title" ? `<p>${esc(s.subtitle)}</p>` : s.layout === "image" ? `<p class="cap">${esc(s.file)}</p>` : `<p>${esc(s.text)}</p>`;
    return `<section class="slide ${esc(s.layout)}">${s.title ? `<h2>${esc(s.title)}</h2>` : ""}${body}</section>`;
  }

  function field(f) {
    const label = esc(f.label || f.name), name = esc(f.name), req = f.required ? " required" : "";
    const input = f.type === "textarea" ? `<textarea name="${name}"${req}></textarea>`
      : f.type === "select" ? `<select name="${name}"${req}>${(f.options || []).map(o => `<option>${esc(o)}</option>`).join("")}</select>`
      : `<input name="${name}" type="${esc(f.type || "text")}"${req}>`;
    return `<label>${label}${input}</label>`;
  }

  function part(c, files, limit) {
    switch (c.type) {
      case "deck": return `<div class="deck" data-deck><div class="deck-bar"><span class="fname">${esc(c.title)}</span><button type="button" class="deck-present">Present</button></div>${c.slides.map(slide).join("")}</div>`;
      case "card": return frame({ title: c.title }, `<p>${esc(c.text)}</p>`);
      case "gallery": return frame(c, `<div class="gallery">${c.items.map(g => { const f = files[g.file]; return `<div>${f?.image ? `<img src="${esc(f.image)}" alt="${esc(g.caption || g.file)}">` : `<p class="cap">${esc(g.file)}</p>`}${g.caption ? `<p class="cap">${esc(g.caption)}</p>` : ""}</div>`; }).join("")}</div>`);
      case "form": return frame(c, `<div class="uiform">${c.fields.map(field).join("")}<button type="button" class="action form-send">${esc(c.submit)}</button></div>`);
      case "heading": return c.level > 2 ? `<h4>${esc(c.text)}</h4>` : `<h2>${esc(c.text)}</h2>`;
      case "markdown": return markdown(c.text,limit);
      case "text": return `<p${c.muted ? ` class="cap"` : ""}>${esc(c.text)}</p>`;
      case "list": return frame(c, c.ordered ? `<ol>${c.items.map(i => `<li>${esc(i)}</li>`).join("")}</ol>` : `<ul>${c.items.map(i => `<li>${esc(i)}</li>`).join("")}</ul>`);
      case "table": {
        const head = c.columns.some(h => String(h).trim()) ? `<thead><tr>${c.columns.map((h, idx) => `<th data-sort-col="${idx}" aria-sort="none">${esc(h)}</th>`).join("")}</tr></thead>` : "";
        const cell = (v, i) => `<td class="${typeof v === "number" || (i > 0 && /^[-+]?[$€£]?[\d,.]+%?$/.test(String(v).trim())) ? "num" : ""}">${fmt(v)}</td>`;
        return frame(c, `<div class="tw"><table>${head}<tbody>${c.rows.map(r => `<tr${/^total\b/i.test(String(r[0])) ? ` class="total"` : ""}>${r.map(cell).join("")}</tr>`).join("")}</tbody></table></div><div class="table-bar"><span></span><button type="button" class="table-csv-btn" data-table-csv>Copy CSV</button></div>`);
      }
      case "stat": {
        const delta = c.delta ? `<span class="stat-delta ${String(c.delta).trim().startsWith("-") ? "down" : "up"}">${esc(c.delta)}</span>` : "";
        return `<div class="stat"><div class="sv">${fmt(c.value)}${c.unit ? `<span class="su"> ${esc(c.unit)}</span>` : ""}${delta}</div><div class="sl">${esc(c.label)}</div>${c.caption ? `<p class="cap">${esc(c.caption)}</p>` : ""}</div>`;
      }
      case "callout": {
        const tone = esc((c.tone || "note").toLowerCase());
        return `<div class="md-callout tone-${tone}">${c.title ? `<div class="md-callout-title">${esc(c.title)}</div>` : ""}<div class="md-callout-body">${markdownInline(c.text || "")}</div></div>`;
      }
      case "sources": {
        const pills = (c.items || []).map((s, idx) => {
          const u = safeUrl(s.url);
          let dom = s.domain || "";
          if (!dom && u) { try { dom = new URL(u).hostname.replace(/^www\./, ""); } catch { /* ignore */ } }
          const inner = `<span class="source-idx">${idx + 1}</span><span class="source-title">${esc(s.title || dom || s.url || "")}</span>${dom ? `<span class="source-dom">${esc(dom)}</span>` : ""}`;
          return u ? `<a class="source-pill" href="${esc(u)}" data-prose-link rel="noopener noreferrer">${inner}</a>` : `<span class="source-pill">${inner}</span>`;
        }).join("");
        return `<div class="ui-sources">${c.title ? `<div class="ui-sources-head">${esc(c.title)}</div>` : ""}<div class="ui-sources-list">${pills}</div></div>`;
      }
      case "bar_chart": return bar(c);
      case "line_chart": return line(c);
      case "map": return map(c);
      case "timeline": return timeline(c);
      case "comparison": return comparison(c);
      case "action": return `<p><button type="button" class="action">${esc(c.label)}</button></p>`;
      case "code": return frame(c, `<div class="markdown">${codeBlock(c.code,c.language)}</div>`);
      case "app": return "";
      case "image": if (c.placeholder && !files[c.placeholder]?.image) return frame(c, `<div class="placeholder">${files[c.placeholder]?.failed ? "This image could not be made." : "Making this image…"}</div>`);
        if (c.placeholder) c = { ...c, file: c.placeholder };
        return files[c.file]?.image ? frame(c, `<img src="${files[c.file].image}" alt="${esc(c.caption || c.file)}">`) : frame(c, `<p class="cap">${esc(c.file)} (not an image this app can show)</p>`);
      case "file": return `<section class="file" data-file="${esc(c.file)}"><p class="fname">${esc(c.file)}</p>${fileParts(c.file, files[c.file], limit).map(x => part(x, files, limit)).join("\n")}</section>`;
      default: return "";
    }
  }

  /** Render validated UiSpec components into HTML, auto-grouping consecutive stats into a KPI strip. */
  function renderParts(view, files, limit) {
    const out = [];
    for (let i = 0; i < (view || []).length; ) {
      if (view[i].type === "stat") {
        const group = [];
        while (i < view.length && view[i].type === "stat") group.push(part(view[i++], files, limit));
        out.push(`<div class="stat-grid">${group.join("")}</div>`);
      } else {
        out.push(part(view[i++], files, limit));
      }
    }
    return out.join("\n");
  }

  /** Render validated UiSpec components directly for native DOM insertion (zero iframe overhead). */
  function renderNative(view, files, limit) {
    return `<div class="ui-native">${renderParts(view, files || {}, limit || { chars: 160000, rows: 200 })}</div>`;
  }

  // Colors come from the app's look (theme.js tokens), so a result reads the
  // same as the cards around it; without them, a light and a dark default.
  const VARS = ["fg", "fg2", "muted", "line", "hair", "surface", "raised", "shadow", "accent", "accent-text", "r-sm", "r-md", "r-lg"];
  const DEFAULT_LIGHT = { fg: "#111318", fg2: "#4B5563", muted: "#565E6B", line: "#E5E7EB", hair: "#EDF0F4", surface: "#FFFFFF", raised: "#EEF1F6", shadow: "0 1px 3px rgba(15,23,42,0.04), 0 8px 24px rgba(15,23,42,0.06)", accent: "#111318", "accent-text": "#111318", "r-sm": "6px", "r-md": "8px", "r-lg": "10px" };
  const vars = t => VARS.map(k => `--${k}:${(t || DEFAULT_LIGHT)[k] ?? DEFAULT_LIGHT[k]};`).join("");
  const markdownStyles = `.markdown { font-size: 15px; line-height: 1.55; overflow-wrap: anywhere; min-width: 0; }
.markdown p { margin: 8px 0; white-space: pre-wrap; }
.markdown p:first-child { margin-top: 0; }
.markdown p:last-child { margin-bottom: 0; }
.markdown h1, .markdown h2, .markdown h3, .markdown h4 { color: var(--fg); margin: 16px 0 8px; line-height: 1.3; }
.markdown h1 { font-size: 22px; } .markdown h2 { font-size: 18px; } .markdown h3 { font-size: 16px; }
.markdown a { color: var(--accent-text); text-decoration: underline; text-underline-offset: 2px; }
.markdown ul, .markdown ol { padding-left: 22px; margin: 6px 0; }
.markdown li { margin: 3px 0; }
.markdown li.task-item { list-style: none; margin-left: -18px; display: flex; align-items: baseline; gap: 7px; }
.markdown li.task-item input[type=checkbox] { margin: 0; accent-color: var(--accent); }
.markdown blockquote { border-left: 3px solid var(--accent); background: color-mix(in srgb, var(--raised) 55%, transparent); border-radius: 0 var(--r-sm, 6px) var(--r-sm, 6px) 0; margin: 10px 0; padding: 8px 14px; color: var(--fg2); }
.markdown code { font-family: var(--mono, monospace); font-size: 13.5px; }
.markdown :not(pre) > code { background: var(--raised); border-radius: 4px; padding: 2px 5px; }
.markdown .code-block { background: var(--raised); border: 1px solid var(--line); border-radius: var(--r-md, 8px); overflow: hidden; margin: 12px 0; }
.markdown .code-head { display: flex; align-items: center; justify-content: space-between; gap: 8px; padding: 5px 12px; color: var(--muted); font-size: 12px; border-bottom: 1px solid var(--line); }
.markdown .code-head button { min-height: 28px; padding: 2px 10px; background: var(--surface); border: 1px solid var(--line); color: var(--fg); border-radius: var(--r-sm, 6px); cursor: pointer; font-size: 12px; }
.markdown .code-head button:focus-visible { outline: 2px solid var(--accent-text); outline-offset: 2px; }
.markdown pre { overflow: auto; white-space: pre; margin: 0; padding: 12px 14px; max-width: 100%; }
.markdown .tok-kw { color: #7c3aed; font-weight: 500; }
.markdown .tok-str { color: #0f766e; }
.markdown .tok-num { color: #c2410c; }
.markdown .tok-com { color: var(--muted); font-style: italic; }
.markdown .tok-key { color: var(--accent-text); }
.markdown .cite-pill { display: inline-flex; align-items: center; justify-content: center; min-width: 16px; height: 16px; padding: 0 4px; margin: 0 1px; border-radius: 99px; background: var(--raised); border: 1px solid var(--line); font-size: 10px; font-weight: 600; color: var(--accent-text); vertical-align: super; line-height: 1; }
.markdown .markdown-table { overflow: auto; max-width: 100%; margin: 12px 0; border: 1px solid var(--line); border-radius: var(--r-md, 8px); background: var(--surface); }
.markdown table { border-collapse: collapse; width: 100%; font-size: 14px; }
.markdown th, .markdown td { padding: 8px 12px; border-bottom: 1px solid var(--hair, var(--line)); text-align: left; min-width: 80px; }
.markdown th { background: var(--raised); font-weight: 600; font-size: 12.5px; color: var(--muted); cursor: pointer; user-select: none; }
.markdown th[aria-sort="ascending"]::after { content: " ↑"; color: var(--accent-text); }
.markdown th[aria-sort="descending"]::after { content: " ↓"; color: var(--accent-text); }
.markdown td.num { text-align: right; font-variant-numeric: tabular-nums; }
.markdown tbody tr:hover { background: color-mix(in srgb, var(--raised) 50%, transparent); }
.markdown .table-bar, .ui-native .table-bar { display: flex; align-items: center; justify-content: space-between; gap: 8px; padding: 6px 10px; border-top: 1px solid var(--hair, var(--line)); background: var(--surface); }
.table-csv-btn { font-size: 11.5px; padding: 3px 8px; border: 1px solid var(--line); border-radius: var(--r-sm, 6px); background: var(--surface); color: var(--muted); cursor: pointer; }
.table-csv-btn:hover { color: var(--fg); border-color: var(--muted); }
.md-callout { margin: 12px 0; padding: 10px 14px; border-radius: var(--r-md, 8px); border: 1px solid var(--line); border-left: 3px solid var(--accent); background: color-mix(in srgb, var(--raised) 65%, transparent); }
.md-callout.tone-tip { border-left-color: #0f766e; }
.md-callout.tone-warning, .md-callout.tone-caution { border-left-color: #c2410c; }
.md-callout.tone-important { border-left-color: #7c3aed; }
.md-callout-title { font-size: 12.5px; font-weight: 600; text-transform: uppercase; letter-spacing: 0.03em; color: var(--fg); margin-bottom: 4px; }
.md-callout-body { font-size: 14px; color: var(--fg2); line-height: 1.5; }
.markdown img { max-width: 100%; height: auto; object-fit: contain; }
.markdown :focus-visible { outline: 2px solid var(--focus, var(--accent-text)); outline-offset: 2px; }
.ui-native { display: flex; flex-direction: column; gap: 12px; margin: 10px 0; --bar: var(--accent); --s0: var(--accent); --s1: #c2410c; --s2: #0f766e; --s3: #7c3aed; --s4: #a16207; --s5: #be123c; }
.ui-native .stat-grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(140px, 1fr)); gap: 10px; }
.ui-native .stat { background: var(--surface); border: 1px solid var(--line); border-radius: var(--r-md, 8px); padding: 12px 14px; margin: 0; }
.ui-native .sv { font-size: 24px; font-weight: 650; letter-spacing: -0.02em; font-variant-numeric: tabular-nums; display: flex; align-items: baseline; gap: 6px; flex-wrap: wrap; }
.ui-native .su { font-size: 13px; color: var(--muted); font-weight: 500; }
.ui-native .sl { font-size: 12.5px; color: var(--muted); margin-top: 2px; }
.stat-delta { font-size: 11.5px; font-weight: 600; padding: 1px 6px; border-radius: 99px; }
.stat-delta.up { background: color-mix(in srgb, #0f766e 15%, transparent); color: #0f766e; }
.stat-delta.down { background: color-mix(in srgb, #be123c 15%, transparent); color: #be123c; }
.ui-native figure { position: relative; margin: 0; background: var(--surface); border: 1px solid var(--line); border-radius: var(--r-lg, 10px); overflow: hidden; }
.ui-native figure > h3 { font-size: 14px; font-weight: 600; color: var(--fg); padding: 12px 14px 0; margin: 0; }
.ui-native figure > svg { width: 100%; max-width: 600px; height: auto; display: block; padding: 10px 14px; box-sizing: border-box; }
.ui-native svg text { fill: var(--fg); font-size: 12px; font-family: inherit; }
.ui-native svg .val { fill: var(--muted); font-variant-numeric: tabular-nums; }
.ui-native .bar { fill: var(--bar); transition: opacity .12s ease; }
.ui-native .chart-row:hover .bar { opacity: 0.8; }
.ui-native .axis, .ui-native .grid { stroke: var(--line); stroke-width: 1; }
.ui-native .ln { fill: none; stroke-width: 2.25; }
.ui-native .ln.s0 { stroke: var(--s0); } .ui-native .ln.s1 { stroke: var(--s1); } .ui-native .ln.s2 { stroke: var(--s2); } .ui-native .ln.s3 { stroke: var(--s3); } .ui-native .ln.s4 { stroke: var(--s4); } .ui-native .ln.s5 { stroke: var(--s5); }
.ui-native .pt { fill: var(--bar); cursor: pointer; }
.ui-native .pt.s0 { fill: var(--s0); } .ui-native .pt.s1 { fill: var(--s1); } .ui-native .pt.s2 { fill: var(--s2); } .ui-native .pt.s3 { fill: var(--s3); } .ui-native .pt.s4 { fill: var(--s4); } .ui-native .pt.s5 { fill: var(--s5); }
.ui-native .legend { font-size: 12.5px; color: var(--muted); padding: 0 14px 10px; margin: 0; }
.ui-native .key { display: inline-block; width: 9px; height: 9px; border-radius: 2px; margin: 0 5px 0 10px; }
.ui-native .key.s0 { background: var(--s0); } .ui-native .key.s1 { background: var(--s1); } .ui-native .key.s2 { background: var(--s2); } .ui-native .key.s3 { background: var(--s3); } .ui-native .key.s4 { background: var(--s4); } .ui-native .key.s5 { background: var(--s5); }
.ui-native .chart-tooltip { position: absolute; top: 10px; right: 12px; padding: 4px 9px; border-radius: var(--r-sm, 6px); background: var(--fg); color: var(--surface); font-size: 12px; font-weight: 500; pointer-events: none; }
.ui-native .tw { overflow-x: auto; padding: 10px 14px; }
.ui-native table { border-collapse: collapse; width: 100%; font-size: 14px; }
.ui-native th, .ui-native td { text-align: left; padding: 6px 10px 6px 0; border-bottom: 1px solid var(--hair, var(--line)); }
.ui-native th { color: var(--muted); font-size: 12px; font-weight: 600; cursor: pointer; }
.ui-native td.num { text-align: right; font-variant-numeric: tabular-nums; }
.ui-native .cmp td.best { font-weight: 650; color: var(--accent-text); }
.ui-native .tl { list-style: none; padding: 0; margin: 0; }
.ui-native .tl li { display: flex; gap: 12px; padding: 10px 14px; border-bottom: 1px solid var(--hair, var(--line)); }
.ui-native .tl li:last-child { border-bottom: 0; }
.ui-native .tl .when { flex: 0 0 64px; font-size: 12px; font-weight: 600; color: var(--accent-text); }
.ui-native .tl .what { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
.ui-native .tl .lbl { font-weight: 600; font-size: 14px; }
.ui-native .tl .note { color: var(--fg2); font-size: 13px; }
.ui-sources { display: flex; flex-direction: column; gap: 6px; }
.ui-sources-head { font-size: 12px; font-weight: 600; color: var(--muted); text-transform: uppercase; letter-spacing: 0.04em; }
.ui-sources-list { display: flex; flex-wrap: wrap; gap: 6px; }
.source-pill { display: inline-flex; align-items: center; gap: 6px; padding: 4px 10px; border: 1px solid var(--line); border-radius: 99px; background: var(--surface); color: var(--fg); text-decoration: none; font-size: 12.5px; max-width: 100%; }
.source-pill:hover { border-color: var(--accent); }
.source-idx { display: inline-flex; align-items: center; justify-content: center; width: 16px; height: 16px; border-radius: 99px; background: var(--raised); color: var(--accent-text); font-size: 10.5px; font-weight: 600; flex: none; }
.source-title { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; max-width: 240px; }
.source-dom { color: var(--muted); font-size: 11.5px; }
@media (any-pointer: coarse) { .markdown .code-head button { min-height: 44px; min-width: 44px; } }
`;
  const CSS = `
    :root { --bar: var(--accent); --s0: var(--accent); --s1:#c2410c; --s2:#0f766e; --s3:#7c3aed; --s4:#a16207; --s5:#be123c; }
    :is(a,button,input,textarea,select,summary,[tabindex]):focus-visible { outline:2px solid var(--accent-text); outline-offset:2px; }
    html,body { margin:0; background: transparent; color:var(--fg); font: 15px/1.5 Inter, system-ui, sans-serif; -webkit-font-smoothing: antialiased; font-feature-settings: "tnum" 1, "cv11" 1; }
    body { padding: 4px; display: flex; flex-direction: column; gap: 16px; }
    body > * { margin: 0; }
    h2 { font-size: 15px; font-weight: 600; letter-spacing: 0; }
    h3 { font-size: 16px; font-weight: 600; color: var(--muted); margin: 0 0 8px; }
    figure > h3 { padding: 14px 16px 0; margin: 0; }
    p { max-width: 70ch; white-space: pre-line; font-size: 15px; }
    h4 { font-size: 14px; font-weight: 600; }
    ul, ol { max-width: 70ch; }
    li { margin: 3px 0; }
    .file { border-top: 1px solid var(--line); padding-top: 4px; }
    .fname { color: var(--muted); font-size: 12px; margin: 4px 0 0; }
    /* Each figure is a card: the surface, its edge and shadow, as in the app. */
    figure { margin: 0; background: var(--surface); border: 1px solid var(--line); border-radius: var(--r-lg, 10px); box-shadow: var(--shadow); overflow: hidden; }
    figure > :not(h3):not(.tw):not(.tl):not(svg) { padding-left: 16px; padding-right: 16px; }
    figure > ul, figure > ol:not(.tl) { padding: 12px 16px 12px 36px; margin: 0; }
    figure > svg { padding: 12px 16px; box-sizing: border-box; }
    .cap { color: var(--muted); font-size: 13px; }
    figure > .cap { padding-bottom: 12px; }
    .tw { overflow-x: auto; padding: 14px 16px; }
    table { border-collapse: collapse; width: 100%; font-size: 15px; }
    th, td { text-align: left; padding: 4px 12px 4px 0; color: var(--fg2); }
    th:last-child, td:last-child { padding-right: 0; }
    th { color: var(--muted); font-weight: 600; font-size: 13px; padding-bottom: 8px; }
    td.num, th.num { text-align: right; font-variant-numeric: tabular-nums; }
    tr.total td { color: var(--fg); font-weight: 600; border-top: 1px solid var(--hair); padding-top: 8px; }
    .stat { display: inline-block; margin: 0 32px 0 0; vertical-align: top; }
    .sv { font-size: 30px; font-weight: 600; letter-spacing: -0.02em; font-variant-numeric: tabular-nums; }
    .su { font-size: 15px; color: var(--muted); font-weight: 500; }
    .sl { color: var(--muted); font-size: 13px; }
    svg { width: 100%; max-width: 560px; height: auto; display: block; }
    svg text { fill: var(--fg); font-size: 12px; font-family: Inter, system-ui, sans-serif; }
    svg .val { fill: var(--muted); font-variant-numeric: tabular-nums; }
    .bar { fill: var(--bar); }
    .axis, .grid { stroke: var(--line); stroke-width: 1; }
    .ln { fill: none; stroke-width: 2.25; }
    ${[0, 1, 2, 3, 4, 5].map(k => `.ln.s${k}{stroke:var(--s${k})} .key.s${k}{background:var(--s${k})}`).join(" ")}
    .legend { font-size: 13px; color: var(--muted); }
    .key { display: inline-block; width: 10px; height: 10px; border-radius: 2px; margin: 0 6px 0 10px; }
    img { max-width: 100%; border: 1px solid var(--line); border-radius: 4px; }
    .area { fill: none; stroke: var(--line); }
    .pt { fill: var(--bar); }
    .deck { display: flex; flex-direction: column; gap: 12px; }
    .deck-bar { display: flex; align-items: center; justify-content: space-between; gap: 8px; }
    .deck-present { min-height: 44px; padding: 0 16px; background: var(--surface); border: 1px solid var(--muted); color: var(--fg); border-radius: var(--r-sm, 6px); cursor: pointer; }
    .slide { aspect-ratio: 16 / 9; background: var(--surface); border: 1px solid var(--line); border-radius: var(--r-md, 8px); padding: 32px; box-sizing: border-box; overflow: hidden; }
    .slide h2 { font-size: 28px; margin: 0 0 16px; }
    .slide.title { display: flex; flex-direction: column; justify-content: center; }
    body.presenting { padding: 0; display: block; }
    body.presenting .deck-bar { display: none; }
    body.presenting .slide { display: none; border: 0; border-radius: 0; height: 100vh; width: 100vw; font-size: 24px; }
    body.presenting .slide.on { display: block; }
    body.presenting .slide.title.on { display: flex; }
    .placeholder { display: grid; place-items: center; min-height: 180px; color: var(--muted); font-size: 13px; background: var(--raised); }
    .gallery { display: grid; grid-template-columns: repeat(auto-fill, minmax(160px, 1fr)); gap: 12px; padding: 14px 16px; }
    .uiform { display: flex; flex-direction: column; gap: 12px; padding: 14px 16px; }
    .uiform label { display: flex; flex-direction: column; gap: 4px; font-size: 13px; color: var(--muted); }
    .uiform :is(input, select, textarea) { font: inherit; color: var(--fg); background: var(--surface); border: 1px solid var(--line); border-radius: var(--r-sm, 6px); padding: 8px 12px; }
    .uiform .form-send { align-self: flex-start; min-height: 44px; padding: 0 16px; }
    .tl { list-style: none; padding: 0; margin: 0; max-width: none; }
    .tl li { display: flex; gap: 14px; padding: 14px 16px; margin: 0; border-bottom: 1px solid var(--hair); }
    .tl li:last-child { border-bottom: 0; }
    .tl .when { flex: 0 0 52px; font-size: 12px; font-weight: 600; color: var(--accent-text); text-transform: uppercase; font-variant-numeric: tabular-nums; }
    .tl .when .sub { display: block; color: var(--muted); font-weight: 500; text-transform: none; }
    .tl .what { display: flex; flex-direction: column; gap: 3px; min-width: 0; }
    .tl .lbl { font-weight: 600; }
    .tl .note { color: var(--fg2); font-size: 13px; }
    .cmp th[scope=row] { text-transform: none; letter-spacing: 0; font-size: 14px; color: var(--fg); font-weight: 500; }
    .cmp td.best { font-weight: 650; color: var(--bar); }
    .sr { position: absolute; width: 1px; height: 1px; overflow: hidden; clip: rect(0 0 0 0); }
    pre.code { font: 15px/1.5 ui-monospace, SFMono-Regular, Menlo, monospace; background: color-mix(in srgb, var(--line) 35%, transparent); padding: 10px 12px; border-radius: 4px; overflow-x: auto; white-space: pre; }
  `;

  /// A whole document for the frame's srcdoc. `files` maps a delivered
  /// file's name to what the app fetched itself: {image: a data: URL} or
  /// {text, media_type}. `limit` is how much of a file to show.
  /// `tokens`, the app's look (Theme.tokens), colors it; `mode` is its
  /// light or dark, so the frame's own color scheme agrees with the page.
  function documentFor(view, files, fontCss, limit, tokens, mode) {
    const body = renderParts(view, files || {}, limit || { chars: 160000, rows: 200 })
      .replace(/<button type="button" data-code-copy[^>]*>Copy code<\/button>/g,'<span data-code-copy-anchor aria-hidden="true" style="display:inline-flex;align-items:center;min-height:44px;padding:0 12px">Copy code</span>')
      .replace(/<a href="([^"]*)" data-prose-link rel="noopener noreferrer">([\s\S]*?)<\/a>/g,'<span data-prose-url="$1" style="color:var(--accent-text);text-decoration:underline">$2</span>');
    return `<!doctype html><html style="color-scheme: ${mode === "dark" ? "dark" : "light"}"><head><meta charset="utf-8">
      <meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'; img-src data:; font-src data: ${location.origin}">
      <style>:root{${vars(tokens)}}${fontCss || ""}${CSS}${markdownStyles}</style></head><body>${body}</body></html>`;
  }

  return { documentFor, renderNative, TEXT, markdown, safeUrl, markdownStyles, maskedText };
})();
