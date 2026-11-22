// Shared user identity avatar renderer (default initials, preset avatars,
// or uploaded photo) used across Rooms, chat, People rail, video tiles, and
// Work lanes (teams_redesign_spec.md Phase 3).
"use strict";

const UserAvatar = (() => {
  const PRESETS = [
    { id: "aurora-indigo", label: "Indigo", bg: "linear-gradient(135deg,#7C8CFF,#3B5BDB)" },
    { id: "aurora-emerald", label: "Emerald", bg: "linear-gradient(135deg,#34D399,#059669)" },
    { id: "aurora-amber", label: "Amber", bg: "linear-gradient(135deg,#FBBF24,#D97706)" },
    { id: "aurora-rose", label: "Rose", bg: "linear-gradient(135deg,#FB7185,#E11D48)" },
    { id: "aurora-silver", label: "Silver", bg: "linear-gradient(135deg,#E4E4E7,#52525B)" },
  ];

  /** @param {string} name */
  function initials(name) {
    return String(name || "?").trim().split(/\s+/).map(w => w[0] || "").join("").slice(0, 2).toUpperCase() || "?";
  }

  /**
   * Renders a user's avatar (`default`, `preset`, or `photo`) with safe fallback.
   * @param {string} name
   * @param {{ kind?: "default"|"preset"|"photo", id?: string, blob_id?: string } | null | undefined} avatar
   * @param {string} [size]
   */
  function html(name, avatar, size = "md") {
    const sz = size === "sm" ? " user-avatar-sm" : size === "lg" ? " user-avatar-lg" : "";
    const letters = esc(initials(name));
    if (avatar && avatar.kind === "photo" && avatar.blob_id) {
      const src = `/v1/files/${enc(avatar.blob_id)}`;
      return `<span class="user-avatar${sz} is-photo" title="${esc(name)}"><img src="${esc(src)}" alt="${esc(name)}" loading="lazy"></span>`;
    }
    if (avatar && avatar.kind === "preset" && avatar.id) {
      const p = PRESETS.find(x => x.id === avatar.id) || PRESETS[0];
      return `<span class="user-avatar${sz} is-preset" style="background:${p.bg}" aria-hidden="true" title="${esc(name)}">${letters}</span>`;
    }
    return `<span class="user-avatar${sz}" aria-hidden="true" title="${esc(name)}">${letters}</span>`;
  }

  /**
   * Renders a compact inline avatar chooser (Default initials, Presets, or Photo).
   * @param {string} name @param {any} current
   */
  function pickerHtml(name, current) {
    const kind = current?.kind || "default";
    const presetId = current?.id || "";
    const opts = PRESETS.map(p => `<button type="button" class="avatar-opt${kind === "preset" && presetId === p.id ? " on" : ""}" data-avatar-preset="${esc(p.id)}" title="${esc(p.label)}">${html(name, { kind: "preset", id: p.id }, "sm")}</button>`).join("");
    return `<div class="avatar-picker" role="group" aria-label="Choose your avatar">
      <button type="button" class="avatar-opt${kind === "default" ? " on" : ""}" data-avatar-default title="Default initials">${html(name, { kind: "default" }, "sm")}</button>
      ${opts}
    </div>`;
  }

  return { PRESETS, initials, html, pickerHtml };
})();
