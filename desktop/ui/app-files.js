// File chips, uploads, previews, glimpses, project move controls, and outcome
// verdict bars (extracted from app.js in Tier 3 Phase 4 to shrink app.js).
"use strict";

// A file wherever it is listed: its name opens it, Download saves it, and text files can open in Split Co-Edit.
function fileChip(f) {
  const editable = /\.(md|txt|json|csv|ya?ml|toml|js|ts|py|rs|html?|css)$/i.test(f.name || "") || /^text\//i.test(f.media_type || "");
  const split = editable ? ` <button type="button" class="link small" data-file-coedit="${esc(f.id)}" data-name="${esc(f.name)}" aria-label="Edit ${esc(f.name)} in Split Co-Edit">Split edit</button>` : "";
  return `<span class="fchip"><a href="#/file/${enc(f.id)}" data-open="file:${esc(f.id)}">${esc(f.name)}</a> <span class="small faint">${esc(kb(f.size))}</span>${split} <button class="link small" data-file-download="${esc(f.id)}" data-name="${esc(f.name)}" aria-label="Download ${esc(f.name)}">Download</button></span>`;
}

// One tap records the person's verdict on an outcome; a note is optional.
const VERDICTS = [["right", "Right"], ["partly", "Partly"], ["wrong", "Wrong"], ["undone", "I had to undo it"]];
function verdictBar(taskId, v) {
  return `<div class="verdict" data-verdict-for="${esc(taskId)}"><span class="vq">How did Magnus do?</span>
    ${VERDICTS.map(([k, label]) => `<button class="${v?.verdict === k ? "on" : ""}" data-verdict="${k}" aria-pressed="${v?.verdict === k}">${label}</button>`).join("")}
    <details class="vnote" data-key="note:${esc(taskId)}"${v?.note ? " open" : ""}><summary class="small">${v?.note ? "Note" : "Add a note"}</summary>
    <textarea rows="2" aria-label="Note" data-draft="note:${esc(taskId)}">${esc(v?.note || "")}</textarea> <button data-verdict-save>Save note</button></details></div>`;
}

// Where a library file came from, as a link.
const fileFrom = f => f.origin.kind === "task" ? `<a href="#/task/${enc(f.origin.task_id)}" data-open="task:${esc(f.origin.task_id)}">${esc(f.origin.title)}</a>`
  : f.origin.kind === "chat" ? `<a href="#/chat/${enc(f.origin.thread)}">Chat: ${esc(f.origin.title || "conversation")}</a>` : `<span class="faint">Uploaded to Files</span>`;

function readFile(file) {
  return new Promise((ok, no) => {
    const r = new FileReader();
    r.onload = () => ok({ name: file.name, media_type: file.type || "application/octet-stream", data_b64: String(r.result).slice(String(r.result).indexOf(",") + 1) });
    r.onerror = () => no(new Error(`Could not read ${file.name}`));
    r.readAsDataURL(file);
  });
}

// Upload chosen files to the library (into a project, or in a thread).
async function upload(list, where = {}) {
  const out = [];
  for (const file of list) out.push(await Server.files.upload({ ...(await readFile(file)), ...where }));
  return out;
}

function projectOptions(projects, current, none = "No project") {
  return `<option value="">${esc(none)}</option>${projects.map(p => `<option value="${esc(p.id)}"${p.id === current ? " selected" : ""}>${esc(p.name)}</option>`).join("")}`;
}

// A control that moves an item into one of the person's projects, or out.
function moveControl(item, current, projects, compact = false) {
  return `<label class="small move"><span class="${compact ? "sr" : "muted"}">Project</span> <select data-move="${esc(item)}" aria-label="Project">${projectOptions(projects, current?.id)}</select></label>`;
}

// A file input behind a button; `onFiles` gets the chosen files.
function attachButton(label = "Add files", iconOnly = false) {
  return `<span class="attachment-picker"><button type="button" class="${iconOnly ? "icon-btn" : "button"} attach" data-attach-open aria-label="${esc(label)}" title="${esc(label)}">${iconOnly ? icon("attach") + `<span class="sr">${esc(label)}</span>` : esc(label)}</button><input type="file" multiple hidden data-attach></span>`;
}

// A file shown where it is mentioned: a glimpse of it (an image, or the
// first lines of a text file), its name to open it beside the conversation,
// and Download.
const GLIMPSE_IMAGE = /^image\/(png|jpeg|gif|webp)$/;
const glimpses = {}, pageTexts = {};
const isPageFile = f => /^text\/html\b/i.test(f.media_type || "") || /\.html?$/i.test(f.name || "");
function filePreview(f, live) {
  if (isPageFile(f)) return live instanceof Set && live.has(f.name) ? `<div class="fprev">${fileChip(f)}</div>`
    : `<div class="fprev fpage" data-preview="${esc(f.id)}" data-live-file="${esc(f.id)}" data-name="${esc(f.name)}" data-type="${esc(f.media_type || "text/html")}">${fileChip(f)}</div>`;
  const can = (GLIMPSE_IMAGE.test(f.media_type) && f.size <= 4 << 20) || (Compose.TEXT.test(f.media_type) && f.size <= 256 << 10);
  const g = !can ? `<span class="small faint">${esc(f.media_type)}</span>` : glimpses[f.id] ?? `<span class="small faint">Loading a preview…</span>`;
  return `<div class="fprev" data-preview="${esc(f.id)}"${can && !(f.id in glimpses) ? ` data-glimpse="${esc(f.media_type)}"` : ""}>
    <a class="glimpse" href="#/file/${enc(f.id)}" data-open="file:${esc(f.id)}" aria-hidden="true" tabindex="-1">${g}</a>${fileChip(f)}</div>`;
}

// The page files in a deliverable that its view already runs live.
const livePages = e => new Set((e.view || []).filter(c => c.type === "file" && isPageFile({ name: c.file })).map(c => c.file));
async function drawPageCards(root) {
  await Promise.all($$("[data-live-file]", root).filter(el => !el.querySelector(".live-page")).map(async el => {
    const id = el.dataset.liveFile;
    try {
      pageTexts[id] ??= decodeText(await Magnus.dataUrl(`/v1/files/${enc(id)}`, el.dataset.type));
      if (el.isConnected && !el.querySelector(".live-page")) el.prepend(livePage(el.dataset.name || "Page", pageTexts[id]));
    } catch (e) { if (!el.querySelector(".bad")) el.insertAdjacentHTML("afterbegin", `<div class="small bad">This page could not be drawn: ${esc(e.message)}</div>`); }
  }));
}

if (typeof document !== "undefined") {
  document.addEventListener("click", async e => {
    const btn = /** @type {?HTMLElement} */ (/** @type {HTMLElement} */ (e.target).closest?.("[data-file-coedit]"));
    if (!btn || typeof globalThis.SplitCoedit === "undefined") return;
    const id = btn.dataset.fileCoedit;
    const name = btn.dataset.name || "Document";
    let content = "";
    try {
      if (id && typeof Magnus !== "undefined" && typeof Magnus.dataUrl === "function") {
        content = decodeText(await Magnus.dataUrl(`/v1/files/${enc(id)}`, "text/plain"));
      }
    } catch { /* open empty draft if offline */ }
    globalThis.SplitCoedit.open({ title: name, content });
  });
}
async function loadGlimpses(root) {
  drawPageCards(root);
  const want = $$("[data-glimpse]", root).filter(el => !(el.dataset.preview in glimpses));
  if (!want.length) return;
  for (const el of want) glimpses[el.dataset.preview] = undefined;
  await Promise.all(want.map(async el => {
    const id = el.dataset.preview, type = el.dataset.glimpse;
    try {
      const url = await Magnus.dataUrl(`/v1/files/${enc(id)}`, type);
      glimpses[id] = GLIMPSE_IMAGE.test(type) ? `<img alt="" src="${esc(url)}">` : `<pre>${esc(decodeText(url).split("\n").slice(0, 8).join("\n"))}</pre>`;
    } catch { glimpses[id] = `<span class="small faint">No preview</span>`; }
  }));
  await Promise.all([App.redraw?.(), drawPanel()]);
}
