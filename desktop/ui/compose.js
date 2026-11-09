// Draws an outcome the agent composed from the fixed components (U8). The
// agent chooses which components and what they say; this code decides how
// they look. The result is a document for a sandboxed frame that may not run
// scripts, so nothing the agent wrote can execute.
"use strict";

const Compose = (() => {
  const esc = s => String(s ?? "").replace(/[&<>"']/g, c => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
  const fmt = v => typeof v === "number" ? (Math.abs(v) >= 1000 ? v.toLocaleString() : String(Math.round(v * 1000) / 1000)) : esc(v);

  function frame(c, inner) {
    const title = c.title ? `<h3>${esc(c.title)}</h3>` : "";
    const cap = c.caption ? `<p class="cap">${esc(c.caption)}</p>` : "";
    return `<figure>${title}${inner}${cap}</figure>`;
  }

  function bar(c) {
    const vals = c.values.map(Number), max = Math.max(0, ...vals), min = Math.min(0, ...vals);
    const span = max - min || 1, rowH = 26, labelW = 120, w = 560, plotW = w - labelW - 70;
    const zero = labelW + ((0 - min) / span) * plotW;
    const rows = c.labels.map((l, i) => {
      const v = vals[i], x0 = labelW + ((Math.min(0, v) - min) / span) * plotW, bw = (Math.abs(v) / span) * plotW, y = i * rowH;
      return `<text x="${labelW - 8}" y="${y + 17}" text-anchor="end" class="lbl">${esc(l)}</text>
        <rect x="${x0}" y="${y + 5}" width="${Math.max(bw, 1)}" height="${rowH - 10}" class="bar"/>
        <text x="${x0 + Math.max(bw, 1) + 6}" y="${y + 17}" class="val">${fmt(v)}${c.unit ? " " + esc(c.unit) : ""}</text>`;
    }).join("");
    const h = c.labels.length * rowH;
    return frame(c, `<svg viewBox="0 0 ${w} ${h + 4}" role="img" aria-label="${esc(c.title || "bar chart")}"><line x1="${zero}" y1="0" x2="${zero}" y2="${h}" class="axis"/>${rows}</svg>`);
  }

  function line(c) {
    const all = c.series.flatMap(s => s.values.map(Number)), max = Math.max(...all), min = Math.min(...all);
    const span = max - min || 1, w = 560, h = 220, l = 48, r = 12, t = 10, b = 30;
    const x = i => l + (c.labels.length === 1 ? 0 : (i / (c.labels.length - 1)) * (w - l - r));
    const y = v => t + (1 - (v - min) / span) * (h - t - b);
    const ticks = [min, (min + max) / 2, max].map(v => `<text x="${l - 6}" y="${y(v) + 4}" text-anchor="end" class="val">${fmt(v)}</text><line x1="${l}" x2="${w - r}" y1="${y(v)}" y2="${y(v)}" class="grid"/>`).join("");
    const step = Math.ceil(c.labels.length / 8);
    const xl = c.labels.map((lb, i) => i % step === 0 ? `<text x="${x(i)}" y="${h - 10}" text-anchor="middle" class="lbl">${esc(lb)}</text>` : "").join("");
    const paths = c.series.map((s, k) => `<polyline class="ln s${k}" points="${s.values.map((v, i) => `${x(i)},${y(Number(v))}`).join(" ")}"/>`).join("");
    const legend = c.series.length > 1 ? `<p class="legend">${c.series.map((s, k) => `<span class="key s${k}"></span>${esc(s.name)}`).join(" ")}</p>` : "";
    return frame(c, `<svg viewBox="0 0 ${w} ${h}" role="img" aria-label="${esc(c.title || "line chart")}">${ticks}${xl}${paths}</svg>${legend}${c.unit ? `<p class="cap">Values in ${esc(c.unit)}</p>` : ""}`);
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
  function codeBlock(text,language="") {
    return `<div class="code-block"><div class="code-head"><span>${esc(language)}</span><button type="button" data-code-copy aria-label="Copy code" data-tooltip="Copy code">Copy code</button></div><pre class="code"><code>${esc(text)}</code></pre></div>`;
  }
  function markdownInline(text, depth = 0) {
    if (depth > 8) return esc(text);
    const pattern = /(`+)([\s\S]*?)\1|(!?)\[([^\]\n]*)\]\(([^\s]*?)(?:\s+"[^"\n]*")?\)|\*\*([^*\n]+)\*\*|__([^_\n]+)__|\*([^*\n]+)\*|_([^_\n]+)_|<(https?:\/\/[^>\s]+)>/g;
    let out = "", at = 0;
    for (const m of text.matchAll(pattern)) {
      out += esc(text.slice(at, m.index)); at = m.index + m[0].length;
      if (m[1]) out += `<code>${esc(m[2].replace(/\n/g, " "))}</code>`;
      else if (m[4] !== undefined || m[10]) {
        const label = m[4] ?? m[10], raw = m[5] ?? m[10], url = safeUrl(raw);
        const localImage = m[3] && localImagePath(raw);
        if (localImage) out += `<img data-prose-image="${esc(localImage)}" alt="${esc(label)}" loading="lazy">`;
        else out += url ? `<a href="${esc(url)}" data-prose-link rel="noopener noreferrer">${markdownInline(label, depth + 1)}</a>` : esc(label);
      } else if (m[6] || m[7]) out += `<strong>${markdownInline(m[6] || m[7], depth + 1)}</strong>`;
      else out += `<em>${markdownInline(m[8] || m[9], depth + 1)}</em>`;
    }
    return out + esc(text.slice(at));
  }
  function markdown(source, limit) {
    const maxRows=Number.isFinite(limit?.rows) ? Math.max(0,Math.floor(limit.rows)) : Infinity;
    const lines = String(source || "").replace(/\r\n?/g, "\n").split("\n"), out = [];
    const cells = line => line.trim().replace(/^\|/, "").replace(/\|$/, "").split(/(?<!\\)\|/).map(c => markdownInline(c.trim().replace(/\\\|/g, "|")));
    const tableRule = /^\s*\|?\s*:?-{3,}:?\s*(?:\|\s*:?-{3,}:?\s*)*\|?\s*$/;
    const block = line => /^\s*$|^\s{0,3}(?:#{1,6}\s|>|[-*+]\s|\d+[.)]\s|`{3,}|~{3,})/.test(line);
    for (let i = 0; i < lines.length;) {
      const line = lines[i]; let m;
      if (!line.trim()) { i++; continue; }
      if ((m = line.match(/^\s{0,3}(`{3,}|~{3,})([^\s]*)\s*$/))) {
        const fence = m[1], lang = m[2], body = [];
        for (i++; i < lines.length && !new RegExp(`^\\s{0,3}${fence[0]}{${fence.length},}\\s*$`).test(lines[i]); i++) body.push(lines[i]);
        if (i < lines.length) i++;
        out.push(codeBlock(body.join("\n"),lang));
      } else if ((m = line.match(/^\s{0,3}(#{1,6})\s+(.*?)\s*#*\s*$/))) { out.push(`<h${m[1].length}>${markdownInline(m[2])}</h${m[1].length}>`); i++; }
      else if (/^\s{0,3}([-*_])(\s*\1){2,}\s*$/.test(line)) { out.push("<hr>"); i++; }
      else if (line.includes("|") && tableRule.test(lines[i + 1] || "")) {
        const head = cells(line), rows = []; let total=0; i += 2;
        while (i < lines.length && lines[i].trim() && lines[i].includes("|")) { if(rows.length<maxRows)rows.push(cells(lines[i]));total++;i++; }
        out.push(`<div class="markdown-table" tabindex="0" role="region" aria-label="Table"><table><thead><tr>${head.map(c => `<th scope="col">${c}</th>`).join("")}</tr></thead><tbody>${rows.map(row => `<tr>${head.map((_, n) => `<td>${row[n] || ""}</td>`).join("")}</tr>`).join("")}</tbody></table>${total>rows.length ? `<p class="cap">The first ${rows.length} of ${total} rows; the rest are in the file.</p>` : ""}</div>`);
      } else if (/^\s{0,3}>/.test(line)) {
        const quote = []; while (i < lines.length && /^\s{0,3}>/.test(lines[i])) quote.push(lines[i++].replace(/^\s{0,3}>\s?/, ""));
        // Quotes need no recursive block parser to remain readable and safe.
        out.push(`<blockquote>${quote.map(x => markdownInline(x)).join("<br>")}</blockquote>`);
      } else if ((m = line.match(/^\s*([-*+]|\d+[.)])\s+(.*)$/))) {
        const ordered = /\d/.test(m[1]), tag = ordered ? "ol" : "ul", items = [], start = ordered ? ` start="${parseInt(m[1], 10)}"` : "";
        while (i < lines.length) {
          const item = lines[i].match(/^\s*([-*+]|\d+[.)])\s+(.*)$/);
          if (!item || /\d/.test(item[1]) !== ordered) break;
          i++; const parts = [item[2]];
          while (i < lines.length && /^\s{2,}\S/.test(lines[i]) && !/^\s*([-*+]|\d+[.)])\s/.test(lines[i])) parts.push(lines[i++].trim());
          items.push(`<li>${markdownInline(parts.join("\n"))}</li>`);
        }
        out.push(`<${tag}${start}>${items.join("")}</${tag}>`);
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

  function part(c, files, limit) {
    switch (c.type) {
      case "heading": return c.level > 2 ? `<h4>${esc(c.text)}</h4>` : `<h2>${esc(c.text)}</h2>`;
      case "markdown": return markdown(c.text,limit);
      case "text": return `<p${c.muted ? ` class="cap"` : ""}>${esc(c.text)}</p>`;
      case "list": return frame(c, c.ordered ? `<ol>${c.items.map(i => `<li>${esc(i)}</li>`).join("")}</ol>` : `<ul>${c.items.map(i => `<li>${esc(i)}</li>`).join("")}</ul>`);
      case "table": {
        const head = c.columns.some(h => String(h).trim()) ? `<thead><tr>${c.columns.map(h => `<th>${esc(h)}</th>`).join("")}</tr></thead>` : "";
        const cell = (v, i) => `<td class="${typeof v === "number" || (i > 0 && /^[-+]?[$€£]?[\d,.]+%?$/.test(String(v).trim())) ? "num" : ""}">${fmt(v)}</td>`;
        return frame(c, `<div class="tw"><table>${head}<tbody>${c.rows.map(r => `<tr${/^total\b/i.test(String(r[0])) ? ` class="total"` : ""}>${r.map(cell).join("")}</tr>`).join("")}</tbody></table></div>`);
      }
      case "stat": return `<div class="stat"><div class="sv">${fmt(c.value)}${c.unit ? `<span class="su"> ${esc(c.unit)}</span>` : ""}</div><div class="sl">${esc(c.label)}</div>${c.caption ? `<p class="cap">${esc(c.caption)}</p>` : ""}</div>`;
      case "bar_chart": return bar(c);
      case "line_chart": return line(c);
      case "map": return map(c);
      case "timeline": return timeline(c);
      case "comparison": return comparison(c);
      case "code": return frame(c, `<div class="markdown">${codeBlock(c.code,c.language)}</div>`);
      case "image": return files[c.file]?.image ? frame(c, `<img src="${files[c.file].image}" alt="${esc(c.caption || c.file)}">`) : frame(c, `<p class="cap">${esc(c.file)} (not an image this app can show)</p>`);
      case "file": return `<section class="file" data-file="${esc(c.file)}"><p class="fname">${esc(c.file)}</p>${fileParts(c.file, files[c.file], limit).map(x => part(x, files, limit)).join("\n")}</section>`;
      default: return "";
    }
  }

  // Colors come from the app's look (theme.js tokens), so a result reads the
  // same as the cards around it; without them, a light and a dark default.
  const VARS = ["fg", "fg2", "muted", "line", "hair", "surface", "raised", "shadow", "accent", "accent-text"];
  const DEFAULT_LIGHT = { fg: "#1C1C1E", fg2: "#3F3F44", muted: "#5E5E63", line: "#E7E6E2", hair: "#EFEEEA", surface: "#FFFFFF", raised: "#F6F6F3", shadow: "0 1px 2px rgba(0,0,0,0.04), 0 6px 20px rgba(0,0,0,0.04)", accent: "#3557D9", "accent-text": "#3557D9" };
  const vars = t => VARS.map(k => `--${k}:${(t || DEFAULT_LIGHT)[k] ?? DEFAULT_LIGHT[k]};`).join("");
  const markdownStyles = `.markdown { font-size: 15px; line-height: 1.5; overflow-wrap: anywhere; min-width: 0; }
.markdown p { margin: 8px 0; white-space: pre-wrap; }
.markdown h1, .markdown h2, .markdown h3, .markdown h4 { color: var(--fg); margin: 16px 0 8px; }
.markdown h1 { font-size: 24px; } .markdown h2 { font-size: 20px; } .markdown h3 { font-size: 17px; }
.markdown a { color: var(--accent-text); text-decoration: underline; }
.markdown ul, .markdown ol { padding-left: 24px; }
.markdown blockquote { border-left: 2px solid var(--muted); margin: 12px 0; padding: 4px 16px; }
.markdown code { font-family: var(--mono, monospace); font-size: 15px; }
.markdown :not(pre) > code { background: var(--raised); border-radius: 4px; padding: 2px 4px; }
.markdown .code-block { background: var(--raised); border: 1px solid var(--line); border-radius: 12px; overflow: hidden; margin: 12px 0; }
.markdown .code-head { display: flex; align-items: center; justify-content: space-between; gap: 8px; padding: 4px 12px; color: var(--muted); }
.markdown .code-head button { min-height: 36px; padding: 4px 12px; background: var(--surface); border: 1px solid var(--muted); color: var(--fg); border-radius: 8px; cursor: pointer; }
.markdown .code-head button:focus-visible { outline: 2px solid var(--accent-text); outline-offset: 2px; }
@media (any-pointer: coarse) { .markdown .code-head button { min-width: 44px; min-height: 44px; } }
.markdown pre { overflow: auto; white-space: pre; margin: 0; padding: 12px 16px; max-width: 100%; }
.markdown .markdown-table { overflow: auto; max-width: 100%; margin: 12px 0; }
.markdown table { border-collapse: collapse; width: 100%; }
.markdown th, .markdown td { padding: 8px 12px; border: 1px solid var(--line); text-align: left; min-width: 100px; }
.markdown img { max-width: 100%; height: auto; object-fit: contain; }
.markdown :focus-visible { outline: 2px solid var(--focus, var(--accent-text)); outline-offset: 2px; }
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
    figure { margin: 0; background: var(--surface); border: 1px solid var(--line); border-radius: 16px; box-shadow: var(--shadow); overflow: hidden; }
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
    const body = (view || []).map(c => part(c, files || {}, limit || { chars: 160000, rows: 200 })).join("\n")
      .replace(/<button type="button" data-code-copy[^>]*>Copy code<\/button>/g,'<span data-code-copy-anchor aria-hidden="true" style="display:inline-flex;align-items:center;min-height:44px;padding:0 12px">Copy code</span>')
      .replace(/<a href="([^"]*)" data-prose-link rel="noopener noreferrer">([\s\S]*?)<\/a>/g,'<span data-prose-url="$1" style="color:var(--accent-text);text-decoration:underline">$2</span>');
    return `<!doctype html><html style="color-scheme: ${mode === "dark" ? "dark" : "light"}"><head><meta charset="utf-8">
      <meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'; img-src data:; font-src data: ${location.origin}">
      <style>:root{${vars(tokens)}}${fontCss || ""}${CSS}${markdownStyles}</style></head><body>${body}</body></html>`;
  }

  return { documentFor, TEXT, markdown, safeUrl, markdownStyles };
})();
