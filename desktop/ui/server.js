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
  const pc = (/** @type {Id} */ id) => `/v1/computers/${enc(id)}`;
  /** A file's path on a computer, always given, the root as "". */
  const atPath = (/** @type {string} */ p) => `?path=${enc(p)}`;

  /** The path that presses or releases a kill switch: the server's, or a level's.
   * @param {string} level @param {?string} target @param {"press"|"release"} act */
  const killPath = (level, target, act) => level === "server" ? `/v1/admin/kill-switch/${act}` : `/v1/admin/kill-switch/${level}/${enc(target || "")}/${act}`;

  return {
    call,

    /** Which kill switches affect the signed-in person. */
    stops: { list: (/** @type {CallOptions} */ opts) => get("/v1/stops", opts) },

    /** Who is signed in. */
    me: () => get("/v1/me"),
    /** What this server runs. @param {CallOptions} [opts] */
    version: opts => get("/v1/version", opts),
    /** Search everything of the person's. */
    search: (/** @type {string} */ q) => get(`/v1/search?q=${enc(q)}`),
    /** Read aloud: one sentence's audio. */
    speak: (/** @type {string} */ text) => post("/v1/speak", { text }),
    /** Voice input: what was said, written down by the server. */
    transcribe: (/** @type {string} */ data_b64) => post("/v1/transcribe", { data_b64 }),
    /** The person opened Home: the updates since they last did. */
    homeOpened: () => post("/v1/home/opened"),
    /** Explore: trigger a read-only background scan for fresh ideas in For you. */
    explore: { scan: () => post("/v1/explore/scan") },
    /** User profile display avatar. */
    profile: {
      setAvatar: (/** @type {any} */ avatar) => put("/v1/profile/avatar", avatar),
    },

    tasks: {
      get: (/** @type {Id} */ id) => get(`/v1/tasks/${enc(id)}`),
      timeline: (/** @type {Id} */ id) => get(`/v1/tasks/${enc(id)}/timeline`),
      cancel: (/** @type {Id} */ id) => post(`/v1/tasks/${enc(id)}/cancel`),
      resume: (/** @type {Id} */ id) => post(`/v1/tasks/${enc(id)}/resume`),
      steer: (/** @type {Id} */ id, /** @type {string} */ text) => post(`/v1/tasks/${enc(id)}/steer`, { text }),
      stopAll: () => post("/v1/tasks/stop-all"),
      /** One of a result's actions, started as a new request. @param {Id} id @param {number|string} index @param {any} [body] */
      action: (id, index, body) => call("POST", `/v1/tasks/${enc(id)}/actions/${enc(index)}`, body),
      verdict: (/** @type {Id} */ id, /** @type {string} */ verdict, /** @type {string} */ note) => post(`/v1/tasks/${enc(id)}/verdict`, { verdict, note }),
    },

    inbox: {
      /** Say which items the person has now seen. */
      opened: (/** @type {Id[]} */ ids) => post("/v1/inbox/opened", { ids }),
      answer: (/** @type {Id} */ id, /** @type {any} */ body) => post(`/v1/inbox/${enc(id)}`, body),
    },

    staged: {
      /** Open staged drafts/actions with service read-back and destination provenance. */
      list: () => get("/v1/staged"),
      /** Resolve a staged item. @param {Id} id @param {{outcome: "sent"|"discarded"|"edited"|"sent_externally"|"deleted_externally", scope?: "once"|"recipient"|"domain"}} body */
      resolve: (id, body) => post(`/v1/staged/${enc(id)}`, body),
    },

    /** Apps the agent made, drawn in the conversation, and the elements they may use. */
    ui: {
      app: (/** @type {Id} */ id) => get(`/v1/ui/apps/${enc(id)}`),
      /** A tool call an app asks for. @param {Id} id @param {{tool: string, arguments: any}} body */
      appRequest: (id, body) => post(`/v1/ui/apps/${enc(id)}/request`, body),
      catalog: () => get("/v1/ui/catalog"),
      /** @param {{version: string, elements: string[]}} body */
      setCatalog: body => put("/v1/ui/catalog", body),
    },

    files: {
      /** @param {{q?: string, project?: Id}} [filter] */
      list: (filter = {}) => get(`/v1/files${query(filter)}`),
      info: (/** @type {Id} */ id) => get(`/v1/files/${enc(id)}/info`),
      /** @param {{name: string, media_type: string, data_b64: string, thread?: Id, project?: Id}} body */
      upload: body => post("/v1/files", body),
      /** Move a file or a conversation into a project, or out of one. @param {Id} item @param {?Id} project */
      move: (item, project) => post("/v1/moves", { item, project }),
    },

    trash: {
      list: () => get("/v1/trash"),
      add: (/** @type {Id} */ item) => post("/v1/trash", { item }),
      restore: (/** @type {Id} */ item) => post(`/v1/trash/${enc(item)}/restore`),
      remove: (/** @type {Id} */ item) => del(`/v1/trash/${enc(item)}`),
      empty: () => post("/v1/trash/empty"),
    },

    projects: {
      get: (/** @type {Id} */ id) => get(`/v1/projects/${enc(id)}`),
      /** @param {{name: string, instructions: string}} body */
      create: body => post("/v1/projects", body),
      update: (/** @type {Id} */ id, /** @type {any} */ body) => put(`/v1/projects/${enc(id)}`, body),
    },

    /** The models the person's agent runs on, and their providers' sign-ins. */
    models: {
      list: () => get("/v1/models"),
      choose: (/** @type {string} */ adapter, /** @type {string} */ model) => put("/v1/model", { adapter, model }),
      signIn: (/** @type {Id} */ adapter) => post(`/v1/adapters/${enc(adapter)}/sign-in`),
      /** @param {Id} adapter @param {{flow: string, input: any}} body */
      completeSignIn: (adapter, body) => post(`/v1/adapters/${enc(adapter)}/sign-in/complete`, body),
      conformance: (/** @type {Id} */ adapter) => post(`/v1/adapters/${enc(adapter)}/conformance`),
    },

    sessions: {
      list: () => get("/v1/sessions"),
      end: (/** @type {Id} */ id) => del(`/v1/sessions/${enc(id)}`),
    },

    preferences: {
      get: () => get("/v1/preferences"),
      set: (/** @type {Record<string, any>} */ changes) => put("/v1/preferences", changes),
    },

    /** What the agent keeps about the person, and what they asked it to forget. */
    memory: {
      deletionRequests: () => get("/v1/deletion-requests"),
      askToDelete: (/** @type {string} */ what) => post("/v1/deletion-requests", { what }),
      relationship: () => get("/v1/relationship"),
      /** @param {{action: "add"|"change"|"remove", note?: Id, text?: string}} body */
      note: body => post("/v1/relationship", body),
      disagreements: () => get("/v1/disagreements"),
      commitments: () => get("/v1/commitments"),
      updateCommitment: (/** @type {Id} */ id, /** @type {any} */ body) => put(`/v1/commitments/${enc(id)}`, body),
      removeCommitment: (/** @type {Id} */ id) => del(`/v1/commitments/${enc(id)}`),
      grants: () => get("/v1/grants"),
      setGrant: (/** @type {Id} */ id, /** @type {any} */ body) => put(`/v1/grants/${enc(id)}`, body),
      removeGrant: (/** @type {Id} */ id) => del(`/v1/grants/${enc(id)}`),
    },

    schedules: {
      /** Creates a recurring schedule (every_minutes or daily_at). */
      create: (/** @type {{text: string, every_minutes?: number, daily_at?: string, until?: string, max_runs?: number, project?: Id}} */ body) => post("/v1/schedules", body),
      /** Deletes a schedule: it is gone, and a run of it still going stops. */
      remove: (/** @type {Id} */ id) => del(`/v1/schedules/${enc(id)}`),
      /** Stops a schedule: no more runs, and a run still going stops; it stays, ended. */
      stop: (/** @type {Id} */ id) => post(`/v1/schedules/${enc(id)}/stop`),
      resume: (/** @type {Id} */ id) => post(`/v1/schedules/${enc(id)}/resume`),
      pause: (/** @type {Id} */ id) => post(`/v1/schedules/${enc(id)}/pause`),
    },

    /** The person's own chat with Magnus. */
    chat: {
      threads: () => get("/v1/chat"),
      send: (/** @type {{text: string, model?: string, files?: Id[], thread?: Id, project?: Id}} */ body) => post("/v1/chat", body),
      /** One of an entry's actions: resend, branch, share, save, spoken, reaction, task, delete.
       * @param {Id} thread @param {Id} entry @param {string} act @param {any} [body] */
      entry: (thread, entry, act, body = {}) => post(`/v1/chat/${enc(thread)}/entries/${enc(entry)}/${act}`, body),
      /** Answer an approval card. @param {Id} card @param {any} body */
      answerCard: (card, body) => post(`/v1/chat/cards/${enc(card)}`, body),
    },

    /** Everyone else on the server, for sharing. */
    directory: () => get("/v1/directory"),

    /** The person's own rules for their agent. */
    rules: {
      list: () => get("/v1/rules"),
      /** Accept a rule the agent proposed. */
      accept: (/** @type {Id} */ id) => call("POST", `/v1/rules/${enc(id)}/accept`),
      remove: (/** @type {Id} */ id) => del(`/v1/rules/${enc(id)}`),
      /** A new rule, or a change to one. @param {?Id} id @param {any} body */
      save: (id, body) => id ? put(`/v1/rules/${enc(id)}`, body) : post("/v1/rules", body),
    },

    /** The person's own connections to apps. */
    connectors: {
      get: (/** @type {string} */ name) => get(`/v1/connectors/${enc(name)}`),
      /** Whether the app can push to this server, and how. */
      push: (/** @type {string} */ name) => get(`/v1/connectors/${enc(name)}/push`),
      setToken: (/** @type {string} */ name, /** @type {string} */ token) => put(`/v1/connectors/${enc(name)}/token`, { token }),
      removeGenerated: (/** @type {string} */ name) => del(`/v1/connectors/${enc(name)}/generated`),
      disconnect: (/** @type {string} */ name) => del(`/v1/connectors/${enc(name)}/connection`),
      signIn: (/** @type {string} */ name, /** @type {string} */ redirect) => post(`/v1/connectors/${enc(name)}/sign-in`, { redirect_uri: redirect }),
      /** @param {string} name @param {{state: string, code: string, iss?: string}} body */
      completeSignIn: (name, body) => post(`/v1/connectors/${enc(name)}/sign-in/complete`, body),
      terms: (/** @type {string} */ name) => get(`/v1/connectors/${enc(name)}/terms`),
      consent: (/** @type {string} */ name, /** @type {string} */ terms) => post(`/v1/connectors/${enc(name)}/consent`, { terms }),
    },

    /** Outside agents the person connected (another organisation's agents), beside their apps. */
    outsideAgents: {
      list: () => get("/v1/outside-agents"),
      /** Connect one by its agent card's address, with the credential its card asks for. @param {{card_url: string, credential?: string, client_id?: string, client_secret?: string}} body */
      connect: body => post("/v1/outside-agents", body),
      /** Let it ask the person, or a shared project as a guest. @param {Id} id @param {{person: string}|{project: string}} to */
      admit: (id, to) => post(`/v1/outside-agents/${enc(id)}/admit`, { to }),
      disconnect: (/** @type {Id} */ id) => del(`/v1/outside-agents/${enc(id)}`),
    },

    account: {
      password: () => get("/v1/account/password"),
      setPassword: (/** @type {string} */ current, /** @type {string} */ next) => post("/v1/account/password", { current_password: current, new_password: next }),
      totp: () => get("/v1/account/totp"),
      beginTotp: () => post("/v1/account/totp/begin"),
      confirmTotp: (/** @type {string} */ code, /** @type {string} */ current_password = "") => post("/v1/account/totp/confirm", { code, current_password }),
      removeTotp: (/** @type {string} */ current_password) => del("/v1/account/totp", { current_password }),
      exportData: () => get("/v1/account/export"),
      deleteAccount: (/** @type {string} */ confirm) => del("/v1/account", { confirm }),
      /** The server's mail settings. */
      mail: () => get("/v1/mail"),
      testMail: () => post("/v1/mail/test"),
      /** Store a credential in the vault. */
      setSecret: (/** @type {string} */ name, /** @type {string} */ value) => put(`/v1/secrets/${enc(name)}`, { value }),
    },

    /** Team messages to tell the person about, since `after` (an ISO time) or all. */
    notices: (/** @type {?string} */ after) => get(`/v1/team/notices${query({ after })}`),

    /** The terms, the privacy notice, usage data and the tester program. */
    legal: {
      status: () => get("/v1/legal"),
      /** @param {{terms: string, privacy: string}} body */
      terms: body => post("/v1/legal/terms", body),
      /** @param {{share: boolean, version: string}} body */
      usage: body => post("/v1/legal/usage", body),
      /** @param {"join"|"leave"|"decline"} act @param {{version: string}} body */
      testers: (act, body) => post(`/v1/legal/testers/${act === "join" ? "join" : act === "leave" ? "leave" : "decline"}`, body),
    },

    payments: {
      get: () => get("/v1/payments"),
      /** A link to connect a payment account. */
      link: () => call("POST", "/v1/payments/link"),
      unlink: () => del("/v1/payments/link"),
      removeCard: () => del("/v1/payments/card"),
      setCard: (/** @type {any} */ card) => put("/v1/payments/card", card),
    },

    /** The person's computers the desktop app bridges to their programs. */
    desktop: {
      devices: () => get("/v1/desktop/devices"),
      allowProgram: (/** @type {Id} */ device, /** @type {string} */ program, /** @type {boolean} */ allowed) => put(`/v1/desktop/devices/${enc(device)}/programs/${enc(program)}`, { allowed }),
    },

    spend: {
      /** Ask an admin for more to spend. @param {number} usd @param {string} reason */
      ask: (usd, reason) => post("/v1/spend/requests", { usd, reason }),
    },

    /** The agent's computers on the server. */
    computers: {
      list: () => get("/v1/computers"),
      get: (/** @type {Id} */ id) => get(pc(id)),
      screen: (/** @type {Id} */ id) => get(`${pc(id)}/screen`),
      activity: (/** @type {Id} */ id) => get(`${pc(id)}/activity`),
      start: (/** @type {Id} */ id) => post(`${pc(id)}/start`),
      hibernate: (/** @type {Id} */ id) => post(`${pc(id)}/hibernate`),
      wake: (/** @type {Id} */ id) => post(`${pc(id)}/wake`),
      /** The kill switch: pause it and take back every access. */
      pause: (/** @type {Id} */ id) => post(`${pc(id)}/kill`, { mode: "pause" }),
      reset: (/** @type {Id} */ id, /** @type {boolean} */ keepFiles) => post(`${pc(id)}/reset`, { keep_files: keepFiles }),
      takeover: (/** @type {Id} */ id) => post(`${pc(id)}/takeover`),
      handback: (/** @type {Id} */ id) => post(`${pc(id)}/handback`),
      /** What handing back would close. */
      handbackCloses: (/** @type {Id} */ id) => get(`${pc(id)}/handback/closes`),
      setTheme: (/** @type {Id} */ id, /** @type {string} */ theme) => put(`${pc(id)}/theme`, { theme }),
      listing: (/** @type {Id} */ id, /** @type {string} */ path) => get(`${pc(id)}/listing${atPath(path)}`),
      file: (/** @type {Id} */ id, /** @type {string} */ path) => get(`${pc(id)}/files${atPath(path)}`),
      writeFile: (/** @type {Id} */ id, /** @type {string} */ path, /** @type {string} */ dataB64) => put(`${pc(id)}/files`, { path, data_b64: dataB64 }),
      removeFile: (/** @type {Id} */ id, /** @type {string} */ path) => del(`${pc(id)}/files${atPath(path)}`),
      /** Type into a sign-in form on the computer, outside the agent's sight.
       * @param {Id} identity @param {{target: string, value: string, submit: boolean, save: boolean, username?: string}} body */
      secureForm: (identity, body) => post(`/v1/identities/${enc(identity)}/secure-form`, body),
    },

    /** Computers of the person's own that the desktop app connects. */
    local: {
      list: () => get("/v1/local/computers"),
      resume: (/** @type {Id} */ id) => post(`/v1/local/computers/${enc(id)}/resume`),
      remove: (/** @type {Id} */ id) => del(`/v1/local/computers/${enc(id)}`),
      removeFile: (/** @type {Id} */ id, /** @type {string} */ path) => del(`/v1/local/computers/${enc(id)}/files${atPath(path)}`),
    },

    workspaces: {
      list: () => get("/v1/workspaces"),
      create: (/** @type {string} */ name) => post("/v1/workspaces", { name }),
      remove: (/** @type {Id} */ w) => del(ws(w)),
      join: (/** @type {string} */ code) => post(`/v1/invites/${enc(code)}/join`),
      home: (/** @type {Id} */ w) => get(`${ws(w)}/home`),
      capabilities: (/** @type {Id} */ w) => get(`${ws(w)}/capabilities`),
      missions: (/** @type {Id} */ w, /** @type {Id} */ room) => get(`${ws(w)}/rooms/${enc(room)}/missions`),
      createMission: (/** @type {Id} */ w, /** @type {Id} */ room, /** @type {any} */ body) => post(`${ws(w)}/rooms/${enc(room)}/missions`, body),
      releaseLane: (/** @type {Id} */ w, /** @type {Id} */ m, /** @type {Id} */ l, /** @type {any} */ body = {}) => post(`${ws(w)}/missions/${enc(m)}/lanes/${enc(l)}/release`, body),
      steerLane: (/** @type {Id} */ w, /** @type {Id} */ m, /** @type {Id} */ l, /** @type {string} */ instruction) => post(`${ws(w)}/missions/${enc(m)}/lanes/${enc(l)}/steer`, { instruction }),
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
      /** Grant an app to the workspace for these roles; none takes it away. @param {Id} w @param {string} app @param {string[]} roles */
      grantApp: (w, app, roles) => post(`${ws(w)}/connectors`, { app, roles }),
      /** Start the workspace's own sign-in to an app (a Team Admin's). @param {Id} w @param {string} app @param {string} redirect_uri */
      appSignIn: (w, app, redirect_uri) => post(`${ws(w)}/connectors/${enc(app)}/sign-in`, { redirect_uri }),
      /** @param {Id} w @param {string} app @param {{state: string, code: string, iss?: string}} body */
      appSignInComplete: (w, app, body) => post(`${ws(w)}/connectors/${enc(app)}/sign-in/complete`, body),
      appDisconnect: (/** @type {Id} */ w, /** @type {string} */ app) => del(`${ws(w)}/connectors/${enc(app)}/connection`),
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
      /** @param {Id} w @param {{channel: Id, text: string, every_hours: number}} body */
      addRoutine: (w, body) => post(`${ws(w)}/routines`, body),
      /** @param {Id} w @param {Id} id @param {{channel?: Id, text?: string, every_hours?: number}} body */
      updateRoutine: (w, id, body) => patch(`${ws(w)}/routines/${enc(id)}`, body),
      removeRoutine: (/** @type {Id} */ w, /** @type {Id} */ id) => del(`${ws(w)}/routines/${enc(id)}`),
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

    /** The developer view (dev.js). */
    dev: {
      /** @param {URLSearchParams} filter status, failure, experiment, ids */
      runs: filter => get(`/v1/dev/runs?${filter}`),
      run: (/** @type {Id} */ id) => get(`/v1/dev/runs/${enc(id)}`),
      /** A page of a run's log, after the event numbered `after`. @param {Id} id @param {number} after @param {number} rows */
      runLog: (id, after, rows) => get(`/v1/dev/runs/${enc(id)}/log?after=${enc(after)}&limit=${enc(rows)}`),
      experiments: () => get("/v1/dev/experiments"),
      /** @param {Id} id @param {URLSearchParams} [want] k, base */
      experiment: (id, want) => get(`/v1/dev/experiments/${enc(id)}${want?.size ? `?${want}` : ""}`),
      metrics: () => get("/v1/dev/metrics"),
      policy: () => get("/v1/dev/policy"),
      toolScreens: () => get("/v1/dev/tool-screens"),
      playbook: () => get("/v1/dev/playbook"),
      computers: () => get("/v1/dev/computers"),
      pauseComputer: (/** @type {Id} */ id) => post(`/v1/dev/computers/${enc(id)}/pause`),
      startComputer: (/** @type {Id} */ id) => post(`/v1/dev/computers/${enc(id)}/start`),
      /** @param {Id} id @param {number} after the last row seen */
      egress: (id, after) => get(`/v1/dev/computers/${enc(id)}/egress?after=${enc(after)}`),
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
