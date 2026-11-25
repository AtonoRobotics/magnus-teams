// Composed frames, sandboxed UI blocks, Markdown image hydration, and frame action overlays.

function loadMarkdownImages(root) {
  drawUiBlocks(root);
  for (const img of root.querySelectorAll("img[data-prose-image]")) {
    if (img._loading) continue; img._loading = true;
    const path = img.dataset.proseImage;
    if (!/^\/v1\/files\/[^/?#]+$/.test(path)) continue;
    Magnus.dataUrl(path).then(src => { if (img.isConnected) img.src = src; }).catch(() => { img.alt = `${img.alt || "Image"} (unavailable)`; });
  }
}
// An answer still being written: prose is redrawn, while a block keeps its
// frame, so what is already drawn stays put as more arrives.
function patchLive(el, text) {
  const parts = UiSpec.split(text);
  [...el.children].slice(parts.length).forEach(c => c.remove());
  parts.forEach((p, i) => {
    const html = p.kind === "text";
    let node = el.children[i];
    if (!node || node.classList.contains("ui-block") === html) {
      const made = document.createElement("div");
      if (node) node.replaceWith(made); else el.append(made);
      node = made;
    }
    if (html) { node.innerHTML = Compose.markdown(p.text); return; }
    node.className = "ui-block";
    Object.assign(node.dataset, { uiKind: p.kind, uiId: p.id || "", uiBody: p.body, uiClosed: p.closed ? "1" : "0" });
  });
}
// Wire table Copy/CSV export and deck PPTX/PDF export inside any composed frame.
function wireFrameExports(frame, docId) {
  const doc = frame.contentDocument;
  if (!doc) return;
  doc.querySelectorAll(".deck-export").forEach(b => {
    if (!docId) { b.style.display = "none"; return; }
    b.addEventListener("click", async () => {
      const fmt = b.dataset.format;
      const deckTitle = b.closest(".deck")?.querySelector(".fname")?.textContent || docId;
      const slug = deckTitle.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "") || docId;
      b.disabled = true;
      try {
        await Magnus.download(`/v1/ui/docs/${enc(docId)}/export/${enc(fmt)}`, `${slug}.${fmt}`);
        toast(`Exported ${slug}.${fmt}.`);
      } catch (err) { toast(err.message); }
      b.disabled = false;
    });
  });
  const rowsOf = wrap => [...wrap.querySelectorAll("tr")].map(tr => [...tr.querySelectorAll("th,td")].map(c => c.textContent.trim()));
  const csvCell = s => /[",\r\n]/.test(s) ? `"${s.replace(/"/g, '""')}"` : s;
  doc.querySelectorAll(".table-copy").forEach(b => b.addEventListener("click", async () => {
    const wrap = b.closest(".tw");
    const tsv = wrap ? rowsOf(wrap).map(r => r.join("\t")).join("\n") : "";
    if (tsv && navigator.clipboard?.writeText) { await navigator.clipboard.writeText(tsv); toast("Table copied."); }
  }));
  doc.querySelectorAll(".table-csv").forEach(b => b.addEventListener("click", () => {
    const wrap = b.closest(".tw");
    const csv = wrap ? rowsOf(wrap).map(r => r.map(csvCell).join(",")).join("\r\n") : "";
    if (!csv) return;
    const blob = new Blob([csv], { type: "text/csv;charset=utf-8" });
    const url = URL.createObjectURL(blob);
    const link = Object.assign(document.createElement("a"), { href: url, download: "table.csv" });
    document.body.append(link); link.click(); link.remove();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
    toast("Exported table.csv.");
  }));
}
// A spec block in the agent's words is drawn in a frame of its own: Compose
// makes the document, and the frame's scripts stay off. HTML the agent wrote
// runs in a sandbox with an opaque origin, under a policy set before it.
function drawUiBlocks(root) {
  if ((root.ownerDocument || root) !== document) return;
  root.querySelectorAll(".ui-block").forEach((block, n) => {
    const body = block.dataset.uiBody, closed = block.dataset.uiClosed === "1", kind = block.dataset.uiKind, docId = block.dataset.uiId || "";
    if (block._drawn === body + closed) return;
    block._drawn = body + closed;
    if (kind === "html") {
      if (!closed) { block.innerHTML = `<div class="small faint">Writing…</div>`; return; }
      const editBtn = docId ? `<button type="button" class="link small" data-ui-edit-block="${esc(docId)}">Edit with Magnus</button>` : "";
      block.innerHTML = `<div class="row small" style="justify-content:flex-end;margin-bottom:4px">${editBtn}<button type="button" class="link small" data-html-fullscreen>Fullscreen</button></div><iframe class="composed ui-html" data-auto-size="1" sandbox="allow-scripts" title="Made by Magnus"></iframe>`;
      const frame = block.querySelector("iframe");
      block.querySelector("[data-html-fullscreen]")?.addEventListener("click", () => frame?.requestFullscreen?.().catch(() => {}));
      block.querySelector("[data-ui-edit-block]")?.addEventListener("click", () => {
        const box = $("#msg");
        if (box) { box.value = `Update block ${docId}: `; box.dispatchEvent(new Event("input", { bubbles: true })); box.focus(); }
      });
      Magnus.sandboxUrl(HtmlFrame.wrap(body, ...look())).then(url => { if (frame?.isConnected) frame.src = url; }, e => { block.innerHTML = `<div class="small bad">This could not be drawn: ${esc(e.message)}</div>`; });
      return;
    }
    let view;
    try { view = closed ? UiSpec.compile(body) : UiSpec.compilePartial(body); }
    catch (e) { block.innerHTML = `<div class="small bad">This could not be drawn: ${esc(e.message)}</div>`; return; }
    if (!view.length) { block.innerHTML = `<div class="small faint">Drawing…</div>`; return; }
    if (closed) view.filter(c => c.type === "app").forEach(c => drawAppView(block, c.view));
    if (view.every(c => c.type === "app")) return;
    const drawn = JSON.stringify(view);
    if (block._view === drawn) return;
    block._view = drawn;
    const files = {};
    const editBar = docId ? `<div class="row small" style="justify-content:flex-end;margin-bottom:4px"><button type="button" class="link small" data-ui-edit-block="${esc(docId)}">Edit with Magnus</button></div>` : "";
    const wireEditBtn = () => {
      block.querySelector("[data-ui-edit-block]")?.addEventListener("click", () => {
        const box = $("#msg");
        if (box) { box.value = `Update block ${docId}: `; box.dispatchEvent(new Event("input", { bubbles: true })); box.focus(); }
      });
    };
    const needsFrame = view.some(c => c.type === "deck");
    if (!needsFrame && typeof Compose.renderNative === "function") {
      const paintNative = () => {
        block.innerHTML = `${editBar}${Compose.renderNative(view, files)}`;
        wireEditBtn();
        if (closed) wireNativeBlock(block, view, n);
        if (typeof RichText !== "undefined") RichText.wireInteractive(block);
      };
      paintNative();
      if (closed) view.filter(c => c.placeholder).forEach(c => waitForImage(c.placeholder, files, block, paintNative));
      return;
    }
    if (!block.querySelector("iframe.composed")) {
      block.innerHTML = `${editBar}<iframe class="composed" sandbox="allow-same-origin" title="What Magnus showed" data-sized=""></iframe>`;
      wireEditBtn();
    }
    const frame = block.querySelector("iframe.composed");
    const paint = () => { frame.srcdoc = Compose.documentFor(view, files, fontCss(), undefined, ...look()); };
    paint();
    frame.onload = () => { sizeComposedFrame(frame); wireFrameExports(frame, docId); if (closed) wireBlock(frame, view, block, n); };
    if (closed) view.filter(c => c.placeholder).forEach(c => waitForImage(c.placeholder, files, block, paint));
  });
}
// Wire interactive action/form buttons on a native DOM magnus-ui block.
function wireNativeBlock(block, view, fence) {
  const task = block.closest("[data-task]")?.dataset.task;
  if (!task) return;
  const at = kind => view.flatMap((c, i) => c.type === kind ? [i] : []);
  const send = (index, values, b) => { b.disabled = true; Server.tasks.action(task, index, { fence, values }).then(() => toast("Started as a new request")).catch(e => { b.disabled = false; toast(e.message); }); };
  block.querySelectorAll("button.action:not(.form-send)").forEach((b, n) => b.addEventListener("click", () => send(at("action")[n], {}, b)));
  block.querySelectorAll(".uiform").forEach((form, n) => form.querySelector(".form-send")?.addEventListener("click", e => {
    if (![...form.querySelectorAll("input,select,textarea")].every(i => i.reportValidity())) return;
    send(at("form")[n], Object.fromEntries([...form.querySelectorAll("[name]")].map(i => [i.name, i.value])), e.currentTarget);
  }));
}
// What is pressed is read again by the server from what the task holds.
function wireBlock(frame, view, block, fence) {
  const task = block.closest("[data-task]")?.dataset.task;
  const doc = frame.contentDocument;
  doc?.querySelectorAll(".deck-present").forEach(b => b.addEventListener("click", () => presentDeck(frame)));
  if (!task) return;
  const at = kind => view.flatMap((c, i) => c.type === kind ? [i] : []);
  const send = (index, values, b) => { b.disabled = true; Server.tasks.action(task, index, { fence, values }).then(() => toast("Started as a new request")).catch(e => { b.disabled = false; toast(e.message); }); };
  doc?.querySelectorAll("button.action:not(.form-send)").forEach((b, n) => b.addEventListener("click", () => send(at("action")[n], {}, b)));
  doc?.querySelectorAll(".uiform").forEach((form, n) => form.querySelector(".form-send").addEventListener("click", e => {
    if (![...form.querySelectorAll("input,select,textarea")].every(i => i.reportValidity())) return;
    send(at("form")[n], Object.fromEntries([...form.querySelectorAll("[name]")].map(i => [i.name, i.value])), e.currentTarget);
  }));
}
// Present mode: the same frame full screen, one slide at a time.
function presentDeck(frame) {
  const doc = frame.contentDocument, slides = [...doc.querySelectorAll(".slide")];
  if (!slides.length) return;
  let at = 0;
  const show = () => slides.forEach((s, i) => s.classList.toggle("on", i === at));
  const end = () => { doc.body.classList.remove("presenting"); doc.removeEventListener("keydown", key); document.removeEventListener("keydown", key); document.removeEventListener("fullscreenchange", left); if (document.fullscreenElement === frame) document.exitFullscreen().catch(() => {}); sizeComposedFrame(frame); };
  const left = () => { if (document.fullscreenElement !== frame) end(); };
  const key = e => { if (e.key === "ArrowRight" || e.key === " ") { at = Math.min(at + 1, slides.length - 1); show(); } else if (e.key === "ArrowLeft") { at = Math.max(at - 1, 0); show(); } else if (e.key === "Escape") end(); };
  doc.body.classList.add("presenting");
  show();
  doc.addEventListener("keydown", key); document.addEventListener("keydown", key); document.addEventListener("fullscreenchange", left);
  frame.requestFullscreen?.().catch(() => {});
}
// A connector's view, in the same sandbox as the agent's own HTML. It talks
// over postMessage; a tool it asks for starts a new request.
async function drawAppView(block, id) {
  if (block.querySelector(`iframe[data-view="${CSS.escape(id)}"]`)) return;
  const frame = document.createElement("iframe");
  Object.assign(frame, { className: "composed ui-html", title: "A connector's view" });
  frame.setAttribute("sandbox", "allow-scripts");
  frame.dataset.view = id;
  block.append(frame);
  let v;
  try { v = await Server.ui.app(id); }
  catch { frame.replaceWith(Object.assign(document.createElement("div"), { className: "small bad", textContent: "This view is no longer available." })); return; }
  const send = m => frame.contentWindow?.postMessage({ jsonrpc: "2.0", ...m }, "*");
  const onMessage = e => {
    if (!frame.isConnected) return window.removeEventListener("message", onMessage);
    if (e.source !== frame.contentWindow || !e.data || e.data.jsonrpc !== "2.0") return;
    const { id: rid, method, params } = e.data;
    if (method === "ui/initialize") send({ id: rid, result: { protocolVersion: params?.protocolVersion, hostInfo: { name: "Magnus", version: "1" }, hostCapabilities: {}, hostContext: {} } });
    else if (method === "ui/notifications/initialized") {
      send({ method: "ui/notifications/tool-input", params: { arguments: v.input } });
      send({ method: "ui/notifications/tool-result", params: { content: [{ type: "text", text: v.text }], structuredContent: v.structured ?? undefined } });
    } else if (method === "tools/call") Server.ui.appRequest(id, { tool: String(params?.name), arguments: params?.arguments ?? {} })
      .then(() => { toast("Started as a new request."); send({ id: rid, result: { content: [{ type: "text", text: "Started as a new request." }] } }); })
      .catch(err => send({ id: rid, error: { code: -32000, message: err.message || String(err) } }));
    else if (rid !== undefined) send({ id: rid, error: { code: -32601, message: "Not available here." } });
  };
  window.addEventListener("message", onMessage);
  frame.src = await Magnus.sandboxUrl(`<!doctype html>${v.html}`);
}
// An image being made: asked after until it is there, then drawn in place.
async function waitForImage(id, files, block, paint) {
  for (let tries = 0; tries < IMAGE_POLLS && block.isConnected; tries++) {
    try {
      const src = await Magnus.dataUrl(`/v1/ui/images/${enc(id)}`);
      if (src && !/^data:[^,]*,$/.test(src)) { files[id] = { image: src }; paint(); return; }
    } catch { files[id] = { failed: true }; paint(); return; }
    await new Promise(r => setTimeout(r, IMAGE_POLL_MS));
  }
}
const IMAGE_POLLS = 120, IMAGE_POLL_MS = 3000;
// Says what this client can draw, so the agent writes only that.
async function loadUiCatalog() {
  try {
    const c = await Server.ui.catalog();
    UiSpec.setCatalog(c);
    await Server.ui.setCatalog({ version: c.version, elements: c.elements.map(e => e.name) });
  } catch {/* Drawn as "could not be drawn" until it loads. */}
}
function wireMarkdown(root) {
  if (root._markdownWired) return; root._markdownWired = true;
  loadMarkdownImages(root);
  if (typeof RichText !== "undefined") RichText.wireInteractive(root);
  root.addEventListener("click", async e => {
    const copy = e.target.closest("[data-code-copy]"), link = e.target.closest("[data-prose-link]");
    if (copy) { e.preventDefault(); try { await copyText(copy.closest(".code-block").querySelector("code").textContent); toast("Code copied."); } catch (err) { toast(err.message); } }
    if (link) { e.preventDefault(); const url = Compose.safeUrl(link.getAttribute("href")); if (url) Magnus.openExternal(url).catch(err => toast(err.message)); }
  });
}


const fontCss = () => {
  const u = f => new URL(`fonts/${f}`, location.href).href;
  return `@font-face{font-family:Inter;font-weight:100 900;src:url(${u("inter-latin-wght-normal.woff2")}) format("woff2")}`;
};
// A task's delivered files, the latest of each name.
const latestOutputs = t => Object.values(Object.fromEntries((t?.outputs || []).map(o => [o.name, o])));
const decodeText = u => new TextDecoder().decode(Uint8Array.from(atob(u.slice(u.indexOf(",") + 1)), c => c.charCodeAt(0)));
// The delivered files a composition shows, fetched by the app itself:
// images as data: URLs, text files decoded for drawing.
async function fileData(view, t) {
  const out = {};
  const names = new Set((view || []).filter(c => c.type === "image" || c.type === "file").map(c => c.file));
  for (const o of latestOutputs(t).filter(o => names.has(o.name))) {
    const path = `/v1/tasks/${enc(t.task_id)}/outputs/${enc(o.id)}`;
    try {
      if (/^image\/(png|jpeg|gif|webp)$/.test(o.media_type)) out[o.name] = { image: await Magnus.dataUrl(path, o.media_type) };
      else if (Compose.TEXT.test(o.media_type)) out[o.name] = { text: decodeText(await Magnus.dataUrl(path, o.media_type)), media_type: o.media_type };
    } catch { /* shown as missing */ }
  }
  return out;
}
// The look in effect, for what a frame draws: its tokens, and light or dark.
const look = () => { const l = window.Theme?.look; return l ? [Theme.tokens(l), l.mode] : []; };

// The height each composed frame in a conversation last had, by its entry:
// a redraw of the thread makes the frame anew, and drawn at its known height
// the thread keeps its layout (and its scroll) while the frame loads again.
const frameHeights = {};
// (Set after the thread is drawn, not in its markup, so the markup stays the
// same and a loaded frame does not make the thread draw itself again.)
const shownFrame = (id, title) => `<iframe class="composed" sandbox="allow-same-origin" title="${title}" data-shown="${esc(id)}"></iframe>`;
const keepHeights = el => $$("iframe[data-shown]", el).forEach(f => { if (frameHeights[f.dataset.shown]) f.style.height = frameHeights[f.dataset.shown] + "px"; });
const frameSizing = new Map();
// Native WebKit blocks frame event handlers when scripts are sandboxed. Keep
// actions in the trusted parent, aligned to inert reading-surface markers.
function alignFrameActions(frame,state) {
  if(!frame.isConnected || frame.contentDocument!==state.doc)return;
  const clips=new Map(), geometry=[];
  for(const control of state.controls) {
    const marker=control._marker;if(!marker.isConnected)continue;
    const r=marker.getBoundingClientRect();
    let left=0,top=0,right=frame.clientWidth,bottom=frame.clientHeight;
    for(let node=marker.parentElement;node;node=node.parentElement) {
      let clip=clips.get(node);
      if(!clip){const style=state.doc.defaultView.getComputedStyle(node),box=node.getBoundingClientRect();clip={x:/auto|scroll|hidden|clip/.test(style.overflowX),y:/auto|scroll|hidden|clip/.test(style.overflowY),left:box.left+node.clientLeft,top:box.top+node.clientTop,right:box.left+node.clientLeft+node.clientWidth,bottom:box.top+node.clientTop+node.clientHeight};clips.set(node,clip);}
      if(clip.x){left=Math.max(left,clip.left);right=Math.min(right,clip.right);}
      if(clip.y){top=Math.max(top,clip.top);bottom=Math.min(bottom,clip.bottom);}
    }
    // Keep the 2px outline and 2px offset visible inside the scrollport.
    const visible=Math.min(right,r.right)>Math.max(left,r.left) && Math.min(bottom,r.bottom)>Math.max(top,r.top);
    geometry.push([control,{left:`${r.left}px`,top:`${r.top}px`,width:`${r.width}px`,height:`${r.height}px`,clipPath:visible?`inset(${Math.max(-4,top-r.top)}px ${Math.max(-4,r.right-right)}px ${Math.max(-4,r.bottom-bottom)}px ${Math.max(-4,left-r.left)}px)`:'inset(50%)'}]);
  }
  // Read frame geometry together before writing parent overlay styles.
  for(const [control,style] of geometry)Object.assign(control.style,style);
  state.scrollports=new Map([...clips].filter(([,clip])=>clip.x || clip.y).map(([node])=>[node,[node.scrollLeft,node.scrollTop]]));
  const root=state.doc.scrollingElement;if(root)state.scrollports.set(root,[root.scrollLeft,root.scrollTop]);
}
function frameActions(frame,doc) {
  let wrapper=frame.parentElement;
  if(!wrapper.classList.contains('composed-wrap')) {
    wrapper=document.createElement('div');wrapper.className='composed-wrap';frame.before(wrapper);wrapper.append(frame);
  }
  let state=frame._frameActions;
  if(state?.doc!==doc || state?.body!==doc.body) {
    state?.dispose();wrapper.querySelector('.frame-actions')?.remove();
    const layer=document.createElement('div');layer.className='frame-actions';wrapper.append(layer);
    let scheduled=0,poll=0,disposed=false,visible=false,visibility;
    state={doc,body:doc.body,controls:[],scrollports:new Map()};
    const align=()=>alignFrameActions(frame,state);
    const schedule=()=>{if(!disposed && !scheduled)scheduled=requestAnimationFrame(()=>{scheduled=0;align();});};
    for(const marker of doc.querySelectorAll('[data-code-copy-anchor], [data-prose-url]')) {
      const copy=marker.hasAttribute('data-code-copy-anchor'),control=document.createElement(copy?'button':'a');
      control.className=copy?'frame-code-copy':'frame-prose-link';
      if(copy){control.type='button';control.dataset.codeCopy='';control.textContent='Copy code';control.setAttribute('aria-label','Copy code');control.dataset.tooltip='Copy code';
        const text=marker.closest('.code-block').querySelector('code').textContent;
        control.onclick=async e=>{e.preventDefault();e.stopPropagation();Tooltips.dismiss();try{await copyText(text);toast('Code copied.');}catch(err){toast(err.message);}};
      }else{control.textContent=marker.textContent;control.setAttribute('aria-label',marker.textContent);control.href=marker.dataset.proseUrl;control.rel='noopener noreferrer';control.onclick=e=>{e.preventDefault();Magnus.openExternal(control.href).catch(err=>toast(err.message));};}
      marker.style.scrollMargin='4px';
      control.onfocus=()=>{marker.scrollIntoView({block:'nearest',inline:'nearest'});align();};
      layer.append(control);control._marker=marker;state.controls.push(control);
    }
    const pollScroll=()=>{
      poll=0;if(disposed || !visible || !frame.isConnected)return;
      // Sandboxed WebKit may omit subdocument scroll events. Checking the
      // few scrollport offsets is cheap; measure links only after movement.
      if(!document.hidden && [...state.scrollports].some(([node,at])=>node.scrollLeft!==at[0] || node.scrollTop!==at[1]))align();
      poll=requestAnimationFrame(pollScroll);
    };
    if(state.controls.length){
      doc.addEventListener('scroll',schedule,true);doc.fonts?.addEventListener('loadingdone',schedule);doc.fonts?.ready.then(schedule);
      if('IntersectionObserver' in window){visibility=new IntersectionObserver(entries=>{if(disposed)return;visible=entries[0].isIntersecting;if(visible){align();if(!poll)poll=requestAnimationFrame(pollScroll);}else{cancelAnimationFrame(poll);poll=0;}});visibility.observe(frame);}
      else{visible=true;poll=requestAnimationFrame(pollScroll);}
    }
    state.dispose=()=>{disposed=true;cancelAnimationFrame(scheduled);cancelAnimationFrame(poll);visibility?.disconnect();doc.removeEventListener('scroll',schedule,true);doc.fonts?.removeEventListener('loadingdone',schedule);};
    frame._frameActions=state;
  }
  alignFrameActions(frame,state);
}
function sizeComposedFrame(frame) {
  try {
    const doc=frame.contentDocument;
    if(!frame.isConnected || !frame.hasAttribute("srcdoc") || !doc?.body || doc.URL==="about:blank")return;
    let sizing=frameSizing.get(frame);
    if(sizing?.doc!==doc || sizing?.body!==doc.body) {
      sizing?.observer.disconnect();
      const measure=()=>{
        if(!frame.isConnected){frameSizing.get(frame)?.observer.disconnect();frame._frameActions?.dispose();delete frame._frameActions;frameSizing.delete(frame);return;}
        if(frame.contentDocument!==doc)return;
        const height=Math.ceil(Math.max(doc.body.scrollHeight,doc.body.getBoundingClientRect().height))+4;
        if(frame.style.height!==`${height}px`)frame.style.height=`${height}px`;
        if(frame.dataset.shown)frameHeights[frame.dataset.shown]=height;
        frameActions(frame,doc);
        frame.dataset.sized="1";
      };
      const observer=new ResizeObserver(measure);observer.observe(doc.body);
      sizing={doc,body:doc.body,observer,measure};frameSizing.set(frame,sizing);
    }
    sizing.measure();
  }catch{/* A frame still loading cannot be measured yet. */}
}

async function fill(frame, view, taskId, task) {
  if (!frame || frame._drawn) return;
  frame._drawn = true;
  const t = task || (view?.some(c => c.type === "image" || c.type === "file") ? await Server.tasks.get(taskId) : null);
  frame.removeAttribute("data-sized");
  frame.srcdoc = Compose.documentFor(view, await fileData(view, t), fontCss(), t?.preview, ...look());
  frame.onload=()=>{wireMarkdown(frame.contentDocument);wireFrameExports(frame,"");wireActions(frame,view,taskId);sizeComposedFrame(frame);};
}

// A button in something the agent made starts a new request and approves nothing.
function wireActions(frame, view, taskId) {
  const at = (view || []).flatMap((c, i) => c.type === "action" ? [i] : []);
  frame.contentDocument?.querySelectorAll("button.action").forEach((b, n) => b.addEventListener("click", async () => {
    b.disabled = true;
    try { await Server.tasks.action(taskId, at[n]); toast("Started as a new request"); }
    catch (e) { b.disabled = false; toast(e.message); }
  }));
}
