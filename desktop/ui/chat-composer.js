// The one composer, for every conversation the chat view shows (chat-view.js):
// the personal chat, a shared conversation and its threads. Files added to
// the next message upload at once and are sent with it. What is typed and
// attached stays with its conversation while the person moves around the
// app, in memory only (MessageDrafts, chat-view.js; #544). Enter sends;
// Shift+Enter and an input method's Enter do not. In a conversation of
// several people, typing @ offers who to mention.

/**
 * What the composer needs from its conversation.
 * @typedef {Object} ComposerConversation
 * @property {(file: File, record: any) => Promise<{ id: string, name: string, media_type?: string, size?: number }>} upload
 * @property {(o: { text: string, files: any[], form: HTMLFormElement, record: any }) => Promise<?(() => Promise<any>)>} send what comes back runs once the box is cleared
 * @property {() => Promise<any>} refresh after a send whose outcome is unknown
 * @property {(partial: string) => Array<{ name: string, agent?: boolean }>} [mentions]
 * @property {() => void} [typed] someone is typing here
 * @property {(f: any) => Promise<any>} [verify] whether an attached file is still there
 */

/**
 * @param {string} placeholder @param {string} bar what sits at the start of the bar (the model picker, or "Only my Magnus sees this")
 * @param {{ side?: boolean, label?: string, mentions?: boolean }} [o]
 */
function composer(placeholder, bar, o = {}) {
  const id = n => o.side ? "" : ` id="${n}"`;
  const label = o.side ? `<textarea rows="1" placeholder="${esc(placeholder)}" aria-label="${esc(o.label || placeholder)}"></textarea>`
    : `<label for="msg" class="sr">Message</label><textarea id="msg" rows="1" placeholder="${esc(placeholder)}"></textarea>`;
  return `<form${id("say")} class="say">${label}${o.mentions ? `<div class="say-mentions" role="listbox" aria-label="Mention" hidden></div>` : ""}
    <div class="row small pending"${id("pending")}></div>
    <div class="say-bar">${bar}<span class="chip run" data-steer-pill hidden>Steering active task</span><span class="grow"></span>${attachButton("Attach files", true)}<button type="button" class="icon-btn voice-in" data-voice-in aria-label="Voice input" title="Speak message" aria-pressed="false">${icon("mic")}</button><button type="button" class="icon-btn say-stop" data-say-stop aria-label="Stop" title="Stop" hidden>${icon("stop")}</button><button class="send"${id("send")} aria-label="Send message" title="Send message">${icon("send")}<span class="sr">Send message</span></button></div></form>`;
}

/** The @word being typed at the caret, if any. @param {string} text @param {number} caret */
function mentionAt(text, caret) {
  const m = text.slice(0, caret).match(/(?:^|\s)@([\w.-]*)$/);
  return m ? m[1] : null;
}

/**
 * @param {HTMLFormElement} form @param {any} record the conversation's draft (MessageDrafts)
 * @param {ComposerConversation} conv
 */
function wireComposer(form, record, conv) {
  const box = $("textarea", form), pending = $(".pending", form), input = $("[data-attach]", form), send = $(".send", form), sideStop = $("[data-say-stop]", form), steerPill = $("[data-steer-pill]", form), epoch = MessageDrafts.epoch();
  let replacing = null, active = true, stopMode = false;
  box.value = record.text;
  const model = $("[data-model]", form);
  if (model && record.model && record.modelUnder === model.dataset.default && [...model.options].some(o => o.value === record.model)) model.value = record.model;
  const fit = () => { box.style.height = "auto"; box.style.height = `${Math.min(box.scrollHeight, innerHeight / 3)}px`; };
  const selected = () => { record.selection = [box.selectionStart, box.selectionEnd]; };
  const read = (selection = true) => { record.text = box.value; record.model = model?.value || ""; record.modelUnder = model?.dataset.default || ""; if (selection) selected(); };
  const fmtSize = n => !Number.isFinite(n) || n <= 0 ? "" : n < 1024 ? `${n} B` : n < 1048576 ? `${(n / 1024).toFixed(n < 10240 ? 1 : 0)} KB` : `${(n / 1048576).toFixed(1)} MB`;
  const chips = () => record.files.map(f => {
    const status = f.state === "ready" ? "Ready" : f.state === "uploading" ? "Uploading…" : f.state === "unavailable" ? "Unavailable" : "Failed";
    const size = fmtSize(f.size);
    return `<span class="attachment-chip" data-upload="${esc(f.uid)}"><span>${esc(f.name)}</span><span class="small muted">${esc(size ? `${size} · ${status}` : status)}</span>${f.error ? `<span class="small">${esc(f.error)}</span>` : ""}${["failed", "unavailable"].includes(f.state) ? `<button type="button" data-retry-upload="${esc(f.uid)}">Retry</button>` : ""}<button type="button" class="icon-btn" data-remove-upload="${esc(f.uid)}" aria-label="Remove ${esc(f.name)} from message" title="Remove ${esc(f.name)} from message">${icon("close")}</button></span>`;
  }).join("");
  // The record is shared by every composer mounted on this conversation:
  // a send that finishes after the person left and came back updates the
  // one now showing, and a send in flight stays in flight there too.
  const hasActiveRun = () => !!(conv.stopRun && (record.sending || record.pendingRun || record.runningIds?.length));
  const canStop = () => !!(hasActiveRun() && (record.sending || (!box.value.trim() && !record.files.length)));
  const canSteer = () => !!(conv.steerRun && record.runningIds?.length && !record.sending && box.value.trim() && !record.files.length);
  const triggerStop = async () => {
    if (!conv.stopRun || record.stopping) return;
    const ids = [...(record.runningIds || [])];
    if (!ids.length) {
      record.cancelRequested = true;
      MessageDrafts.changed();
      return;
    }
    record.stopping = true;
    MessageDrafts.changed();
    try {
      await conv.stopRun(ids);
      record.runningIds = [];
      record.pendingRun = false;
      record.cancelRequested = false;
    } finally {
      record.stopping = false;
      MessageDrafts.changed();
    }
  };
  const syncSend = () => {
    const nextStop = canStop();
    const steer = !nextStop && canSteer();
    const stopping = !!(record.stopping || record.cancelRequested);
    if (nextStop !== stopMode) {
      stopMode = nextStop;
      send.type = stopMode ? "button" : "submit";
      send.classList.toggle("is-stop", stopMode);
      send.innerHTML = stopMode ? `${icon("stop")}<span class="sr">Stop</span>` : `${icon("send")}<span class="sr">Send message</span>`;
    }
    if (stopMode) {
      send.setAttribute("aria-label", stopping ? "Stopping…" : "Stop");
      send.title = stopping ? "Stopping…" : "Stop (Esc)";
    } else if (steer) {
      send.setAttribute("aria-label", "Steer Magnus");
      send.title = "Steer active work";
    } else {
      send.setAttribute("aria-label", "Send message");
      send.title = "Send message";
    }
    send.classList.toggle("is-steer", steer);
    if (steerPill) steerPill.hidden = !steer;
    send.disabled = stopMode ? stopping : (!!record.sending || record.files.some(f => f.state !== "ready"));
    send.classList.toggle("is-empty", !stopMode && !box.value.trim() && !record.files.length);
    if (sideStop) {
      sideStop.hidden = !hasActiveRun() || stopMode;
      sideStop.disabled = stopping;
      sideStop.title = stopping ? "Stopping…" : "Stop active work";
    }
  };
  send.onclick = async e => {
    if (!stopMode) return;
    e.preventDefault();
    await triggerStop();
  };
  if (sideStop) sideStop.onclick = async e => { e.preventDefault(); await triggerStop(); };
  const show = () => {
    if (!active || !form.isConnected) return;
    if (box.value !== record.text) { box.value = record.text; fit(); }
    paint(pending, chips());
    syncSend();
  };
  syncSend();
  const unsubscribe = MessageDrafts.subscribe(show);
  async function startUpload(slot, file) {
    const currentEpoch = MessageDrafts.epoch(); slot.state = "uploading"; delete slot.error;
    MessageDrafts.originals.set(slot.uid, file); MessageDrafts.changed();
    try {
      const ref = await conv.upload(file, record);
      if (currentEpoch !== MessageDrafts.epoch() || !record.files.includes(slot)) return;
      Object.assign(slot, { id: ref.id, name: ref.name, media_type: ref.media_type, size: ref.size, state: "ready" });
    } catch (err) {
      if (currentEpoch !== MessageDrafts.epoch() || !record.files.includes(slot)) return;
      slot.state = "failed"; slot.error = err.message || "Upload failed. Retry or remove this file.";
    }
    MessageDrafts.changed();
  }
  const attach = list => {
    for (const [i, file] of [...list].entries()) {
      const slot = i === 0 && replacing ? record.files.find(f => f.uid === replacing) : null;
      const item = slot || { uid: MessageDrafts.uid(), name: file.name, media_type: file.type, size: file.size, state: "uploading" };
      if (!slot) record.files.push(item);
      item.name = file.name; startUpload(item, file);
    }
    replacing = null;
  };
  input.onchange = e => { const files = [...e.target.files]; e.target.value = ""; attach(files); };
  pending.onclick = e => {
    const remove = e.target.closest("[data-remove-upload]"), retry = e.target.closest("[data-retry-upload]");
    // Removing a file takes it off this message; the shared file stays in Files.
    if (remove) { const uid = remove.dataset.removeUpload; record.files = record.files.filter(f => f.uid !== uid); MessageDrafts.originals.delete(uid); MessageDrafts.changed(); }
    if (retry) {
      const uid = retry.dataset.retryUpload, slot = record.files.find(f => f.uid === uid), original = MessageDrafts.originals.get(uid);
      if (original) startUpload(slot, original); else { replacing = uid; input.click(); }
    }
  };
  const menu = $(".say-mentions", form);
  let mentionIdx = 0, mentionChoices = [];
  if (menu) {
    if (!menu.id) menu.id = `say-mentions-${MessageDrafts.uid()}`;
    box.setAttribute("role", "combobox");
    box.setAttribute("aria-autocomplete", "list");
    box.setAttribute("aria-controls", menu.id);
    box.setAttribute("aria-expanded", "false");
  }
  const closeMentions = () => {
    if (!menu) return;
    menu.hidden = true;
    mentionChoices = [];
    mentionIdx = 0;
    box.setAttribute("aria-expanded", "false");
    box.removeAttribute("aria-activedescendant");
  };
  const syncMentionActive = () => {
    if (!menu) return;
    const opts = [...menu.querySelectorAll("[data-mention]")];
    opts.forEach((el, i) => {
      const on = i === mentionIdx;
      el.classList.toggle("on", on);
      el.setAttribute("aria-selected", on ? "true" : "false");
    });
    const cur = opts[mentionIdx];
    if (cur) {
      box.setAttribute("aria-activedescendant", cur.id);
      cur.scrollIntoView?.({ block: "nearest" });
    } else {
      box.removeAttribute("aria-activedescendant");
    }
  };
  const choose = name => { const at = box.selectionStart, before = box.value.slice(0, at).replace(/@[\w.-]*$/, `@${name} `); box.value = before + box.value.slice(at); box.setSelectionRange(before.length, before.length); closeMentions(); read(); };
  const offer = () => {
    if (!menu || !conv.mentions) return;
    const partial = mentionAt(box.value, box.selectionStart), choices = partial === null ? [] : conv.mentions(partial);
    mentionChoices = choices;
    mentionIdx = 0;
    menu.hidden = !choices.length;
    box.setAttribute("aria-expanded", choices.length ? "true" : "false");
    menu.innerHTML = choices.map((p, i) => `<button type="button" role="option" id="${esc(menu.id)}-${i}" aria-selected="${i === 0}" class="${i === 0 ? "on" : ""}" data-mention="${esc(p.name)}">${typeof avatar === "function" ? avatar(p.name, p.agent) : ""}${esc(p.name)}${p.agent ? ` <span class="small faint">personal agent</span>` : ""}</button>`).join("");
    syncMentionActive();
  };
  if (menu) menu.onclick = e => { const b = e.target.closest("[data-mention]"); if (b) { choose(b.dataset.mention); box.focus(); } };
  box.oninput = () => { read(); fit(); syncSend(); offer(); conv.typed?.(); };
  box.onselect = selected;
  const wrapSelection = (before, after = before, placeholder = "") => {
    const start = box.selectionStart ?? 0, end = box.selectionEnd ?? start;
    const sel = box.value.slice(start, end) || placeholder;
    const next = `${before}${sel}${after}`;
    box.setRangeText(next, start, end, "select");
    box.setSelectionRange(start + before.length, start + before.length + sel.length);
    read(); fit(); syncSend();
  };
  box.onpaste = e => {
    const dt = e.clipboardData;
    if (!dt) return;
    const files = dt.files?.length ? [...dt.files] : [...(dt.items || [])].filter(i => i.kind === "file").map(i => i.getAsFile()).filter(Boolean);
    if (files.length) { e.preventDefault(); attach(files); return; }
    const html = dt.getData("text/html");
    if (html && typeof RichText !== "undefined") {
      const md = RichText.htmlToMarkdown(html);
      if (md) {
        e.preventDefault();
        const start = box.selectionStart ?? box.value.length, end = box.selectionEnd ?? start;
        box.setRangeText(md, start, end, "end");
        read(); fit(); syncSend();
      }
    }
  };
  model?.addEventListener("change", read);
  box.onkeydown = e => {
    if ((e.metaKey || e.ctrlKey) && !e.altKey && !e.shiftKey) {
      const k = e.key.toLowerCase();
      if (k === "b") { e.preventDefault(); e.stopPropagation(); wrapSelection("**"); return; }
      if (k === "i") { e.preventDefault(); e.stopPropagation(); wrapSelection("*"); return; }
      if (k === "e") { e.preventDefault(); e.stopPropagation(); wrapSelection("`"); return; }
      if (k === "k" && box.selectionStart !== box.selectionEnd) { e.preventDefault(); e.stopPropagation(); wrapSelection("[", "](https://)"); return; }
    }
    if (menu && !menu.hidden && mentionChoices.length) {
      if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        mentionIdx = (mentionIdx + (e.key === "ArrowDown" ? 1 : -1) + mentionChoices.length) % mentionChoices.length;
        syncMentionActive();
        return;
      }
      if (e.key === "Enter" || e.key === "Tab") {
        const picked = mentionChoices[mentionIdx] || mentionChoices[0];
        if (picked) { e.preventDefault(); choose(picked.name); return; }
      }
      if (e.key === "Escape") { e.preventDefault(); closeMentions(); return; }
    }
    if (e.key === "Escape" && stopMode && !box.value.trim()) { e.preventDefault(); triggerStop(); return; }
    if (e.key === "Enter" && !e.shiftKey && !e.isComposing && e.keyCode !== 229) { e.preventDefault(); form.requestSubmit(); }
  };
  // Files dropped on the conversation are attached to the next message.
  const zone = form.closest("section.conv") || form;
  let dragDepth = 0;
  zone.ondragenter = e => { if (e.dataTransfer?.types?.includes("Files")) { dragDepth++; zone.classList.add("dropping"); } };
  zone.ondragover = e => { if (e.dataTransfer?.types?.includes("Files")) { e.preventDefault(); zone.classList.add("dropping"); } };
  zone.ondragleave = () => { dragDepth = Math.max(0, dragDepth - 1); if (dragDepth === 0) zone.classList.remove("dropping"); };
  zone.ondrop = e => { dragDepth = 0; zone.classList.remove("dropping"); if (!e.dataTransfer?.files?.length) return; e.preventDefault(); attach(e.dataTransfer.files); };
  form.onsubmit = async e => {
    e.preventDefault(); read();
    const text = record.text.trim(); if ((!text && !record.files.length) || record.sending) return;
    if (record.files.some(f => f.state !== "ready")) { toast("Wait for uploads, or retry/remove unavailable files before sending."); return; }
    if (canSteer()) {
      record.sending = true; MessageDrafts.changed();
      try {
        await conv.steerRun([...record.runningIds], text);
        if (epoch !== MessageDrafts.epoch()) return;
        if (record.text.trim() === text) { record.text = ""; record.selection = [0, 0]; }
      } catch (err) {
        if (epoch !== MessageDrafts.epoch()) return;
        toast(err.message || "Could not steer active work.");
      } finally { record.sending = false; MessageDrafts.changed(); }
      return;
    }
    const files = [...record.files];
    record.sending = true; record.pendingRun = !!conv.stopRun; record.cancelRequested = false; MessageDrafts.changed();
    try {
      const then = await conv.send({ text, files, form, record });
      if (epoch !== MessageDrafts.epoch()) return;
      // Clear what was sent; anything typed or attached meanwhile stays.
      if (record.text.trim() === text) { record.text = ""; record.selection = [0, 0]; }
      record.files = record.files.filter(f => !files.includes(f));
      if (then) await then();
    } catch (err) {
      if (epoch !== MessageDrafts.epoch()) return;
      record.pendingRun = false;
      record.cancelRequested = false;
      // The message stays in the box. A lost response may still have been
      // accepted, so say so and never send it again by itself (UX-08).
      const rejected = err.status >= 400 && err.status < 500 && typeof err.body?.error === "string";
      toast(rejected ? err.message : "Send outcome not confirmed. Your message is still here; check the conversation before sending it again.");
      if (!rejected) conv.refresh().catch(() => {});
    } finally { record.sending = false; MessageDrafts.changed(); }
  };
  const stopVoice = wireVoice(form, box);
  onLeave(() => { stopVoice(); if (active) { read(false); active = false; unsubscribe(); } });
  // Only a missing/expired reference is marked unavailable. A connection
  // failure does not silently turn a valid attachment into a missing one.
  if (conv.verify) for (const f of record.files.filter(f => f.state === "ready")) conv.verify(f).catch(err => {
    if ([404, 410].includes(err.status) && epoch === MessageDrafts.epoch() && record.files.includes(f)) { f.state = "unavailable"; f.error = "Select the file again or remove it."; MessageDrafts.changed(); }
  });
  fit(); show();
  box.setSelectionRange(...(record.selection || [box.value.length, box.value.length]));
}

/** The longest a spoken message is recorded for the server to write down. */
const VOICE_MAX_MS = 120_000;

/**
 * The composer's mic. When the server has voice input installed, what is
 * said is recorded and the server writes it down (POST /v1/transcribe,
 * whisper.cpp), so it never leaves the server; this works in any web view.
 * Otherwise the browser's own speech recognition writes it as it is said,
 * where the browser has one. With neither, there is no mic. The person reads
 * the words before sending them.
 * @param {HTMLFormElement} form @param {HTMLTextAreaElement} box
 * @returns {() => void} stops listening
 */
function wireVoice(form, box) {
  const voiceBtn = /** @type {?HTMLButtonElement} */ ($("[data-voice-in]", form));
  // The server's, when it has voice input, so what is said stays on it.
  const recorder = !!(typeof App !== "undefined" && App.me?.voice) && !!navigator.mediaDevices?.getUserMedia && typeof MediaRecorder !== "undefined";
  const SR = recorder ? null : window.SpeechRecognition || window.webkitSpeechRecognition;
  if (!voiceBtn) return () => {};
  if (!SR && !recorder) { voiceBtn.hidden = true; return () => {}; }
  /** @type {any} */
  let recog = null;
  const add = (/** @type {string} */ said) => {
    if (!said.trim()) return;
    box.value = (box.value ? `${box.value.trimEnd()} ` : "") + said.trim();
    box.dispatchEvent(new Event("input"));
    box.focus();
  };
  const idle = () => {
    voiceBtn.classList.remove("recording");
    voiceBtn.setAttribute("aria-pressed", "false");
    voiceBtn.title = "Speak message";
  };
  const stopVoice = () => {
    if (recog) { try { recog.stop(); } catch { /* ignore */ } recog = null; }
    idle();
  };
  const listening = () => {
    voiceBtn.classList.add("recording");
    voiceBtn.setAttribute("aria-pressed", "true");
    voiceBtn.title = "Stop speaking";
  };
  if (SR) {
    voiceBtn.onclick = () => {
      if (recog) { stopVoice(); return; }
      recog = new SR();
      recog.continuous = true;
      recog.interimResults = false;
      recog.onresult = (/** @type {any} */ ev) => {
        let added = "";
        for (let k = ev.resultIndex; k < ev.results.length; k++) if (ev.results[k].isFinal) added += ev.results[k][0].transcript;
        add(added);
      };
      recog.onerror = () => stopVoice();
      recog.onend = () => stopVoice();
      listening();
      try { recog.start(); } catch { stopVoice(); }
    };
    return stopVoice;
  }
  // Recorded, and written down by the server.
  /** @type {?MediaRecorder} */
  let rec = null;
  let limit = 0;
  const stopRecording = () => { clearTimeout(limit); if (rec && rec.state !== "inactive") rec.stop(); };
  voiceBtn.onclick = async () => {
    if (rec) { stopRecording(); return; }
    /** @type {MediaStream} */
    let stream;
    try { stream = await navigator.mediaDevices.getUserMedia({ audio: true }); }
    catch { toast("Magnus cannot hear you: allow the microphone for this app, then try again."); return; }
    /** @type {Blob[]} */
    const parts = [];
    const r = new MediaRecorder(stream);
    rec = r;
    r.ondataavailable = e => { if (e.data.size) parts.push(e.data); };
    r.onstop = async () => {
      stream.getTracks().forEach(t => t.stop());
      rec = null;
      idle();
      const audio = new Blob(parts, { type: r.mimeType || "audio/webm" });
      if (!audio.size) return;
      voiceBtn.disabled = true;
      voiceBtn.title = "Writing down what you said…";
      try {
        const data = await new Promise((ok, bad) => { const f = new FileReader(); f.onload = () => ok(String(f.result).split(",")[1] || ""); f.onerror = bad; f.readAsDataURL(audio); });
        const said = await Server.transcribe(/** @type {string} */ (data));
        add(said?.text || "");
      } catch (err) { toast(`What you said was not written down: ${/** @type {Error} */ (err).message || err}`); }
      finally { voiceBtn.disabled = false; voiceBtn.title = "Speak message"; }
    };
    r.start();
    listening();
    limit = window.setTimeout(stopRecording, VOICE_MAX_MS);
  };
  return () => { stopRecording(); stopVoice(); };
}
