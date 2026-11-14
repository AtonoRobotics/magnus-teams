// One frontend, two hosts. In the desktop app the Rust side holds the
// session token, pins the appliance's certificate and makes every request;
// this page never sees the token. In a browser served by the appliance the
// session is an HttpOnly cookie, and changes carry a header another site's
// page cannot set.
"use strict";

class ApiError extends Error {
  constructor(status, message, body) { super(message); this.status = status; this.body = body; }
}

function errorOf(status, body) {
  const msg = (body && (body.error || body.message)) || (typeof body === "string" && body) || `request failed (${status})`;
  return new ApiError(status, msg, body);
}

const tauri = window.__TAURI__;

/** Base64 to bytes with a plain loop: Uint8Array.from with a mapping
 * function calls it once per byte, which a screen's chunks feel. */
function bytesOfB64(b64) {
  const s = atob(b64), out = new Uint8Array(s.length);
  for (let i = 0; i < s.length; i++) out[i] = s.charCodeAt(i);
  return out;
}

const appBridge = tauri && {
  kind: "app",
  // A shell command fails with a plain string; the page shows `e.message`.
  invoke: (cmd, args) => tauri.core.invoke(cmd, args || {}).catch(e => { throw e instanceof Error ? e : new Error(String(e)); }),
  async restore() { return this.invoke("restore"); },
  async connect(url) { return this.invoke("connect", { url }); },
  async trust(url, fingerprint) { return this.invoke("trust", { url, fingerprint }); },
  async signInMethod() { return this.invoke("sign_in_method"); },
  // Pair this computer for the public address. The key never comes back to the page.
  async pairDevice(publicUrl) { return this.invoke("pair_device", { publicUrl }); },
  // Resolves with who signed in, or { code_needed }, { browser } or
  // { machine_code_needed } (the owner's backup while the provider is down).
  async signIn(person, password, code, machineCode) { return this.invoke("sign_in", { person, password, code: code || null, machineCode: machineCode || null }); },
  // Sign in through the identity provider in the person's own browser,
  // which comes back to this computer; resolves with who signed in.
  async providerSignIn(loginHint) { return this.invoke("provider_sign_in", { loginHint: loginHint || null }); },
  async providerSignInCancel() { return this.invoke("provider_sign_in_cancel"); },
  async signOut() { return this.invoke("sign_out"); },
  // Ask for a recovery email; the answer is the same whoever holds the address.
  async forgotPassword(email) { return this.invoke("forgot_password", { email }); },
  async api(method, path, body) {
    const r = await this.invoke("api", { method, path, body: body === undefined ? null : body });
    if (r.status === 426) window.dispatchEvent(new CustomEvent("magnus-update-required", { detail: r.body }));
    if (r.status >= 400) throw errorOf(r.status, r.body);
    return r.body;
  },
  live(task, onText) {
    let stop = false, unlisten = null;
    tauri.event.listen("live", e => { if (!stop && e.payload.task === task) onText(e.payload.text); }).then(u => { unlisten = u; if (stop) u(); });
    this.invoke("live_start", { task }).catch(() => {});
    return () => { stop = true; if (unlisten) unlisten(); this.invoke("live_stop", { task }).catch(() => {}); };
  },
  // The system's own notifications. The desktop app may always show them;
  // a click on one is the system's to handle.
  async notify(title, body) { return this.invoke("notify", { title, body }).catch(() => {}); },
  notifyPermission() { return "granted"; },
  async askToNotify() { return "granted"; },
  async speak(text) { return this.invoke("speak", { text }); },
  async stopSpeaking() { return this.invoke("stop_speaking").catch(() => {}); },
  async download(path, name) { return this.invoke("download", { path, name }); },
  async openExternal(url) { return this.invoke("open_url", { url }); },
  // App sign-ins come back to a loopback address on this computer.
  async loopbackOpen(redirect) { return this.invoke("loopback_open", { redirect: redirect || null }); },
  async loopbackWait() { return this.invoke("loopback_wait"); },
  async sandboxUrl(html) { return this.invoke("sandbox_frame", { html, csp: UiSpec.catalog?.html_csp || null }); },
  async putBytes(path, bytes) { let s = ""; for (const b of bytes) s += String.fromCharCode(b); return this.invoke("put_bytes", { path, dataB64: btoa(s) }); },
  async localState() { return this.invoke("local_state"); },
  async localStart(how) { return this.invoke("local_start", how); },
  async localPause(paused) { return this.invoke("local_pause", { paused }); },
  async localStop() { return this.invoke("local_stop"); },
  // This computer as the bridge to its programs names it: { id, name }.
  async desktopDevice() { return this.invoke("desktop_device"); },
  // Report this computer's programs again now, after a consent.
  async desktopRefresh() { return this.invoke("desktop_refresh").catch(() => {}); },
  // A takeover's screen, carried by the app so its pinned certificate applies.
  displayOpen(computer, ticket, prefix, onBytes, onEnd) {
    const handle = crypto.randomUUID();
    const off = [];
    const end = reason => { off.splice(0).forEach(u => u()); onEnd(reason); };
    // The app says with each chunk whether the server chose the deflated screen.
    let push = null;
    const take = p => {
      const bytes = bytesOfB64(p.b64);
      if (!p.z) return onBytes(bytes);
      push ||= Rfb.inflater(onBytes, reason => { this.invoke("display_close", { handle }).catch(() => {}); end(reason); });
      push(bytes);
    };
    tauri.event.listen("display-bytes", e => { if (e.payload.handle === handle) take(e.payload); }).then(u => off.push(u));
    tauri.event.listen("display-end", e => { if (e.payload.handle === handle) end(e.payload.reason); }).then(u => off.push(u));
    this.invoke("display_open", { handle, computer, ticket, prefix, deflate: Rfb.canInflate() }).catch(e => end(String(e)));
    return {
      send: bytes => { let s = ""; for (const b of bytes) s += String.fromCharCode(b); this.invoke("display_send", { handle, b64: btoa(s) }).catch(() => {}); },
      close: () => { off.splice(0).forEach(u => u()); this.invoke("display_close", { handle }).catch(() => {}); },
    };
  },
  async dataUrl(path, mediaType) { const file = await this.invoke("fetch_bytes", { path }); return `data:${mediaType || file.media_type};base64,${file.data_b64}`; },
  // The app's own updates, served by the appliance and signed with the release key.
  async updateState() { return this.invoke("update_state"); },
  async updateLook() { return this.invoke("update_look"); },
  async updateInstall() { return this.invoke("update_install"); },
  // `again`: also after it was shown once (for Settings).
  async whatsNew(again = false) { return this.invoke("whats_new", { again }); },
  onUpdateReady(fn) { tauri.event.listen("update-ready", e => fn(e.payload)); },
  // Glass from the system's own window material where it has one.
  async setMaterial(glass, dark, tint) { return this.invoke("set_material", { glass, dark, tint }).catch(() => ({ native: false })); },
  async systemLook() { return this.invoke("system_look").catch(() => ({ reduce_transparency: false })); },
};

const webBridge = {
  kind: "web",
  async restore() {
    const r = await fetch("/v1/me", { credentials: "same-origin" });
    if (!r.ok) return null;
    return { person: await r.json(), appliance: location.origin };
  },
  async connect() { return { status: "trusted", url: location.origin }; },
  async trust() {},
  async signInMethod() { return this.api("GET", "/v1/sign-in"); },
  async signIn(person, password, code, machineCode) {
    try {
      return (await this.api("POST", "/v1/sign-in", { person, password, code: code || undefined, machine_code: machineCode || undefined })).person;
    } catch (e) {
      if (e.body?.code_needed || e.body?.browser || e.body?.machine_code_needed) return e.body;
      throw e;
    }
  },
  async deviceStart() { return this.api("POST", "/v1/sign-in/device", { client: "browser" }); },
  async devicePoll(flow) {
    const r = await fetch(`/v1/sign-in/device/${encodeURIComponent(flow)}`, { method: "POST", credentials: "same-origin", headers: { "x-magnus-client": "web" } });
    const body = await r.json().catch(() => null);
    if (r.status === 202) return { pending: true };
    if (!r.ok) throw errorOf(r.status, body);
    return { person: body.person };
  },
  async signOut() { await this.api("POST", "/v1/sign-out", {}).catch(() => {}); },
  async forgotPassword(email) { return this.api("POST", "/v1/password-reset/request", { email }); },
  async api(method, path, body) {
    const opts = { method, credentials: "same-origin", headers: { "x-magnus-client": "web" } };
    if (body !== undefined && body !== null) { opts.headers["content-type"] = "application/json"; opts.body = JSON.stringify(body); }
    const r = await fetch(path, opts);
    const text = await r.text();
    let parsed = text;
    try { parsed = text ? JSON.parse(text) : null; } catch { /* plain text */ }
    if (!r.ok) throw errorOf(r.status, parsed);
    return parsed;
  },
  live(task, onText) {
    const es = new EventSource(`/v1/tasks/${encodeURIComponent(task)}/live`);
    es.onmessage = e => { try { onText(JSON.parse(e.data).text); } catch { /* ignore */ } };
    return () => es.close();
  },
  // A browser shows the system's notifications only once the person has
  // allowed them, which it asks only from their own click (askToNotify).
  async notify(title, body, onClick) {
    if (this.notifyPermission() !== "granted") return;
    const n = new Notification(title, { body });
    n.onclick = () => { window.focus(); n.close(); onClick?.(); };
  },
  notifyPermission() { return "Notification" in window ? Notification.permission : "unsupported"; },
  async askToNotify() { return "Notification" in window ? Notification.requestPermission() : "unsupported"; },
  async speak(text) {
    if (!("speechSynthesis" in window)) throw new Error("This browser cannot read aloud.");
    await this.stopSpeaking();
    await new Promise((resolve, reject) => {
      this._speakingDone = resolve;
      const utterance = new SpeechSynthesisUtterance(text);
      utterance.onend = resolve;
      utterance.onerror = e => ["canceled", "interrupted"].includes(e.error) ? resolve() : reject(new Error(`Read aloud: ${e.error}`));
      speechSynthesis.speak(utterance);
    });
  },
  async stopSpeaking() { if ("speechSynthesis" in window) speechSynthesis.cancel(); this._speakingDone?.(); this._speakingDone = null; },
  async download(path, name) {
    const a = document.createElement("a");
    a.href = path; a.download = name; document.body.append(a); a.click(); a.remove();
    return name;
  },
  async openExternal(url) { window.open(url, "_blank", "noopener"); },
  // The appliance serves this page, so it is always the appliance's own version.
  async updateState() { return null; },
  async updateLook() { return null; },
  async whatsNew() { return null; },
  onUpdateReady() {},
  // A browser has no window material: the page draws its backdrop.
  async setMaterial() { return { native: false }; },
  async systemLook() { return { reduce_transparency: false }; },
  async sandboxUrl(html) { return (await this.api("POST", "/v1/ui/frames", { html })).url; },
  async putBytes(path, bytes) {
    const r = await fetch(path, { method: "PUT", credentials: "same-origin", body: bytes });
    if (!r.ok) throw errorOf(r.status, await r.text());
  },
  // Connecting this computer needs the desktop app.
  async localState() { return null; },
  // A program on this computer is used through the desktop app.
  async desktopDevice() { return null; },
  async desktopRefresh() {},
  displayOpen(computer, ticket, prefix, onBytes, onEnd) {
    const scheme = location.protocol === "https:" ? "wss:" : "ws:";
    // A server that knows the deflated screen chooses it; an older one sends it raw.
    const offered = [prefix + ticket, ...(Rfb.canInflate() ? [Rfb.DEFLATE] : []), "magnus.display"];
    const socket = new WebSocket(`${scheme}//${location.host}/v1/computers/${encodeURIComponent(computer)}/display`, offered);
    socket.binaryType = "arraybuffer";
    let push = null;
    socket.onopen = () => { if (socket.protocol === Rfb.DEFLATE) push = Rfb.inflater(onBytes, reason => { socket.onclose = null; socket.close(); onEnd(reason); }); };
    socket.onmessage = e => (push || onBytes)(new Uint8Array(e.data));
    socket.onclose = () => onEnd();
    return { send: bytes => { if (socket.readyState === WebSocket.OPEN) socket.send(bytes); }, close: () => { socket.onclose = null; socket.close(); } };
  },
  async dataUrl(path) {
    const r = await fetch(path, { credentials: "same-origin" });
    if (!r.ok) throw errorOf(r.status, await r.text());
    const blob = await r.blob();
    return new Promise((ok, no) => { const f = new FileReader(); f.onload = () => ok(f.result); f.onerror = no; f.readAsDataURL(blob); });
  },
};

window.Magnus = appBridge || webBridge;
window.ApiError = ApiError;
