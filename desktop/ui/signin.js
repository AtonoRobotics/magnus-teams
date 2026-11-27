// Server connection, certificate pinning, device pairing, and sign-in flow.

// ---------- sign-in ----------

async function signInScreen(note) {
  document.body.innerHTML = `<div class="signin"><h1>Magnus</h1><p class="muted">Sign in to your server.</p><div class="card" id="si"></div></div>`;
  Magnus.pageWorks();
  const box = $("#si");
  // Signed in. Pair this computer for the public name once, on the private
  // address; a failure is said and sign-in still stands.
  async function enter(person, data) {
    if (data.person) localStorage.setItem("magnus.person", data.person);
    if (Magnus.pairDevice && data.public_url) {
      try { await Magnus.pairDevice(data.public_url); }
      catch (err) { return ask("paired", { ...data, error: err.message || String(err), who: person }); }
    }
    App.me = person;
    App.appliance = data.url || location.origin;
    boot();
  }
  const ask = async (stage, data = {}) => {
    if (stage === "address") {
      paint(box, `${note ? `<p class="small" style="color:var(--bad)">${esc(note)}</p>` : ""}
        <form id="f"><label for="addr">Server address</label>
        <input id="addr" style="width:100%" placeholder="https://magnus.local:7443" value="${esc(data.url || localStorage.getItem("magnus.address") || "")}" required>
        <p class="small muted">The owner gives you this address.</p>
        <div class="row" style="margin-top:14px"><button class="primary">Continue</button></div></form>`);
      $("#f").onsubmit = async e => {
        e.preventDefault();
        const url = $("#addr").value.trim().replace(/\/+$/, "");
        try {
          const c = await Magnus.connect(url);
          localStorage.setItem("magnus.address", url);
          if (c.status === "trusted") ask("method", { url });
          else if (c.status === "needs_device") ask("needs_device", { url });
          else ask("pin", { url, ...c });
        } catch (err) { note = err.message || String(err); ask("address", { url }); }
      };
    } else if (stage === "pin") {
      const changed = data.status === "changed";
      paint(box, `<h3>${changed ? "The server's certificate changed" : "Check the server"}</h3>
        <p class="small">${changed ? "This is not the certificate you trusted before. Continue only if the owner told you it was replaced." : "This is the first connection to this server. Compare this fingerprint with the one the owner sees on the server."}</p>
        <div class="fp" id="fp">${esc(data.fingerprint)}</div>
        <div class="row" style="margin-top:14px"><button class="${changed ? "danger" : "primary"}" id="ok">They match</button><button id="no">Cancel</button></div>`);
      $("#ok").onclick = async () => { await Magnus.trust(data.url, data.fingerprint); ask("method", data); };
      $("#no").onclick = () => ask("address", data);
    } else if (stage === "needs_device") {
      paint(box, `<h3>This computer is not paired</h3>
        <p class="small">magnus.pyrallon.ai accepts only a computer that was paired with the server. Sign in once on the server's private address, the one on your network or Tailscale. This computer is paired then, and this address works after that, from anywhere.</p>
        <div class="row" style="margin-top:14px"><button id="back">Use a private address</button></div>`);
      $("#back").onclick = () => ask("address", {});
    } else if (stage === "paired") {
      paint(box, `<h3>This computer was not paired</h3>
        <p class="small" style="color:var(--bad)" role="alert">${esc(data.error)}</p>
        <p class="small muted">You are signed in on this network. ${esc(data.public_url || "")} will not accept this computer until pairing works.</p>
        <div class="row" style="margin-top:14px"><button class="primary" id="go">Continue</button></div>`);
      $("#go").onclick = () => { App.me = data.who; App.appliance = data.url || location.origin; boot(); };
    } else if (stage === "method") {
      paint(box, `<p class="small muted">${esc(data.url || location.origin)}</p><p class="small muted">Connecting…</p>`);
      try {
        const m = await Magnus.signInMethod();
        ask("password", { ...data, public_url: m.public_url, label: m.method === "oidc" ? m.label : undefined });
      } catch (err) { note = err.message || String(err); ask(Magnus.kind === "app" ? "address" : "password", data); }
    } else if (stage === "forgot") {
      forgotPasswordStep(box, data, () => ask("password", { ...data, error: undefined }));
    } else if (stage === "browser") {
      // The provider's own page in the person's browser: for a passkey, the
      // first time, or when their account needs finishing there.
      const back = () => ask("password", { ...data, message: undefined });
      if (Magnus.providerSignIn) {
        paint(box, `<p class="small muted">${esc(data.url || location.origin)}</p>
          <p>${esc(data.message || `Sign in on the ${data.label} page that just opened in your browser, then come back here.`)}</p>
          <p class="small muted" id="wait" role="status">Waiting for you to sign in…</p>
          <div class="row" style="margin-top:14px"><button class="link" id="cancel">Cancel</button></div>`);
        // A cancelled sign-in stops waiting; one the browser still finishes is not kept.
        let cancelled = false;
        $("#cancel").onclick = () => { cancelled = true; Magnus.providerSignInCancel?.().catch(() => {}); back(); };
        try {
          const r = await Magnus.providerSignIn(data.person);
          if (cancelled) return;
          return enter(r.person, data);
        } catch (err) { if (!cancelled) ask("password", { ...data, message: undefined, error: err.message || String(err) }); return; }
      }
      let s;
      try { s = await Magnus.deviceStart(); } catch (err) { return ask("password", { ...data, error: err.message || String(err) }); }
      const link = s.verification_uri_complete || s.verification_uri;
      let stop = false;
      paint(box, `<p class="small muted">${esc(data.url || location.origin)}</p>
        <p>${esc(data.message || `Sign in on the ${s.label} page that just opened.`)}</p>
        <p class="small">Check that it shows this code:</p><div class="fp" id="code">${esc(s.user_code)}</div>
        <div class="row" style="margin-top:14px"><button data-url="${esc(link)}" id="open">Open the sign-in page again</button><button class="link" id="cancel">Cancel</button></div>
        <p class="small muted" id="wait" role="status">Waiting for you to approve…</p>`);
      $("#open").onclick = () => Magnus.openExternal(link).catch(() => {});
      $("#cancel").onclick = () => { stop = true; back(); };
      Magnus.openExternal(link).catch(() => {});
      const until = Date.now() + s.expires_in * 1000;
      const every = Math.max(1, s.interval) * 1000;
      while (!stop && Date.now() < until) {
        await new Promise(r => setTimeout(r, every));
        if (stop) return;
        try {
          const r = await Magnus.devicePoll(s.flow);
          if (r.pending) continue;
          return enter(r.person, data);
        } catch (err) { return ask("password", { ...data, message: undefined, error: err.message || String(err) }); }
      }
      if (!stop) ask("password", { ...data, message: undefined, error: "The sign-in code expired. Start again." });
    } else {
      // Username and password, then the code from an authenticator app if
      // the person uses one. The password stays in this closure only.
      const coding = stage === "code" || stage === "machine";
      const machine = stage === "machine";
      paint(box, `<form id="f"><p class="small muted">${esc(data.url || location.origin)}</p>
        ${data.error ? `<p class="small" style="color:var(--bad)" role="alert">${esc(data.error)}</p>` : ""}
        ${machine
          ? `<label for="otp">Enter the code \`magnus sign-in-code\` printed on the server</label><input id="otp" style="width:100%" autocomplete="one-time-code" maxlength="16" required autofocus>`
          : coding
          ? `<label for="otp">Enter the 6-digit code from your authenticator app</label><input id="otp" style="width:100%" inputmode="numeric" autocomplete="one-time-code" pattern="[0-9]*" maxlength="6" required autofocus>`
          : `<label for="who">Username or email</label><input id="who" style="width:100%" autocomplete="username" value="${esc(data.person || localStorage.getItem("magnus.person") || "")}" required>
        <label for="pw">Password</label><input id="pw" type="password" style="width:100%" autocomplete="current-password" required>`}
        <div class="row" style="margin-top:16px"><button class="primary" id="go">${coding ? "Continue" : "Sign in"}</button>${coding ? `<button type="button" class="link" id="again">Back</button>` : data.label ? `<button type="button" class="link" id="passkey">Use a passkey instead</button>` : `<button type="button" class="link" id="forgot">Forgot password?</button>`}${Magnus.kind === "app" && !coding ? `<button type="button" class="link" id="back">Other server</button>` : ""}</div></form>`);
      $("#back")?.addEventListener("click", () => ask("address", data));
      $("#again")?.addEventListener("click", () => ask("password", { ...data, error: undefined, password: undefined }));
      $("#passkey")?.addEventListener("click", () => ask("browser", { ...data, person: $("#who").value.trim() || undefined, error: undefined }));
      $("#forgot")?.addEventListener("click", () => ask("forgot", { ...data, person: $("#who").value.trim() || undefined, error: undefined }));
      $("#f").onsubmit = async e => {
        e.preventDefault();
        const person = coding ? data.person : $("#who").value.trim();
        const password = coding ? data.password : $("#pw").value;
        $("#go").disabled = true;
        try {
          const typed = coding ? $("#otp").value.trim() : undefined;
          const r = await Magnus.signIn(person, password, machine ? undefined : typed, machine ? typed : undefined);
          if (r?.machine_code_needed) return ask("machine", { ...data, person, password, error: r.error });
          if (r?.code_needed) return ask("code", { ...data, person, password, error: coding ? r.error : undefined });
          if (r?.browser) return ask("browser", { ...data, person, password: undefined, message: r.error });
          return enter(r, { ...data, person });
        } catch (err) { ask("password", { ...data, person, password: undefined, error: err.message || String(err) }); }
      };
    }
  };
  if (Magnus.kind === "web") ask("method", { url: location.origin });
  else ask("address");
}

