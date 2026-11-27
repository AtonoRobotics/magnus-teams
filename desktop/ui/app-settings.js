// Trash & Settings views (extracted from app.js in Tier 5 Phase 4).
"use strict";

// ---------- Trash ----------

const TRASHED = { task: "Task", conversation: "Conversation", message: "Chat message", file: "File", project: "Project", memory: "Memory", knowledge: "Knowledge entry" };
route(/^#\/trash$/, async main => {
  const draw = async () => {
    const { items, keep_days } = await Server.trash.list();
    paint(main, `<h1>Trash</h1><p class="muted">What you delete waits here for ${esc(keep_days)} days, then it is gone for good. Restore puts it back where it was.</p>
      ${items.length ? `<div class="row"><button data-act="empty">Empty trash now</button></div>
      <table class="t trash"><thead><tr><th>Deleted</th><th>When</th><th>Gone for good</th><th></th></tr></thead><tbody>${items.map(i => `<tr data-trashed="${esc(i.item)}">
        <td><div class="small faint">${esc(TRASHED[i.kind] || i.kind)}</div><div data-title>${esc(i.title) || `<span class="faint">Untitled</span>`}</div></td>
        <td class="small faint n">${esc(when(i.trashed_at))}</td><td class="small faint n">${esc(new Date(i.removed_at).toLocaleDateString())}</td>
        <td><div class="row"><button data-act="restore">Restore</button><button class="link" data-act="remove">Delete now</button></div></td></tr>`).join("")}</tbody></table>`
      : `<p class="empty">The trash is empty.</p>`}`);
  };
  main.onclick = async e => {
    const b = e.target.closest("button[data-act]"); if (!b) return;
    const item = b.closest("[data-trashed]")?.dataset.trashed;
    try {
      if (b.dataset.act === "restore") { b.disabled = true; await Server.trash.restore(item); toast("Restored."); }
      if (b.dataset.act === "remove") {
        if (!await ask("Delete this for good? It cannot be restored.", "Delete for good")) return;
        b.disabled = true; await Server.trash.remove(item); toast("Deleted for good.");
      }
      if (b.dataset.act === "empty") {
        if (!await ask("Delete everything in the trash for good? None of it can be restored.", "Delete everything")) return;
        b.disabled = true; const r = await Server.trash.empty(); toast(`${r.removed} deleted for good.`);
      }
      await draw();
    } catch (err) { toast(err.message); b.disabled = false; }
  };
  App.redraw = draw;
  onLeave(() => { App.redraw = null; });
  await draw();
});

// The organisation's apps are Admin › Connectors' (admin-connectors.js); an
// added server's card for new tools that change or send is in the inbox.

// ---------- Settings ----------

// Everything a person sets for themselves, in one place; developer views
// stay in their own group. Administration lives in Admin, not here (spec
// rev 141): the server's models, the organisation's connectors, people's
// roles, the server's updates and the kill switch moved there.
const SETTINGS = [
  ["account", "Account", accountSection], ["rules", "Rules", (el) => rulesSection(el)], ["payments", "Payments", (el) => paymentsSection(el)],
  ["appearance", "Appearance", appearanceSection], ["notifications", "Notifications", (el) => notificationsSection(el)], ["updates", "Updates", (el) => updatesSection(el)], ["data", "Data and privacy", (el) => dataSection(el)],
  ["agreements", "Agreements", (el) => agreementsSection(el)],
];
/** Where Settings' administration went: its old addresses open Admin. */
const MOVED_TO_ADMIN = { models: "#/admin/models", connectors: "#/admin/connectors", safety: "#/admin/overview" };
/** Whether the signed-in person administers here (admin.js knows). */
const administers = () => typeof AdminState !== "undefined" && !!AdminState.standing?.admin;
async function openSettingsModal(which) {
  if (which === "rules") { openPanel("rules", { history: true, focus: true }); return; }
  if (typeof AdminState !== "undefined" && !AdminState.standing && typeof Server !== "undefined" && Server.admin?.standing) {
    AdminState.standing = await Server.admin.standing({ background: true }).catch(() => null);
  }
  const PERSONAL_TABS = [
    ["account", "Account & Privacy", async el => {
      el.innerHTML = `<div class="settings-stack"><section data-s="account"></section><section data-s="payments"></section><section data-s="data"></section><section data-s="agreements"></section></div>`;
      await accountSection($('[data-s="account"]', el));
      await paymentsSection($('[data-s="payments"]', el));
      await dataSection($('[data-s="data"]', el));
      await agreementsSection($('[data-s="agreements"]', el));
    }, "account privacy password sign out payments billing spend data export delete agreements terms"],
    ["memory", "Memory & Promises", memorySection, "memory facts forget playbook lessons promises commitments relationship notes"],
    ["appearance", "Appearance", appearanceSection, "appearance theme style dark light accent color motion density font sidebar"],
    ["notifications", "Notifications", notificationsSection, "notifications alerts sounds desktop badges"],
    ["updates", "Updates", updatesSection, "updates version check restart release"],
  ];
  const ADMIN_LABELS = { models: "Models & Providers" };
  const ADMIN_KEYWORDS = {
    overview: "overview kill switch safety status health",
    models: "models providers api keys llm adapters",
    connectors: "connectors apps integrations oauth",
    people: "people users roles invites members",
    agents: "agents workers computers",
    spend: "spend budget usage cost billing",
    security: "security audit backups passwords",
    operations: "operations server updates rollback logs",
  };
  const adminTabs = () => {
    if (!administers() || typeof ADMIN_SECTIONS === "undefined") return [];
    return ADMIN_SECTIONS.map(([k, label]) => [
      `admin:${k}`,
      ADMIN_LABELS[k] || label,
      async el => {
        const render = AdminState.sections.get(k);
        if (render) await render(el);
        else paint(el, `<h2>${esc(label)}</h2><p class="empty">${esc(label)} is not in the app yet.</p>`);
      },
      ADMIN_KEYWORDS[k] || label.toLowerCase(),
    ]);
  };
  const mapKey = k => {
    if (!k) return "account";
    if (k.startsWith("admin:")) return k;
    if (["models", "connectors", "people", "agents", "spend", "security", "operations", "overview"].includes(k)) {
      return administers() ? `admin:${k}` : "account";
    }
    if (k === "safety") return administers() ? "admin:overview" : "account";
    return ["payments", "data", "agreements"].includes(k) ? "account" : k;
  };
  let d = $("#settings-modal");
  let navigatingAway = false;
  if (!d) {
    d = document.createElement("dialog");
    d.id = "settings-modal";
    d.className = "settings-modal";
    d.setAttribute("aria-label", "Settings");
    document.body.append(d);
    const onHash = () => {
      if (!location.hash.startsWith("#/settings") && !location.hash.startsWith("#/admin") && location.hash !== "#/models") {
        navigatingAway = true;
        if (d.open) d.close();
      }
    };
    window.addEventListener("hashchange", onHash);
    d.addEventListener("click", e => {
      if (e.target === d) { d.close(); return; }
      if (/** @type {Element} */ (e.target).closest?.('a[href^="#/"]')) { navigatingAway = true; d.close(); }
    });
    d.addEventListener("close", () => {
      window.removeEventListener("hashchange", onHash);
      d.remove();
      if (!navigatingAway && (location.hash.startsWith("#/settings") || location.hash.startsWith("#/admin") || location.hash === "#/models")) {
        location.hash = App.lastNonSettingsHash || "#/home";
      }
    });
  }
  const aTabs = adminTabs();
  const allTabs = [...PERSONAL_TABS, ...aTabs];
  const renderTab = async key => {
    const target = mapKey(key);
    const sec = allTabs.find(x => x[0] === target) || PERSONAL_TABS[0];
    d.querySelectorAll("[data-settings-tab]").forEach(el => {
      const b = /** @type {HTMLElement} */ (el);
      const on = b.dataset.settingsTab === sec[0];
      b.classList.toggle("on", on);
      if (on) b.setAttribute("aria-current", "page");
      else b.removeAttribute("aria-current");
    });
    const host = $("#setting", d);
    if (host) {
      host.dataset.section = sec[0];
      host.innerHTML = "";
    }
    if (!d.open) d.showModal();
    if (host) await sec[2](host);
  };
  const adminKey = aTabs.map(x => x[0]).join(",");
  if (!$(".settings-shell", d) || d.dataset.adminTabs !== adminKey) {
    d.dataset.adminTabs = adminKey;
    const btn = ([k, label, , kw]) => `<button type="button" data-settings-tab="${esc(k)}" data-keywords="${esc(kw || "")}">${esc(label)}</button>`;
    d.innerHTML = `<div class="settings-head"><h2>Settings</h2><button type="button" class="icon-btn settings-close" data-settings-done aria-label="Close Settings" title="Close Settings">${icon("close")}</button></div>
      <div class="settings-shell">
        <nav class="settings-nav" aria-label="Settings">
          <input type="search" class="settings-filter" data-settings-filter placeholder="Filter settings…" aria-label="Filter settings">
          <div class="settings-nav-group" data-settings-group="personal">Personal</div>
          ${PERSONAL_TABS.map(btn).join("")}
          ${aTabs.length ? `<div class="settings-nav-group" data-settings-group="admin">Server &amp; Admin</div>${aTabs.map(btn).join("")}` : ""}
          <p class="small faint settings-filter-empty" data-settings-empty hidden>No matching settings.</p>
        </nav>
        <div class="settings-body"><section id="setting"></section></div>
      </div>`;
    $("[data-settings-done]", d).onclick = () => d.close();
    const filterInput = /** @type {?HTMLInputElement} */ ($("[data-settings-filter]", d));
    if (filterInput) {
      filterInput.oninput = () => {
        const q = filterInput.value.trim().toLowerCase();
        d.querySelectorAll("[data-settings-tab]").forEach(el => {
          const b = /** @type {HTMLElement} */ (el);
          const hay = `${b.textContent || ""} ${b.dataset.keywords || ""}`.toLowerCase();
          b.hidden = !!q && !hay.includes(q);
        });
        const tabs = [...d.querySelectorAll("[data-settings-tab]")];
        const personalVis = tabs.some(el => !/** @type {HTMLElement} */ (el).hidden && !/** @type {HTMLElement} */ (el).dataset.settingsTab?.startsWith("admin:"));
        const adminVis = tabs.some(el => !/** @type {HTMLElement} */ (el).hidden && /** @type {HTMLElement} */ (el).dataset.settingsTab?.startsWith("admin:"));
        const pg = /** @type {?HTMLElement} */ ($('[data-settings-group="personal"]', d));
        const ag = /** @type {?HTMLElement} */ ($('[data-settings-group="admin"]', d));
        const empty = /** @type {?HTMLElement} */ ($("[data-settings-empty]", d));
        if (pg) pg.hidden = !personalVis;
        if (ag) ag.hidden = !adminVis;
        if (empty) empty.hidden = personalVis || adminVis;
      };
    }
    d.querySelectorAll("[data-settings-tab]").forEach(b => {
      b.onclick = () => renderTab(b.dataset.settingsTab);
    });
  }
  await renderTab(which);
}

route(/^#\/settings(?:\/([a-z:]+))?$/, async (main, which) => {
  await openSettingsModal(which);
});
// Where these lived before Settings.
route(/^#\/models$/, () => openSettingsModal(administers() ? "admin:models" : "account"));
route(/^#\/apps$/, () => { location.hash = administers() ? "#/admin/connectors" : "#/library"; });

// How Magnus looks on this device: light, dark or the computer's; an accent;
// and how much it moves. The choice takes effect at once; the computer's own
// setting for less motion still applies on top.
function appearanceSection(el) {
  const radio = (look, value, label, inner = label, cls = "", style = "") => {
    const on = Theme.prefs()[look] === value;
    return `<button class="${cls}${on ? " on" : ""}" role="radio" aria-checked="${on}" data-look="${look}" data-value="${value}" title="${esc(label)}"${style ? ` style="${style}"` : ""}>${inner}</button>`;
  };
  const draw = () => {
    const p = Theme.prefs(), look = Theme.look;
    const isStage = Theme.stage(p.style);
    const defAccent = Theme.get?.(p.style)?.defaultAccent || "blue";
    const isBlueDefault = defAccent === "blue";
    const defHex = isBlueDefault ? "#7C8CFF" : (look.mode === "light" ? "#111111" : "#F5F5F5");
    const swatchColor = a => a.id === "graphite" && look.mode === "light" ? "#111111" : a.swatch;
    const currentHex = /^#[0-9a-fA-F]{6}$/.test(p.accent) ? p.accent.toUpperCase() : (Theme.ACCENTS.find(a => a.id === p.accent)?.[look.mode]?.accent || defHex);
    const styleDesc = {
      nova: "Nova pairs a pitch-carbon canvas and dissolving midnight halo with subtle floating orbs, an expressive companion avatar, borderless matte cards, and the self-composing autonomous stage.",
      obsidian: "Obsidian uses tight architectural corners, flush borders, and a pure black monochrome baseline.",
      "obsidian-2": "Obsidian 2 frames the workspace in a pure black card with a matte sidebar, inline file badges, and a circular send button.",
      "obsidian-aurora": "Obsidian 2 Aurora pairs Obsidian 2's monochrome precision and file badges with floating islands, living presence orbs, and dynamic execution beams.",
      prism: "Prism uses soft pill surfaces, rounded cards, and circular avatars.",
      studio: "Studio uses a two-column editorial hero, bento cards, uppercase nav groups, and an elevated pill composer.",
      aurora: "Aurora brings living ambient light shaders, liquid-glass presence orbs, floating workspace islands, and dynamic execution beams across desktop and mobile.",
    }[p.style] || "";
    const companionPicker = isStage && Theme.AVATAR_CHOICES
      ? `<h3 id="avatar-h">Companion avatar</h3><div class="seg" role="radiogroup" aria-labelledby="avatar-h">${Theme.AVATAR_CHOICES.map(([k, l]) => radio("avatar", k, l)).join("")}</div>
      <p class="small faint">Reacts live to every state (ready, listening, thinking, working, waiting, done) and follows your focus.</p>`
      : "";
    const bgPicker = isStage
      ? `<h3 id="bg-h">Motion background</h3><div class="seg" role="radiogroup" aria-labelledby="bg-h">${Theme.BACKGROUNDS.map(([k, l]) => radio("background", k, l)).join("")}</div>
      <p class="small faint">${esc(Theme.BACKGROUND_HINTS[p.background] || "")}</p>`
      : "";
    paint(el, `<h2>Appearance</h2><div class="appearance">
      ${Theme.saved === false ? `<p class="small" role="status">Appearance changed for this session but was not saved on this device.</p>` : ""}
      <h3 id="style-h">Theme</h3><div class="seg" role="radiogroup" aria-labelledby="style-h">${Theme.STYLES.map(([k, l]) => radio("style", k, l)).join("")}</div>
      <p class="small faint">${esc(styleDesc)}</p>
      ${companionPicker}
      <h3 id="mode-h">Mode</h3><div class="seg" role="radiogroup" aria-labelledby="mode-h">${Theme.MODES.map(([k, l]) => radio("mode", k, l)).join("")}</div>
      <h3 id="acc-h">Accent color</h3><div class="swatches ag-swatches" role="radiogroup" aria-labelledby="acc-h">${Theme.ACCENTS.map(a => { const sc = swatchColor(a); return radio("accent", a.id, a.label, `<span class="sr">${esc(a.label)}</span>`, "swatch", `--swatch:${sc};background:${sc}`); }).join("")}<label class="swatch-custom" title="Pick any custom hex color"><span class="swatch-custom-inner" aria-hidden="true">+</span><input type="color" data-custom-accent value="${esc(currentHex)}" aria-label="Custom accent color"></label><div class="hex-box"><span class="hex-dot" aria-hidden="true"></span><input type="text" class="hex-input" data-custom-hex value="${esc(currentHex)}" maxlength="7" aria-label="Custom accent hex"></div>${p.accent !== defAccent ? `<button type="button" class="link small" data-look="accent" data-value="${esc(defAccent)}">Reset to default</button>` : ""}</div>
      <p class="small faint">${isBlueDefault ? "Default uses Indigo." : "Default uses Monochrome graphite."} Custom colors automatically preserve legible contrast.</p>
      ${bgPicker}
      <h3 id="mot-h">Motion</h3><div class="seg" role="radiogroup" aria-labelledby="mot-h">${Theme.MOTIONS.map(([k, l]) => radio("motion", k, l)).join("")}</div>
      <p class="small muted" data-motion-hint>${esc(Theme.MOTION_HINTS[p.motion])}${look.motion !== p.motion ? " Your computer asks for reduced motion, so Magnus uses quick fades." : ""}</p>
      <p class="small faint">Magnus also follows your computer's setting for reduced motion.</p></div>`);
  };
  // Redraw keeps keyboard focus on the choice that had it.
  const redraw = () => {
    const a = document.activeElement?.dataset;
    draw();
    if (a?.look) $(`[data-look="${a.look}"][data-value="${a.value}"]`, el)?.focus({ preventScroll: true });
  };
  el.onclick = e => {
    const b = e.target.closest("[data-look]");
    if (!b) return;
    b.focus({ preventScroll: true });
    if (b.dataset.look === "style") {
      const nextStyle = b.dataset.value, p = Theme.prefs(), patch = { style: nextStyle };
      const nextDef = Theme.get?.(nextStyle)?.defaultAccent || "blue";
      if (nextDef === "blue" && p.accent === "graphite") patch.accent = "blue";
      else if (nextDef === "graphite" && p.accent === "blue") patch.accent = "graphite";
      Theme.set(patch);
      return;
    }
    Theme.set({ [b.dataset.look]: b.dataset.value });
  };
  el.oninput = e => {
    const hex = e.target.closest("[data-custom-hex]");
    if (hex && /^#[0-9a-fA-F]{6}$/.test(hex.value.trim())) Theme.set({ accent: hex.value.trim().toUpperCase() });
  };
  el.onchange = e => {
    const inp = e.target.closest("[data-custom-accent]");
    if (inp) Theme.set({ accent: inp.value.toUpperCase() });
  };
  const changed = () => { if (el.isConnected) redraw(); };
  window.addEventListener("magnus-theme", changed);
  onLeave(() => window.removeEventListener("magnus-theme", changed));
  draw();
}

// The owner's kill switch, and what it revoked.
async function accountSection(el) {
  const draw = async () => {
    const { sessions } = await Server.sessions.list();
    paint(el, `<h2>Account</h2>
      <div class="card"><div class="spread"><div><strong>${esc(App.me.name)}</strong> <span class="faint">(${esc(App.me.id)})</span>
        <div class="small muted">Signed in to ${esc(App.appliance)}${App.me.roles?.length ? ` · roles: ${esc(App.me.roles.join(", "))}` : ""}${App.me.groups?.length ? ` · groups: ${esc(App.me.groups.join(", "))}` : ""}</div></div>
        <button data-act="sign-out">Sign out</button></div></div>
      <div data-password></div><div data-totp></div><div data-timezone></div><div data-mail></div><div data-account-privacy></div>
      <h3>Devices</h3>
      ${sessions.length ? `<table class="t" data-devices><tbody>${sessions.map(x => `<tr data-session="${esc(x.id)}"><td>${x.client === "app" ? "Magnus app" : x.client === "browser" ? "Browser" : "Device"}${x.current ? ` <span class="chip ok">this device</span>` : ""}</td>
        <td class="small muted">signed in ${esc(when(x.created))} · until ${esc(new Date(x.expires).toLocaleDateString())}</td>
        <td>${x.current ? "" : `<button class="link" data-act="end" data-id="${esc(x.id)}">Sign out</button>`}</td></tr>`).join("")}</tbody></table>`
        : `<p class="empty">Signed in with an access key; no devices to list.</p>`}
      <p class="small muted" data-guide-again>Setting up? <a href="#/start">Open the getting started guide</a>: connect your apps, set up team chat and schedule a first workflow.</p>
      ${administers() ? `<p class="small muted" data-roles-moved>People's roles are set in <a href="#/admin/people">Admin › People</a>.</p>` : ""}`);
    const pw = $("[data-password]", el);
    if (pw && !pw.childElementCount) { await passwordCard(pw); await mailCard($("[data-mail]", el)); if (typeof accountExtras === "function") await accountExtras(el); }
  };
  el.onclick = async e => {
    const b = e.target.closest("button[data-act]"); if (!b) return;
    try {
      if (b.dataset.act === "sign-out") return $("nav.side [data-act=sign-out]")?.click();
      if (b.dataset.act === "end") { b.disabled = true; await Server.sessions.end(b.dataset.id); toast("That device is signed out."); }
      el._html = null; await draw();
    } catch (err) { toast(err.message); b.disabled = false; }
  };
  await draw();
}
