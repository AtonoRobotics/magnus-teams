// The page's one client of its server (decisions/2026-11-14-desktop-server-client.md).
// Each request the page makes is a named call here, grouped by what it acts
// on: `Server.schedules.remove(id)`, `Server.workspaces.addTask(ws, body)`.
// This file builds every path, encodes every id and holds the one error
// policy; a script never spells out a method or a route. It sends through
// the host's transport, `Magnus.api` (bridge.js): the app's Rust side or the
// browser's fetch. Checks put an answer table in its place
// (desktop/tests/server-harness.mjs).
"use strict";

/**
 * @typedef {"GET"|"POST"|"PUT"|"PATCH"|"DELETE"} Method
 * @typedef {{background?: boolean, stepped?: boolean}} CallOptions
 *   background: not tied to the view on screen (a poll): sent as it is,
 *   with no step-up, no spend offer and no "view changed".
 * @typedef {string} Id
 */

const Server = (() => {
  const enc = encodeURIComponent;
  /** Methods that change nothing: a lost reply to one leaves nothing unknown. */
  const SAFE = ["GET", "HEAD", "OPTIONS"];

  const cancelled = message => Object.assign(new Error(message), { cancelled: true });

  /** Who is signed in where, and which view is drawn: a read answered after
   * any of them changed belongs to a view no longer on screen. */
  function stamp() {
    if (typeof App === "undefined") return "";
    return JSON.stringify([App.me?.id, App.appliance, App.sessionGeneration, App.seq]);
  }

  /** Send one request under the page's one error policy:
   * - 426: the app is older than the server takes; the page shows its update screen.
   * - 402 with a spend limit of the person's own: offer to ask for more (admin-spend.js).
   * - 403 step_up: an admin change the server holds for the password again
   *   (admin.js): ask once, then send it once more; cancelled, it throws a
   *   cancelled error and nothing is sent.
   * - A change whose reply was lost or failed on the server: "Outcome not confirmed".
   * - A read answered after the view changed: a cancelled error.
   * @param {Method} method @param {string} path @param {any} [body] @param {CallOptions} [opts]
   * @returns {Promise<any>} */
  async function call(method, path, body, opts = {}) {
    const before = stamp();
    let result;
    try { result = await Magnus.api(method, path, body); }
    catch (error) {
      if (error?.status === 426 && typeof window !== "undefined") window.dispatchEvent(new CustomEvent("magnus-update-required", { detail: error.body }));
      if (opts.background) throw error;
      if (error?.status === 402 && error.body?.spend_limit && typeof offerMoreSpend === "function") offerMoreSpend(error.body);
      if (error?.status === 403 && error.body?.error === "step_up" && !opts.stepped && typeof stepUp === "function") {
        if (!(await stepUp())) throw cancelled("The change was not made.");
        return call(method, path, body, { ...opts, stepped: true });
      }
      // A lost mutation reply does not establish whether its effect happened.
      if (!SAFE.includes(method) && (!error?.status || error.status >= 500))
        throw Object.assign(new Error(`Outcome not confirmed. ${error?.message || "Check the current state before trying again."}`), { status: error?.status, body: error?.body });
      throw error;
    }
    if (method === "GET" && !opts.background && before !== stamp()) throw cancelled("This view changed.");
    return result;
  }

  /** A query string from the pairs that have a value, with its "?".
   * @param {Record<string, any>} q */
  const query = q => { const s = Object.entries(q).filter(([, v]) => v !== undefined && v !== null && v !== "").map(([k, v]) => `${k}=${enc(v)}`).join("&"); return s ? `?${s}` : ""; };
  const get = (p, opts) => call("GET", p, undefined, opts);
  const post = (p, b = {}) => call("POST", p, b);
  const put = (p, b) => call("PUT", p, b);
  const patch = (p, b) => call("PATCH", p, b);
  const del = (p, b) => call("DELETE", p, b);

  const ws = (/** @type {Id} */ w) => `/v1/workspaces/${enc(w)}`;
  const channel = (/** @type {Id} */ w, /** @type {Id} */ c) => `${ws(w)}/channels/${enc(c)}`;
  const message = (/** @type {Id} */ w, /** @type {Id} */ c, /** @type {Id} */ m) => `${channel(w, c)}/messages/${enc(m)}`;
  const knowledge = (/** @type {Id} */ w, /** @type {Id} */ k) => `${ws(w)}/knowledge/${enc(k)}`;
  const person = (/** @type {Id} */ p) => `/v1/admin/people/${enc(p)}`;

  /** The path that presses or releases a kill switch: the server's, or a level's.
   * @param {string} level @param {?string} target @param {"press"|"release"} act */
  const killPath = (level, target, act) => level === "server" ? `/v1/admin/kill-switch/${act}` : `/v1/admin/kill-switch/${level}/${enc(target || "")}/${act}`;

  return {
    call,

    /** Which kill switches affect the signed-in person. */
    stops: { list: (/** @type {CallOptions} */ opts) => get("/v1/stops", opts) },

    tasks: {
      cancel: (/** @type {Id} */ id) => post(`/v1/tasks/${enc(id)}/cancel`),
      resume: (/** @type {Id} */ id) => post(`/v1/tasks/${enc(id)}/resume`),
    },

    schedules: {
      /** Stops a schedule that is running; deletes one that has ended. */
      remove: (/** @type {Id} */ id) => del(`/v1/schedules/${enc(id)}`),
      resume: (/** @type {Id} */ id) => post(`/v1/schedules/${enc(id)}/resume`),
      pause: (/** @type {Id} */ id) => post(`/v1/schedules/${enc(id)}/pause`),
    },

    /** The person's own chat with Magnus. */
    chat: {
      send: (/** @type {{text: string, model?: string, files?: Id[], thread?: Id, project?: Id}} */ body) => post("/v1/chat", body),
      fileInfo: (/** @type {Id} */ file) => get(`/v1/files/${enc(file)}/info`),
      /** One of an entry's actions: resend, branch, share, save, spoken, reaction, task, delete.
       * @param {Id} thread @param {Id} entry @param {string} act @param {any} [body] */
      entry: (thread, entry, act, body = {}) => post(`/v1/chat/${enc(thread)}/entries/${enc(entry)}/${act}`, body),
      /** Answer an approval card. @param {Id} card @param {any} body */
      answerCard: (card, body) => post(`/v1/chat/cards/${enc(card)}`, body),
    },

    /** Everyone else on the server, for sharing. */
    directory: () => get("/v1/directory"),

    spend: {
      /** Ask an admin for more to spend. @param {number} usd @param {string} reason */
      ask: (usd, reason) => post("/v1/spend/requests", { usd, reason }),
    },

    computers: {
      get: (/** @type {Id} */ id) => get(`/v1/computers/${enc(id)}`),
      screen: (/** @type {Id} */ id) => get(`/v1/computers/${enc(id)}/screen`),
    },

    workspaces: {
      list: () => get("/v1/workspaces"),
      create: (/** @type {string} */ name) => post("/v1/workspaces", { name }),
      remove: (/** @type {Id} */ w) => del(ws(w)),
      join: (/** @type {string} */ code) => post(`/v1/invites/${enc(code)}/join`),
      home: (/** @type {Id} */ w) => get(`${ws(w)}/home`),
      activity: (/** @type {Id} */ w) => get(`${ws(w)}/activity`),
      /** The side panel, for a project or the whole workspace. @param {Id} w @param {?Id} [project] */
      panel: (w, project) => get(`${ws(w)}/panel${query({ project })}`),
      search: (/** @type {Id} */ w, /** @type {string} */ q) => get(`${ws(w)}/search${query({ q })}`),
      /** Open a direct conversation. @param {Id} w @param {{agent?: boolean, people?: Id[]}} body */
      direct: (w, body) => post(`${ws(w)}/direct`, body),
      /** Start a channel or a project. @param {Id} w @param {{kind: string, name: string}} body */
      start: (w, body) => post(`${ws(w)}/start`, body),
      /** Invite by email, or make a link with no email. @param {Id} w @param {string} [email] */
      invite: (w, email) => post(`${ws(w)}/invites`, email === undefined ? {} : { email }),
      /** @param {Id} w @param {{usd: number, channel?: Id}} body */
      setLimit: (w, body) => post(`${ws(w)}/limit`, body),
      people: (/** @type {Id} */ w) => get(`${ws(w)}/people`),
      /** Add a member, or change their role. @param {Id} w @param {Id} who @param {string} role */
      setMember: (w, who, role) => post(`${ws(w)}/members`, { person: who, role }),
      removeMember: (/** @type {Id} */ w, /** @type {Id} */ who) => del(`${ws(w)}/members/${enc(who)}`),
      /** @param {Id} w @param {{title: string, project?: Id, status?: string, from?: any}} body */
      addTask: (w, body) => post(`${ws(w)}/tasks`, body),
      updateTask: (/** @type {Id} */ w, /** @type {Id} */ id, /** @type {any} */ body) => put(`${ws(w)}/tasks/${enc(id)}`, body),
      removeTask: (/** @type {Id} */ w, /** @type {Id} */ id) => del(`${ws(w)}/tasks/${enc(id)}`),
      setProjectMembers: (/** @type {Id} */ w, /** @type {Id} */ project, /** @type {Id[]} */ members) => put(`${ws(w)}/projects/${enc(project)}`, { members }),
      removeFile: (/** @type {Id} */ w, /** @type {Id} */ file) => del(`${ws(w)}/files/${enc(file)}`),
      knowledge: (/** @type {Id} */ w, /** @type {Id} */ project) => get(`${ws(w)}/knowledge${query({ project })}`),
      /** @param {Id} w @param {{statement: string, kind?: string, project?: Id}} body */
      addKnowledge: (w, body) => post(`${ws(w)}/knowledge`, body),
      correctKnowledge: (/** @type {Id} */ w, /** @type {Id} */ id, /** @type {string} */ statement) => post(`${knowledge(w, id)}/correct`, { statement }),
      flagKnowledge: (/** @type {Id} */ w, /** @type {Id} */ id, /** @type {string} */ why) => post(`${knowledge(w, id)}/flag`, { why }),
      retireKnowledge: (/** @type {Id} */ w, /** @type {Id} */ id) => post(`${knowledge(w, id)}/retire`),
      /** Share Magnus's private answer, or a whisper, with the channel. */
      shareAnswer: (/** @type {Id} */ w, /** @type {Id} */ id) => post(`${ws(w)}/answers/${enc(id)}/share`),
      shareWhisper: (/** @type {Id} */ w, /** @type {Id} */ id) => post(`${ws(w)}/whispers/${enc(id)}/share`),
    },

    /** A team channel or direct conversation. */
    channels: {
      view: (/** @type {Id} */ w, /** @type {Id} */ c) => get(`${channel(w, c)}/view`),
      whispers: (/** @type {Id} */ w, /** @type {Id} */ c) => get(`${channel(w, c)}/whispers`),
      read: (/** @type {Id} */ w, /** @type {Id} */ c) => post(`${channel(w, c)}/read`),
      typing: (/** @type {Id} */ w, /** @type {Id} */ c) => post(`${channel(w, c)}/typing`),
      /** @param {Id} w @param {Id} c @param {{text: string, thread?: Id, whisper?: boolean, model?: string}} body */
      post: (w, c, body) => post(`${channel(w, c)}/messages`, body),
      archive: (/** @type {Id} */ w, /** @type {Id} */ c, /** @type {boolean} */ on) => post(`${channel(w, c)}/archive`, { on }),
      clear: (/** @type {Id} */ w, /** @type {Id} */ c) => post(`${channel(w, c)}/clear`),
      /** A channel action the command bar or the header names (rename, topic, members...).
       * @param {Id} w @param {Id} c @param {string} what @param {any} body */
      act: (w, c, what, body) => post(`${channel(w, c)}/${what}`, body),
      react: (/** @type {Id} */ w, /** @type {Id} */ c, /** @type {Id} */ m, /** @type {string} */ emoji, /** @type {boolean} */ on) => post(`${message(w, c, m)}/react`, { emoji, on }),
    },

    /** One message in a team channel. */
    messages: {
      edit: (/** @type {Id} */ w, /** @type {Id} */ c, /** @type {Id} */ m, /** @type {string} */ text) => put(message(w, c, m), { text }),
      retry: (/** @type {Id} */ w, /** @type {Id} */ c, /** @type {Id} */ m) => post(`${message(w, c, m)}/retry`),
      share: (/** @type {Id} */ w, /** @type {Id} */ c, /** @type {Id} */ m, /** @type {Id} */ to) => post(`${message(w, c, m)}/share`, { person: to }),
      save: (/** @type {Id} */ w, /** @type {Id} */ c, /** @type {Id} */ m) => post(`${message(w, c, m)}/save`),
      pin: (/** @type {Id} */ w, /** @type {Id} */ c, /** @type {Id} */ m, /** @type {boolean} */ pinned) => post(`${message(w, c, m)}/pin`, { pinned }),
      remove: (/** @type {Id} */ w, /** @type {Id} */ c, /** @type {Id} */ m) => del(message(w, c, m)),
    },

    admin: {
      standing: (/** @type {CallOptions} */ opts) => get("/v1/admin/standing", opts),
      stepUp: (/** @type {string} */ password, /** @type {?string} */ code) => post("/v1/admin/step-up", { password, code }),
      overview: () => get("/v1/admin/overview"),
      killSwitch: () => get("/v1/admin/kill-switch"),
      killSwitches: () => get("/v1/admin/kill-switches"),
      /** Pressing needs no step-up; releasing may. @param {string} level @param {?Id} target */
      press: (level, target) => post(killPath(level, target, "press")),
      release: (/** @type {string} */ level, /** @type {?Id} */ target) => post(killPath(level, target, "release")),
      agents: () => get("/v1/admin/agents"),
      pauseAgent: (/** @type {Id} */ id) => post(`/v1/admin/agents/${enc(id)}/pause`),
      resumeAgent: (/** @type {Id} */ id) => post(`/v1/admin/agents/${enc(id)}/resume`),
      connectors: () => get("/v1/admin/connectors"),
      /** @param {string} name @param {{enabled?: boolean, access_groups?: string[]}} changes */
      updateConnector: (name, changes) => patch(`/v1/admin/connectors/${enc(name)}`, changes),
      setConnectorCredential: (/** @type {string} */ name, /** @type {string} */ value) => put(`/v1/admin/connectors/${enc(name)}/credential`, { value }),
      updates: () => get("/v1/admin/updates"),
      install: () => post("/v1/admin/updates/install"),
      rollback: () => post("/v1/admin/updates/rollback"),
      /** Which channel the server, or the desktop app, updates from. @param {"server"|"desktop"} which @param {string} name */
      setUpdateChannel: (which, name) => put(`/v1/admin/updates/${which === "server" ? "server" : "desktop"}-channel`, { channel: name }),
      backups: () => get("/v1/admin/backups"),
      backUp: () => post("/v1/admin/backups"),
      computers: () => get("/v1/admin/computers"),
      health: () => get("/v1/admin/health"),
      people: () => get("/v1/admin/people"),
      /** @param {{name: string, email: string, role: string, send: boolean}} body */
      addPerson: body => post("/v1/admin/people", body),
      setRole: (/** @type {Id} */ id, /** @type {string} */ role) => patch(person(id), { role }),
      resendInvite: (/** @type {Id} */ id) => post(`${person(id)}/invite`, { send: true }),
      cancelInvite: (/** @type {Id} */ id) => post(`${person(id)}/cancel-invite`),
      signOut: (/** @type {Id} */ id) => post(`${person(id)}/sign-out`),
      disable: (/** @type {Id} */ id) => post(`${person(id)}/disable`),
      enable: (/** @type {Id} */ id) => post(`${person(id)}/enable`),
      /** @param {Id} id @param {any} body what becomes of their work */
      removePerson: (id, body) => del(person(id), body),
      setTeamAdmin: (/** @type {Id} */ id, /** @type {boolean} */ on) => put(`${person(id)}/team-admin`, { on }),
      workspaces: () => get("/v1/admin/workspaces"),
      policy: () => get("/v1/admin/policy"),
      policyHistory: (/** @type {string} */ layer) => get(`/v1/admin/policy/history${query({ layer })}`),
      draftPolicy: (/** @type {string} */ layer, /** @type {string} */ cedar) => post("/v1/admin/policy/draft", { layer, cedar }),
      applyPolicy: (/** @type {string} */ layer, /** @type {string} */ cedar) => post("/v1/admin/policy/apply", { layer, cedar }),
      rules: () => get("/v1/admin/rules"),
      /** @param {{effect: string, connector: string, tool: string, destinations: string[], words: string}} body */
      addRule: body => post("/v1/admin/rules", body),
      removeRule: (/** @type {Id} */ id) => del(`/v1/admin/rules/${enc(id)}`),
      setAutoReview: (/** @type {boolean} */ acts) => put("/v1/admin/auto-review", { acts }),
      signInPolicy: () => get("/v1/admin/sign-in-policy"),
      setSignInPolicy: (/** @type {any} */ body) => put("/v1/admin/sign-in-policy", body),
      /** @param {string} [filter] a query string without its "?" */
      audit: filter => get(`/v1/admin/audit${filter ? `?${filter}` : ""}`),
      auditExport: (/** @type {string} */ filter) => get(`/v1/admin/audit/export${filter ? `?${filter}` : ""}`),
      retention: () => get("/v1/admin/retention"),
      /** @param {{trash_days: number, backup_days: number}} body */
      setRetention: body => put("/v1/admin/retention", body),
      developerAccess: () => get("/v1/admin/developer-access"),
      identityProvider: () => get("/v1/admin/identity-provider"),
      testIdentityProvider: (/** @type {any} */ body) => post("/v1/admin/identity-provider/test", body),
      pollIdentityProviderTest: () => post("/v1/admin/identity-provider/test/poll"),
      /** Use the provider just tested, or none. @param {any} provider */
      setIdentityProvider: provider => put("/v1/admin/identity-provider", { provider }),
      /** @param {string} [month] YYYY-MM */
      spend: month => get(`/v1/admin/spend${query({ month })}`),
      setSpendLimit: (/** @type {any} */ body) => post("/v1/admin/spend/limits", body),
      /** @param {Id} id @param {{approve: boolean, usd: ?number, note: string}} body */
      answerSpendRequest: (id, body) => post(`/v1/admin/spend/requests/${enc(id)}`, body),
      models: () => get("/v1/admin/models"),
      providers: () => get("/v1/admin/providers"),
      /** @param {Id} id @param {{enabled?: boolean, default_model?: string}} changes */
      updateProvider: (id, changes) => patch(`/v1/admin/providers/${enc(id)}`, changes),
      testProvider: (/** @type {Id} */ id) => post(`/v1/admin/providers/${enc(id)}/test`),
      /** @param {any} body {person|group: who, models} */
      setModelAccess: body => post("/v1/admin/models/access", body),
      testers: () => get("/v1/admin/testers"),
      inviteTester: (/** @type {Id} */ who) => post("/v1/admin/testers/invite", { person: who }),
    },
  };
})();
