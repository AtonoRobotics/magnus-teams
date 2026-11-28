// Add people to a personal project: one step. The person picks people from
// the server's directory, or types the email of someone not on it, and
// presses Share. The server makes the shared project from the project (its
// name, instructions and files) with them in it (POST /v1/projects/{id}/share);
// an email is then invited to it. Then the shared project opens.
"use strict";

const ProjectShare = (() => {
  /** Emails typed into the box: separated by commas, spaces or new lines. */
  function emails(typed) {
    return String(typed || "").split(/[\s,;]+/).map(s => s.trim()).filter(s => /^[^@\s]+@[^@\s]+\.[^@\s]+$/.test(s));
  }

  /** The dialog's markup. @param {{id: string, name: string}[]} people @param {string} name */
  function dialogHtml(people, name) {
    return `<form method="dialog" class="share-form">
      <div class="share-dialog-head"><h2>Add people to ${esc(name)}</h2><button type="button" class="icon-btn" data-share-close aria-label="Close">${icon("close")}</button></div>
      ${people.length ? `<div class="share-pick-list" role="group" aria-label="People on this server">${people.map(p => `<label class="room-pick small"><input type="checkbox" name="person" value="${esc(p.id)}"> ${esc(p.name)}</label>`).join("")}</div>` : ""}
      <label class="stack small"><span>Email</span><input type="text" name="emails" inputmode="email" placeholder="name@example.com" autocomplete="off"></label>
      <div class="row share-actions"><button type="submit" class="primary" data-share-go disabled>Share</button></div>
    </form>`;
  }

  /**
   * Share the project with the people picked and the emails typed: the
   * shared project's address.
   * @param {string} id @param {string[]} people @param {string[]} invite
   * @returns {Promise<string>}
   */
  async function share(id, people, invite) {
    const { workspace } = await Server.projects.share(id, people);
    const notes = [];
    for (const email of invite) {
      try {
        const r = await Server.workspaces.invite(workspace, email);
        // Only what the person must do: send a link no email carried.
        if (!r?.added && !r?.emailed && typeof inviteOutcome === "function") notes.push(inviteOutcome(email, r));
      } catch (err) { notes.push(`${email}: ${err.message}`); }
    }
    if (notes.length) toast(notes.join(" "), 10000);
    return `#/team/${enc(workspace)}`;
  }

  /** Open the dialog for a personal project. @param {string} id @param {string} name */
  async function open(id, name) {
    const directory = await Server.directory().catch(() => ({ people: [] }));
    const people = (Array.isArray(directory) ? directory : directory?.people || []).filter(p => p.id && p.id !== App.me?.id);
    const d = document.createElement("dialog");
    d.className = "share-dialog";
    d.setAttribute("aria-label", `Add people to ${name}`);
    d.innerHTML = dialogHtml(people, name);
    const go = /** @type {HTMLButtonElement} */ ($("[data-share-go]", d));
    const picked = () => [...d.querySelectorAll('input[name="person"]:checked')].map(el => /** @type {HTMLInputElement} */ (el).value);
    const typed = () => emails(/** @type {HTMLInputElement} */ ($('input[name="emails"]', d)).value);
    const ready = () => { go.disabled = !picked().length && !typed().length; };
    d.addEventListener("input", ready);
    d.addEventListener("change", ready);
    d.addEventListener("click", e => { if (e.target === d || /** @type {HTMLElement} */ (e.target).closest("[data-share-close]")) d.close(); });
    $("form", d)?.addEventListener("submit", async e => {
      e.preventDefault();
      go.disabled = true;
      try {
        const next = await share(id, picked(), typed());
        d.close();
        if (typeof refresh === "function") await refresh().catch(() => {});
        location.hash = next;
      } catch (err) {
        toast(err.message);
        ready();
      }
    });
    d.addEventListener("close", () => d.remove());
    document.body.append(d);
    d.showModal();
  }

  return { open, share, emails, dialogHtml };
})();
