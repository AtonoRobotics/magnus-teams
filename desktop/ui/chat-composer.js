// The one composer, for every conversation the chat view shows (chat-view.js):
// the personal chat, a team channel and a team thread. Files added to the
// next message upload at once and are sent with it. What is typed and
// attached stays with its conversation while the person moves around the app
// (#544). Enter sends; Shift+Enter and an input method's Enter do not. In a
// conversation of several people, typing @ offers who to mention.

/**
 * What the composer needs from its conversation.
 * @typedef {Object} ComposerConversation
 * @property {(file: File, record: any) => Promise<{ id: string, name: string, media_type?: string, size?: number }>} upload
 * @property {(o: { text: string, files: any[], form: HTMLFormElement, record: any }) => Promise<?(() => Promise<any>)>} send what comes back runs once the box is cleared
 * @property {() => Promise<any>} refresh after a send whose outcome is unknown
 * @property {(partial: string) => Array<{ name: string, agent?: boolean }>} [mentions]
 * @property {() => void} [typed] someone is typing here
 * @property {(text: string) => void} [keep] what is typed now, to keep beyond this session
 * @property {(f: any) => Promise<any>} [verify] whether an attached file is still there
 */

/**
 * @param {string} placeholder @param {string} bar what sits at the start of the bar (the model picker, or "Only my Magnus sees this")
 * @param {{ side?: boolean, label?: string, mentions?: boolean }} [o]
 */
function composer(placeholder, bar, o = {}) {
  const id = n => o.side ? "" : ` id="${n}"`;
  const label = o.side ? `<textarea rows="1" placeholder="${esc(placeholder)}" aria-label="${esc(o.label || placeholder)}" required></textarea>`
    : `<label for="msg" class="sr">Message</label><textarea id="msg" rows="1" placeholder="${esc(placeholder)}" required></textarea>`;
  return `<form${id("say")} class="say">${label}${o.mentions ? `<div class="say-mentions" role="listbox" aria-label="Mention" hidden></div>` : ""}
    <div class="row small pending"${id("pending")}></div>
    <div class="say-bar">${bar}<span class="grow"></span>${attachButton("Attach files", true)}<button class="send"${id("send")} aria-label="Send message" title="Send message">${icon("send")}<span class="sr">Send message</span></button></div></form>`;
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
  const box = $("textarea", form), pending = $(".pending", form), input = $("[data-attach]", form), send = $(".send", form), epoch = MessageDrafts.epoch();
  let replacing = null, active = true;
  box.value = record.text;
  const model = $("[data-model]", form);
  if (model && record.model && record.modelUnder === model.dataset.default && [...model.options].some(o => o.value === record.model)) model.value = record.model;
  const fit = () => { box.style.height = "auto"; box.style.height = `${Math.min(box.scrollHeight, innerHeight / 3)}px`; };
  const selected = () => { record.selection = [box.selectionStart, box.selectionEnd]; };
  const read = (selection = true) => { record.text = box.value; record.model = model?.value || ""; record.modelUnder = model?.dataset.default || ""; if (selection) selected(); };
  const chips = () => record.files.map(f => `<span class="attachment-chip" data-upload="${esc(f.uid)}"><span>${esc(f.name)}</span><span class="small muted">${esc(f.state === "ready" ? "Ready" : f.state === "uploading" ? "Uploading…" : f.state === "unavailable" ? "Unavailable" : "Failed")}</span>${f.error ? `<span class="small">${esc(f.error)}</span>` : ""}${["failed", "unavailable"].includes(f.state) ? `<button type="button" data-retry-upload="${esc(f.uid)}">Retry</button>` : ""}<button type="button" class="icon-btn" data-remove-upload="${esc(f.uid)}" aria-label="Remove ${esc(f.name)} from message" title="Remove ${esc(f.name)} from message">${icon("close")}</button></span>`).join("");
  // The record is shared by every composer mounted on this conversation:
  // a send that finishes after the person left and came back updates the
  // one now showing, and a send in flight stays in flight there too.
  const show = () => {
    if (!active || !form.isConnected) return;
    if (box.value !== record.text) { box.value = record.text; fit(); }
    paint(pending, chips());
    send.disabled = !!record.sending || record.files.some(f => f.state !== "ready");
  };
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
  const choose = name => { const at = box.selectionStart, before = box.value.slice(0, at).replace(/@[\w.-]*$/, `@${name} `); box.value = before + box.value.slice(at); box.setSelectionRange(before.length, before.length); menu.hidden = true; read(); conv.keep?.(box.value); };
  const offer = () => {
    if (!menu || !conv.mentions) return;
    const partial = mentionAt(box.value, box.selectionStart), choices = partial === null ? [] : conv.mentions(partial);
    menu.hidden = !choices.length;
    menu.innerHTML = choices.map(p => `<button type="button" role="option" data-mention="${esc(p.name)}">${typeof avatar === "function" ? avatar(p.name, p.agent) : ""}${esc(p.name)}${p.agent ? ` <span class="small faint">team agent</span>` : ""}</button>`).join("");
  };
  if (menu) menu.onclick = e => { const b = e.target.closest("[data-mention]"); if (b) { choose(b.dataset.mention); box.focus(); } };
  box.oninput = () => { read(); fit(); conv.keep?.(box.value); offer(); conv.typed?.(); };
  box.onselect = selected;
  model?.addEventListener("change", read);
  box.onkeydown = e => {
    if (menu && !menu.hidden && (e.key === "Enter" || e.key === "Tab")) { const first = $("[data-mention]", menu); if (first) { e.preventDefault(); choose(first.dataset.mention); return; } }
    if (menu && e.key === "Escape") menu.hidden = true;
    if (e.key === "Enter" && !e.shiftKey && !e.isComposing && e.keyCode !== 229) { e.preventDefault(); form.requestSubmit(); }
  };
  // Files dropped on the conversation are attached to the next message.
  const zone = form.closest("section.conv") || form;
  zone.ondragover = e => { if (e.dataTransfer?.types?.includes("Files")) { e.preventDefault(); zone.classList.add("dropping"); } };
  zone.ondragleave = () => zone.classList.remove("dropping");
  zone.ondrop = e => { if (!e.dataTransfer?.files?.length) return; e.preventDefault(); zone.classList.remove("dropping"); attach(e.dataTransfer.files); };
  form.onsubmit = async e => {
    e.preventDefault(); read();
    const text = record.text.trim(); if (!text || record.sending) return;
    if (record.files.some(f => f.state !== "ready")) { toast("Wait for uploads, or retry/remove unavailable files before sending."); return; }
    const files = [...record.files];
    record.sending = true; MessageDrafts.changed();
    try {
      const then = await conv.send({ text, files, form, record });
      if (epoch !== MessageDrafts.epoch()) return;
      // Clear what was sent; anything typed or attached meanwhile stays.
      if (record.text.trim() === text) { record.text = ""; record.selection = [0, 0]; }
      record.files = record.files.filter(f => !files.includes(f));
      conv.keep?.(record.text);
      if (then) await then();
    } catch (err) {
      if (epoch !== MessageDrafts.epoch()) return;
      // The message stays in the box. A lost response may still have been
      // accepted, so say so and never send it again by itself (UX-08).
      const rejected = err.status >= 400 && err.status < 500 && typeof err.body?.error === "string";
      toast(rejected ? err.message : "Send outcome not confirmed. Your message is still here; check the conversation before sending it again.");
      if (!rejected) conv.refresh().catch(() => {});
    } finally { record.sending = false; MessageDrafts.changed(); }
  };
  onLeave(() => { if (active) { read(false); active = false; unsubscribe(); } });
  // Only a missing/expired reference is marked unavailable. A connection
  // failure does not silently turn a valid attachment into a missing one.
  if (conv.verify) for (const f of record.files.filter(f => f.state === "ready")) conv.verify(f).catch(err => {
    if ([404, 410].includes(err.status) && epoch === MessageDrafts.epoch() && record.files.includes(f)) { f.state = "unavailable"; f.error = "Select the file again or remove it."; MessageDrafts.changed(); }
  });
  fit(); show();
  box.setSelectionRange(...(record.selection || [box.value.length, box.value.length]));
}
