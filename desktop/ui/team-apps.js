// Team Admins connect a workspace account, independently of their own apps.
async function teamApps(center, ws) {
  const base = `/v1/workspaces/${enc(ws)}`, apps = await api("GET", `${base}/apps`);
  const host = document.createElement("section");
  host.innerHTML = `<h2>Workspace apps</h2><p class="small faint">The team agent uses these workspace accounts. Your personal sign-ins stay private.</p>` + apps.map(c => {
    const roles = c.roles.length ? c.roles : ["member", "admin", "owner"];
    return `<div class="card flat" data-team-app="${esc(c.name)}"><h3>${esc(c.title || c.name)}</h3><p class="small">${esc(c.account || c.state.replaceAll("_", " "))}</p>
      <fieldset><legend>Roles that may use this app</legend>${["member", "admin", "owner"].map(r => `<label><input type="checkbox" value="${r}"${roles.includes(r) ? " checked" : ""}>${esc(teamRoleLabel(r))}</label>`).join("")}</fieldset>
      <button data-team-connect>${c.state === "connected" && c.roles.length ? "Update access" : "Connect workspace account"}</button>
      ${c.roles.length ? '<button data-team-disconnect>Disconnect</button>' : ""}</div>`;
  }).join("");
  $(".team-scroll", center).append(host);
  host.onclick = async e => {
    const connect = e.target.closest("[data-team-connect]"), disconnect = e.target.closest("[data-team-disconnect]");
    if (!connect && !disconnect) return;
    const row = e.target.closest("[data-team-app]"), c = apps.find(c => c.name === row.dataset.teamApp);
    const roles = [...row.querySelectorAll("input:checked")].map(i => i.value);
    const buttons = [...row.querySelectorAll("button")]; buttons.forEach(b => b.disabled = true);
    try {
      if (disconnect) {
        await api("POST", `${base}/connectors`, { app: c.name, roles: [] });
      } else {
        if (!roles.length) throw new Error("Choose a role, or disconnect this app.");
        const appBase = `${base}/apps/${enc(c.name)}`;
        if (!c.roles.length || c.state !== "connected") {
          const terms = await api("GET", `${appBase}/terms`);
          if (!await connectScreen({ ...terms, title: `${terms.title} for ${Team.home.name}` })) return;
          await api("POST", `${appBase}/consent`, { terms: terms.terms });
        }
        await api("POST", `${base}/connectors`, { app: c.name, roles });
        if (c.signs_in && c.state !== "connected") {
          if (!Magnus.loopbackOpen) throw new Error("Connect workspace accounts from the Magnus desktop app.");
          if (!c.ready) throw new Error("This provider's sign-in is not configured on the server.");
          const caught = await Magnus.loopbackOpen(c.handoff || (onThisComputer(c.redirect_uri) ? c.redirect_uri : null));
          const redirect = c.redirect_uri && !onThisComputer(c.redirect_uri) ? c.redirect_uri : caught;
          const flow = await api("POST", `${appBase}/sign-in`, { redirect_uri: redirect });
          await Magnus.openExternal(flow.authorize_url);
          const got = await Magnus.loopbackWait();
          if (got.state !== flow.state) throw new Error("That sign-in belongs to another request.");
          await api("POST", `${appBase}/sign-in/complete`, { state: got.state, code: got.code, iss: got.iss });
        }
      }
      toast(disconnect ? "Workspace app disconnected." : "Workspace app updated.");
      host.remove(); await teamApps(center, ws);
    } catch (err) { toast(err.message); }
    finally { buttons.forEach(b => b.disabled = false); }
  };
}
