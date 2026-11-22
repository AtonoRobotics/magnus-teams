// Connecting an app from a web browser
// (decisions/2026-11-21-connect-on-the-web.md). The desktop app catches a
// sign-in's answer on this computer; a browser cannot. So this tab goes to
// the app's own sign-in page, and the app sends it back to the server's own
// address, /connected. That address is this app: it takes the answer out of
// the address at once, finishes the sign-in in the person's own session,
// and goes back to where they connected from.
"use strict";

const ConnectWeb = (() => {
  const KEY = "magnus-connecting";
  /** How long a sign-in started here waits for its answer, as the server's. */
  const WAITS_MS = 15 * 60_000;
  const local = () => location.protocol === "http:" && ["127.0.0.1", "localhost", "[::1]"].includes(location.hostname);

  /**
   * Where this browser's sign-in to an app comes back to, if it can: only
   * for an app whose provider takes an address Magnus names (web_sign_in),
   * the server's public return address when it is this page's own origin,
   * or, for a page on the server's own computer, its own /connected.
   * @param {any} card
   */
  function returnFor(card) {
    if (!card?.web_sign_in) return null;
    try { if (card.web_return && new URL(card.web_return).origin === location.origin) return card.web_return; } catch { /* not an address */ }
    return local() ? `${location.origin}/connected` : null;
  }

  /** Why an app cannot be connected from this browser. @param {any} card @param {string} name */
  function why(card, name) {
    const title = card?.title || name;
    if (card?.web_sign_in && card.web_return) {
      try { return `Connect ${title} at ${new URL(card.web_return).host}, or from the Magnus desktop app.`; } catch { /* not an address */ }
    }
    if (card?.web_sign_in) return `${title} can be connected from a browser once this server has its public address. Until then, connect it from the Magnus desktop app.`;
    return `${title} can be connected only from the Magnus desktop app: its sign-in comes back only to the desktop app.`;
  }

  /**
   * Goes to the app's sign-in page in this tab; begin(redirect) starts the
   * sign-in at the server. What to finish is kept for this tab only.
   * @param {{ name: string, title: string, ws?: string }} app
   * @param {string} redirect
   * @param {(redirect: string) => Promise<{ authorize_url: string, state: string }>} begin
   */
  async function go(app, redirect, begin) {
    const r = await begin(redirect);
    try {
      sessionStorage.setItem(KEY, JSON.stringify({ ...app, state: r.state, back: location.hash, at: Date.now() }));
    } catch {
      throw new Error("This browser keeps nothing for this tab, so the sign-in could not come back here. Connect from the Magnus desktop app.");
    }
    location.assign(r.authorize_url);
  }

  // The answer comes back in this page's address: it is taken out before
  // anything else reads the address, and the page shows where the person
  // connected from.
  /** @type {?{ q: URLSearchParams, w: any }} */
  let answer = null;
  if (location.pathname === "/connected") {
    let w = null;
    try { w = JSON.parse(sessionStorage.getItem(KEY) || "null"); sessionStorage.removeItem(KEY); } catch { w = null; }
    answer = { q: new URLSearchParams(location.search), w };
    const back = w?.back || (w && !w.ws ? `#/library/${encodeURIComponent(w.name)}` : "");
    history.replaceState(null, "", `/${back}`);
  }

  /**
   * Once the person is signed in to Magnus: finishes the sign-in the
   * provider sent this tab back with, if it did. The connected card is
   * drawn again by done().
   * @param {() => void} done
   */
  async function finish(done) {
    if (!answer) return;
    const { q, w } = answer;
    answer = null;
    if (!w || !w.state || q.get("state") !== w.state || Date.now() - (w.at || 0) > WAITS_MS) {
      toast("That sign-in came back for another request, or too late. Try again.");
      return;
    }
    if (q.get("error") || !q.get("code")) {
      toast(`${w.title} was not connected: ${q.get("error_description") || q.get("error") || "its sign-in sent nothing back"}.`);
      return;
    }
    const body = { state: w.state, code: q.get("code"), ...(q.get("iss") ? { iss: q.get("iss") } : {}) };
    try {
      if (w.ws) await Server.workspaces.appSignInComplete(w.ws, w.name, body);
      else await Server.connectors.completeSignIn(w.name, body);
      toast(w.ws ? `${w.title} is connected for the workspace.` : "Connected.");
    } catch (e) {
      toast(/** @type {any} */ (e).body?.message || /** @type {Error} */ (e).message);
    }
    done();
  }

  return { returnFor, why, go, finish };
})();
