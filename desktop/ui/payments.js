// How the agent pays: the person's Link wallet, and a saved card for when
// Link cannot serve them. A card goes from here to the vault and is never shown again.
async function paymentsSection(el) {
  const draw = async () => {
    const s = await api("GET", "/v1/payments");
    if (!s.available) { paint(el, `<h2>Payments</h2><p class="muted">Payments are not set up on this server.</p>`); return; }
    paint(el, `<h2>Payments</h2>
      <p class="muted">The agent asks before it buys anything. It never sees a card number.</p>
      ${s.link_offered ? `<div class="card"><div class="spread"><div><strong>Link wallet</strong>
        <div class="small muted">A one-time card for each purchase you approve in Link. US and Canada only.</div></div>
        ${s.link_connected ? `<button data-pay="disconnect">Disconnect</button>` : `<button class="primary" data-pay="connect">Connect Link</button>`}</div></div>` : ""}
      <div class="card"><div class="spread"><div><strong>Saved card</strong>
        <div class="small muted">Used only when Link is not available. You approve every use.</div>
        ${s.card_last4 ? `<div class="small">Ending in ${esc(s.card_last4)}</div>` : ""}</div>
        ${s.card_last4 ? `<button data-pay="remove-card">Remove</button>` : ""}</div>
        ${s.card_last4 ? "" : `<form data-pay-form class="stack" autocomplete="off">
          <label>Card number <input name="number" inputmode="numeric" required></label>
          <div class="row"><label>Month <input name="exp_month" type="number" min="1" max="12" required></label>
          <label>Year <input name="exp_year" type="number" min="2000" max="2100" required></label>
          <label>Security code <input name="cvc" inputmode="numeric" required></label></div>
          <label>Name on card <input name="name"></label>
          <label>Postal code <input name="postal_code"></label>
          <button class="primary" type="submit">Save card</button></form>`}
      </div>
      <p class="small err" data-pay-note></p>`);
    const note = m => { $("[data-pay-note]", el).textContent = m; };
    const run = async fn => { try { await fn(); await draw(); } catch (e) { note(e.message || String(e)); } };
    el.querySelectorAll("[data-pay]").forEach(b => b.onclick = () => run(async () => {
      if (b.dataset.pay === "connect") { const { url } = await api("POST", "/v1/payments/link"); window.open(url, "_blank", "noopener"); }
      if (b.dataset.pay === "disconnect") await api("DELETE", "/v1/payments/link");
      if (b.dataset.pay === "remove-card") await api("DELETE", "/v1/payments/card");
    }));
    const form = $("[data-pay-form]", el);
    if (form) form.onsubmit = e => {
      e.preventDefault();
      const d = Object.fromEntries(new FormData(form));
      run(() => api("PUT", "/v1/payments/card", { ...d, exp_month: Number(d.exp_month), exp_year: Number(d.exp_year) }));
    };
  };
  await draw();
}

const paymentCard = i => i.details.url
  ? `<div class="what"><a class="btn primary" href="${esc(i.details.url)}" target="_blank" rel="noopener">Open Link</a></div>`
  : `<div class="what muted">Open your Link app to respond.</div>`;
