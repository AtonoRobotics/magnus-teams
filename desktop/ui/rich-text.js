// Rich text helpers: smart HTML-to-Markdown paste, zero-dependency code syntax
// highlighting, and client-side interactivity for Markdown tables and native
// magnus-ui charts (zero extra LLM tokens).
"use strict";

const RichText = (() => {
  const esc = s => String(s ?? "").replace(/[&<>"']/g, c => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
  const SAFE_URL_RE = /^https?:\/\//i;
  const RICH_TAG_RE = /<(?:h[1-6]|strong|b|em|i|del|s|a|ul|ol|li|table|thead|tbody|tr|th|td|blockquote|pre|code|hr)\b/i;

  /**
   * Convert clipboard HTML (Google Docs, Notion, Slack, Excel, web pages) into
   * clean Markdown. Returns null when the HTML has no meaningful formatting so
   * plain text paste runs untouched.
   * @param {string} html
   * @returns {?string}
   */
  function htmlToMarkdown(html) {
    if (!html || !RICH_TAG_RE.test(html) || typeof DOMParser === "undefined") return null;
    const doc = new DOMParser().parseFromString(html, "text/html");
    for (const bad of doc.querySelectorAll("script, style, meta, link, title, head")) bad.remove();

    const cleanInline = s => s.replace(/\u00a0/g, " ").replace(/[ \t]+/g, " ");

    /** @param {Node} node @param {{ listDepth: number, inPre: boolean }} ctx @returns {string} */
    const walk = (node, ctx) => {
      if (node.nodeType === 3) {
        const raw = node.textContent || "";
        return ctx.inPre ? raw : raw.replace(/\r?\n/g, " ");
      }
      if (node.nodeType !== 1) return "";
      const el = /** @type {HTMLElement} */ (node);
      const tag = el.tagName.toLowerCase();
      const children = (nextCtx = ctx) => [...el.childNodes].map(c => walk(c, nextCtx)).join("");

      if (tag === "br") return "\n";
      if (tag === "hr") return "\n\n---\n\n";
      if (/^h[1-6]$/.test(tag)) {
        const txt = cleanInline(children()).trim();
        return txt ? `\n\n${"#".repeat(Number(tag[1]))} ${txt}\n\n` : "";
      }
      if (tag === "pre") {
        const codeEl = el.querySelector("code");
        const langMatch = (codeEl?.className || el.className || "").match(/language-([a-z0-9_-]+)/i);
        const lang = langMatch ? langMatch[1] : "";
        const body = (codeEl ? codeEl.textContent : el.textContent || "").replace(/\r\n?/g, "\n").replace(/\n$/, "");
        return `\n\n\`\`\`${lang}\n${body}\n\`\`\`\n\n`;
      }
      if (tag === "code") {
        const txt = (el.textContent || "").replace(/\r?\n/g, " ");
        return txt ? `\`${txt.replace(/`/g, "\\`")}\`` : "";
      }
      if (tag === "strong" || tag === "b") {
        const txt = children().trim();
        return txt ? `**${txt}**` : "";
      }
      if (tag === "em" || tag === "i") {
        const txt = children().trim();
        return txt ? `*${txt}*` : "";
      }
      if (tag === "del" || tag === "s" || tag === "strike") {
        const txt = children().trim();
        return txt ? `~~${txt}~~` : "";
      }
      if (tag === "a") {
        const href = (el.getAttribute("href") || "").trim();
        const label = cleanInline(children()).trim();
        if (!label) return "";
        return SAFE_URL_RE.test(href) ? `[${label}](${href})` : label;
      }
      if (tag === "blockquote") {
        const inner = children().trim();
        if (!inner) return "";
        return `\n\n${inner.split("\n").map(l => `> ${l}`).join("\n")}\n\n`;
      }
      if (tag === "ul" || tag === "ol") {
        const ordered = tag === "ol";
        const start = Number(el.getAttribute("start")) || 1;
        const indent = "  ".repeat(ctx.listDepth);
        let idx = 0;
        const rows = [];
        for (const child of el.children) {
          if (child.tagName.toLowerCase() !== "li") continue;
          const chk = child.querySelector(':scope > input[type="checkbox"]');
          const bullet = ordered ? `${start + idx++}.` : chk ? (/** @type {HTMLInputElement} */ (chk).checked ? "- [x]" : "- [ ]") : "-";
          const parts = [...child.childNodes]
            .filter(c => c !== chk)
            .map(c => walk(c, { listDepth: ctx.listDepth + 1, inPre: false }))
            .join("")
            .trim();
          const lines = parts.split("\n");
          const first = (lines.shift() || "").trim();
          const rest = lines.length ? "\n" + lines.join("\n") : "";
          rows.push(`${indent}${bullet} ${first}${rest}`);
        }
        return rows.length ? `${ctx.listDepth === 0 ? "\n\n" : "\n"}${rows.join("\n")}${ctx.listDepth === 0 ? "\n\n" : ""}` : "";
      }
      if (tag === "table") {
        const trs = [...el.querySelectorAll("tr")];
        if (!trs.length) return "";
        const matrix = trs.map(tr => [...tr.querySelectorAll("th, td")].map(c => cleanInline(walk(c, ctx)).trim().replace(/\|/g, "\\|")));
        const cols = Math.max(...matrix.map(r => r.length), 0);
        if (!cols) return "";
        const pad = r => Array.from({ length: cols }, (_, i) => r[i] || "");
        const head = pad(matrix[0]);
        const body = matrix.slice(1).map(pad);
        const lines = [
          `| ${head.join(" | ")} |`,
          `| ${head.map(() => "---").join(" | ")} |`,
          ...body.map(r => `| ${r.join(" | ")} |`),
        ];
        return `\n\n${lines.join("\n")}\n\n`;
      }
      if (tag === "p" || tag === "div" || tag === "section" || tag === "article") {
        const inner = children().trim();
        return inner ? `\n\n${inner}\n\n` : "";
      }
      if (tag === "span") {
        const st = el.getAttribute("style") || "";
        let inner = children();
        if (!inner.trim()) return inner;
        if (/font-weight\s*:\s*(?:bold|[6-9]00)/i.test(st)) inner = `**${inner.trim()}**`;
        if (/font-style\s*:\s*italic/i.test(st)) inner = `*${inner.trim()}*`;
        return inner;
      }
      return children();
    };

    const md = walk(doc.body, { listDepth: 0, inPre: false })
      .replace(/[ \t]+\n/g, "\n")
      .replace(/\n{3,}/g, "\n\n")
      .trim();
    return md || null;
  }

  /**
   * Lightweight, zero-dependency syntax highlighter for fenced code blocks.
   * Always escapes raw text first so untrusted HTML never enters the DOM.
   * @param {string} raw @param {string} [lang]
   */
  function highlightCode(raw, lang = "") {
    const l = String(lang || "").toLowerCase().trim();
    const escaped = esc(raw);
    if (!l || l === "text" || l === "txt" || l === "plain") return escaped;

    const kwByLang = {
      js: /\b(?:async|await|break|case|catch|class|const|continue|default|delete|do|else|export|extends|false|finally|for|from|function|if|import|in|instanceof|let|new|null|of|return|static|super|switch|this|throw|true|try|typeof|undefined|var|void|while|yield)\b/g,
      rust: /\b(?:as|async|await|break|const|continue|crate|dyn|else|enum|extern|false|fn|for|if|impl|in|let|loop|match|mod|move|mut|pub|ref|return|self|Self|static|struct|super|trait|true|type|unsafe|use|where|while)\b/g,
      py: /\b(?:and|as|assert|async|await|break|class|continue|def|del|elif|else|except|False|finally|for|from|global|if|import|in|is|lambda|None|nonlocal|not|or|pass|raise|return|True|try|while|with|yield)\b/g,
      sql: /\b(?:SELECT|FROM|WHERE|JOIN|LEFT|RIGHT|INNER|OUTER|ON|GROUP|BY|ORDER|LIMIT|INSERT|INTO|VALUES|UPDATE|SET|DELETE|CREATE|TABLE|ALTER|DROP|AS|AND|OR|NOT|NULL|IN|IS|LIKE|DISTINCT|HAVING|UNION|ALL|CASE|WHEN|THEN|ELSE|END)\b/gi,
      sh: /\b(?:if|then|else|elif|fi|for|do|done|while|case|esac|in|function|local|export|return|exit|echo|cd|set)\b/g,
    };
    const family = ["js", "mjs", "cjs", "ts", "tsx", "jsx"].includes(l) ? "js"
      : ["rs", "rust"].includes(l) ? "rust"
      : ["py", "python"].includes(l) ? "py"
      : ["sql", "postgres", "psql"].includes(l) ? "sql"
      : ["sh", "bash", "zsh", "shell"].includes(l) ? "sh"
      : l === "json" ? "json" : "";
    if (!family) return escaped;

    // Single-pass tokenizing on escaped HTML so we never match inside our own spans.
    const tokenRe = family === "json"
      ? /(&quot;(?:\\.|[^&]|&(?!quot;))*?&quot;(?=\s*:))|(&quot;(?:\\.|[^&]|&(?!quot;))*?&quot;)|\b(true|false|null)\b|\b(-?\d+(?:\.\d+)?)\b/g
      : family === "py" || family === "sh"
        ? /(#[^\n]*)|(&quot;(?:\\.|[^&]|&(?!quot;))*?&quot;|&#39;(?:\\.|[^&]|&(?!#39;))*?&#39;)|\b(\d+(?:\.\d+)?)\b/g
        : family === "sql"
          ? /(--[^\n]*)|(&#39;(?:[^&]|&(?!#39;))*?&#39;)|\b(\d+(?:\.\d+)?)\b/g
          : /(\/\/[^\n]*|\/\*[\s\S]*?\*\/)|(&quot;(?:\\.|[^&]|&(?!quot;))*?&quot;|&#39;(?:\\.|[^&]|&(?!#39;))*?&#39;|`[^`]*`)|\b(\d+(?:\.\d+)?)\b/g;

    const kwRe = kwByLang[family];
    let out = "", last = 0;
    for (const m of escaped.matchAll(tokenRe)) {
      const plain = escaped.slice(last, m.index);
      out += kwRe ? plain.replace(kwRe, k => `<span class="tok-kw">${k}</span>`) : plain;
      last = m.index + m[0].length;
      if (family === "json") {
        if (m[1]) out += `<span class="tok-key">${m[1]}</span>`;
        else if (m[2]) out += `<span class="tok-str">${m[2]}</span>`;
        else if (m[3]) out += `<span class="tok-kw">${m[3]}</span>`;
        else out += `<span class="tok-num">${m[4]}</span>`;
      } else if (m[1]) out += `<span class="tok-com">${m[1]}</span>`;
      else if (m[2]) out += `<span class="tok-str">${m[2]}</span>`;
      else out += `<span class="tok-num">${m[3]}</span>`;
    }
    const tail = escaped.slice(last);
    return out + (kwRe ? tail.replace(kwRe, k => `<span class="tok-kw">${k}</span>`) : tail);
  }

  /** Serialize an HTML table to RFC 4180 CSV. @param {HTMLTableElement} table */
  function tableToCsv(table) {
    const cell = el => {
      const t = (el.textContent || "").trim().replace(/\s+/g, " ");
      return /[",\n]/.test(t) ? `"${t.replace(/"/g, '""')}"` : t;
    };
    return [...table.querySelectorAll("tr")].map(tr => [...tr.querySelectorAll("th, td")].map(cell).join(",")).join("\n");
  }

  /** Sort a table by column index (numeric-aware). @param {HTMLTableElement} table @param {number} col @param {boolean} asc */
  function sortTable(table, col, asc) {
    const tbody = table.querySelector("tbody");
    if (!tbody) return;
    const rows = [...tbody.querySelectorAll("tr")];
    const num = s => {
      const cleaned = s.replace(/[$€£,%\s]/g, "");
      return cleaned !== "" && Number.isFinite(Number(cleaned)) ? Number(cleaned) : null;
    };
    rows.sort((a, b) => {
      const av = (a.children[col]?.textContent || "").trim();
      const bv = (b.children[col]?.textContent || "").trim();
      const an = num(av), bn = num(bv);
      const cmp = an !== null && bn !== null ? an - bn : av.localeCompare(bv, undefined, { numeric: true, sensitivity: "base" });
      return asc ? cmp : -cmp;
    });
    for (const r of rows) tbody.append(r);
  }

  /** Wire client-side table sorting, CSV copy, and native chart hover tooltips. @param {HTMLElement} root */
  function wireInteractive(root) {
    if (!root || root._richInteractiveWired) return;
    root._richInteractiveWired = true;
    root.addEventListener("click", async e => {
      const t = /** @type {HTMLElement} */ (e.target);
      const csvBtn = t.closest?.("[data-table-csv]");
      if (csvBtn) {
        const tbl = csvBtn.closest(".markdown-table, figure")?.querySelector("table");
        if (tbl && typeof copyText === "function") {
          e.preventDefault();
          try { await copyText(tableToCsv(tbl)); if (typeof toast === "function") toast("Table copied as CSV."); }
          catch (err) { if (typeof toast === "function") toast(err.message); }
        }
        return;
      }
      const th = t.closest?.("th[data-sort-col]");
      if (th) {
        const tbl = th.closest("table");
        if (!tbl) return;
        const col = Number(th.dataset.sortCol);
        const asc = th.dataset.sortDir !== "asc";
        tbl.querySelectorAll("th[data-sort-col]").forEach(h => {
          /** @type {HTMLElement} */ (h).dataset.sortDir = "";
          h.setAttribute("aria-sort", "none");
        });
        th.dataset.sortDir = asc ? "asc" : "desc";
        th.setAttribute("aria-sort", asc ? "ascending" : "descending");
        sortTable(tbl, col, asc);
      }
    });
    root.addEventListener("pointerover", e => {
      const target = /** @type {HTMLElement} */ (e.target).closest?.("[data-chart-tip]");
      if (!target) return;
      const fig = target.closest("figure");
      if (!fig) return;
      let tip = /** @type {?HTMLElement} */ (fig.querySelector(".chart-tooltip"));
      if (!tip) {
        tip = document.createElement("div");
        tip.className = "chart-tooltip";
        tip.setAttribute("role", "status");
        fig.append(tip);
      }
      tip.textContent = target.dataset.chartTip || "";
      tip.hidden = false;
    });
    root.addEventListener("pointerout", e => {
      const target = /** @type {HTMLElement} */ (e.target).closest?.("[data-chart-tip]");
      if (!target) return;
      const tip = /** @type {?HTMLElement} */ (target.closest("figure")?.querySelector(".chart-tooltip"));
      if (tip) tip.hidden = true;
    });
  }

  return { htmlToMarkdown, highlightCode, tableToCsv, sortTable, wireInteractive };
})();
