// A release's notes as the app shows them: What's new after an update, and
// Settings › About.
//
// The notes are the release's commit message. Its lines are wrapped short,
// "- " starts an item, and it ends in trailers such as Co-Authored-By. The
// app shows sentences whole, items as a list, and no trailers.

/**
 * @param {?string} text
 * @returns {string} HTML
 */
function releaseNotesHtml(text) {
  /** @type {{ item: boolean, text: string }[]} */
  const blocks = [];
  /** @type {?{ item: boolean, text: string }} */
  let cur = null;
  for (const raw of String(text || "").split("\n")) {
    const line = raw.trim();
    if (!line) { cur = null; continue; }
    if (/^[A-Za-z]+(?:-[A-Za-z]+)*-by:\s/i.test(line)) continue;
    const item = /^[-*•]\s+(.*)$/.exec(line);
    if (item) { cur = { item: true, text: item[1] }; blocks.push(cur); continue; }
    if (cur) cur.text += ` ${line}`;
    else { cur = { item: false, text: line }; blocks.push(cur); }
  }
  if (!blocks.length) return "<p>Fixes and improvements.</p>";
  let html = "";
  for (let i = 0; i < blocks.length; i++) {
    if (!blocks[i].item) { html += `<p>${esc(blocks[i].text)}</p>`; continue; }
    html += "<ul>";
    for (; i < blocks.length && blocks[i].item; i++) html += `<li>${esc(blocks[i].text)}</li>`;
    html += "</ul>";
    i--;
  }
  return html;
}
