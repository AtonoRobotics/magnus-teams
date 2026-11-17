// The two looks, in the installed app: light and dark, chosen in Settings >
// Appearance. In each look every surface is solid (the sidebar, the panel
// beside the conversation, the conversation and what is read in it), nothing
// blurs what is behind it, and every text on screen reaches 4.5:1 against
// what is under it. Then: the accent, the motion setting, and the choice kept
// on this device across a restart of the page.
//
// MAGNUS_WEBDRIVER  tauri-driver's address      MAGNUS_APP  the app binary
// MAGNUS_URL        the appliance (https)       MAGNUS_FINGERPRINT  what it printed
// MAGNUS_PROVIDER   the identity provider's name on the sign-in button
// MAGNUS_PERSON     who signs in
// MAGNUS_SHOTS      optional: screenshots, one per look
import { env, open, pinAndSignIn } from "./appdriver.mjs";

const [wd, app, url, fingerprint, provider, person] =
  ["MAGNUS_WEBDRIVER", "MAGNUS_APP", "MAGNUS_URL", "MAGNUS_FINGERPRINT", "MAGNUS_PROVIDER", "MAGNUS_PERSON"].map(env);
const d = await open(wd, app, process.env.MAGNUS_SHOTS, "looks");
const { step, css, xpath, click, type, run, until } = d;

await pinAndSignIn(d, { url, fingerprint, provider, person });

const appearance = async () => {
  await click(xpath("//nav//a[@aria-label='Settings']"));
  await click(xpath("//nav[@aria-label='Settings']//a[normalize-space()='Appearance']"));
  await xpath("//section[@data-section='appearance']//h2[normalize-space()='Appearance']");
};
const choose = async (label, value) => {
  await click(xpath(`//section[@data-section='appearance']//button[@role='radio' and @data-look='${label}' and @data-value='${value}']`));
  await until(`${label} ${value} chosen`, () => run("return document.querySelector(`[data-look='${arguments[0]}'][data-value='${arguments[1]}']`)?.getAttribute('aria-checked') === 'true'", label, value));
};

// Everything the page shows, measured as it is drawn: each surface's
// background, and each text's contrast against the surfaces under it, laid
// over the page's own background wherever they let it through.
const MEASURE = `
  const rgba = c => { const p = (c.match(/[\\d.]+/g) || []).map(Number); return p.length === 3 ? [...p, 1] : p; };
  const over = (t, u) => [0, 1, 2].map(i => t[i] * t[3] + u[i] * (1 - t[3])).concat(1);
  const lum = c => { const [r, g, b] = c.slice(0, 3).map(v => { v /= 255; return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4; }); return 0.2126 * r + 0.7152 * g + 0.0722 * b; };
  const ratio = (a, b) => { const [x, y] = [lum(a), lum(b)].sort((p, q) => q - p); return (x + 0.05) / (y + 0.05); };
  // The layers under an element, top first, down to the first opaque one.
  const under = el => { const ls = []; for (let e = el; e && e !== document.documentElement; e = e.parentElement) { const c = rgba(getComputedStyle(e).backgroundColor); if (c[3] === 0) continue; ls.push(c); if (c[3] === 1) return ls; } return ls; };
  // Under every surface: the page's own solid background.
  const behind = [rgba(getComputedStyle(document.documentElement).backgroundColor)];
  const backs = el => { const ls = under(el); const opaque = ls.length && ls[ls.length - 1][3] === 1; return (opaque ? [ls.pop()] : behind).map(b => ls.reduceRight((acc, l) => over(l, acc), b)); };
  const shown = el => { const r = el.getBoundingClientRect(); const s = getComputedStyle(el); return r.width > 0 && r.height > 0 && s.visibility !== "hidden" && s.opacity !== "0" && !el.closest(".sr, [hidden], option"); };
  const texts = root => [...root.querySelectorAll("*")].filter(el => [...el.childNodes].some(n => n.nodeType === 3 && n.textContent.trim()) && shown(el));
  const low = [];
  for (const root of document.querySelectorAll("nav.side, #detail .detail-head, main .conv-head, #thread, #composer")) for (const el of texts(root)) {
    const fg = rgba(getComputedStyle(el).color);
    for (const b of backs(el)) { const r = ratio(fg, b); if (r < 4.5) low.push(\`\${el.tagName.toLowerCase()}.\${el.className} "\${el.textContent.trim().slice(0, 30)}" \${r.toFixed(2)}:1\`); }
  }
  for(const el of document.querySelectorAll('.conv-head select, #composer select'))if(shown(el))for(const b of backs(el)){const r=ratio(rgba(getComputedStyle(el).color),b);if(r<4.5)low.push('Selected control '+el.getAttribute('aria-label')+' '+r.toFixed(2)+':1');}
  const alpha = q => { const el = document.querySelector(q); return el ? rgba(getComputedStyle(el).backgroundColor)[3] : null; };
  const filter = q => { const s = getComputedStyle(document.querySelector(q)); return s.backdropFilter || s.webkitBackdropFilter || "none"; };
  return JSON.stringify({ low, chromeScheme:getComputedStyle(document.querySelector('.conv-head')).colorScheme, side: alpha("nav.side"), panel: alpha("#detail"), sideBlur: filter("nav.side"), panelBlur: filter("#detail"),
    reading: ["main", "#composer .say", "#thread .msg.me", "#detail iframe.composed, #detail .card"].map(q => [q, alpha(q)]),
    look: { ...document.documentElement.dataset } });
`;

let fileId;
await step("an actual conversation and uploaded reading surface",async()=>{
  await type(css('#msg'),'Hello');await click(css('#send'));await xpath("//div[@id='thread']//div[@data-answer]");
  await run("window.__lookFile=null;Magnus.api('POST','/v1/files',{name:'Appearance reading.md',media_type:'text/markdown',data_b64:btoa('# Reading surface\\n\\nActual uploaded document.\\n\\n| Item | Value |\\n| --- | --- |\\n| Theme | Preserved |')}).then(r=>window.__lookFile=r.id,e=>window.__lookFile={error:e.message});");
  fileId=await until('file uploaded',()=>run('return window.__lookFile'));if(fileId.error)throw Error(fileId.error);
});
const reading=async()=>{
  await click(xpath("//nav//a[@data-tooltip='Hello' or @title='Hello' or normalize-space()='Hello']"));
  await click(xpath("//nav//a[@aria-label='Files']"));
  await click(xpath(`//a[@data-open='file:${fileId}']`));
  await until('document decoded',()=>run("const f=document.getElementById('fview');return f?.contentDocument?.body?.textContent.includes('Actual uploaded document.') && f.getBoundingClientRect().height>=f.contentDocument.body.scrollHeight"));
};
await step('Appearance offers theme, mode, accent and motion',async()=>{
  await appearance();
  for(const value of ['obsidian','prism'])await xpath(`//button[@data-look='style' and @data-value='${value}']`);
  for(const value of ['light','dark','system'])await xpath(`//button[@data-look='mode' and @data-value='${value}']`);
  if(await run("return document.querySelectorAll('[data-look=theme],[data-follow-computer]').length"))throw Error('Named presets or Follow computer are still offered');
  if(await run("return document.querySelectorAll('[data-look=material]').length"))throw Error('A material choice is still offered');
  for(const value of ['full','reduced','off'])await xpath(`//button[@data-look='motion' and @data-value='${value}']`);
  await choose('style','prism');
  const compLook=await run('return {...document.documentElement.dataset}');
  if(compLook.style!=='prism' || compLook.accent!=='blue')throw Error(`Prism did not switch to Indigo default: ${JSON.stringify(compLook)}`);
  await choose('style','obsidian');
  const agLook=await run('return {...document.documentElement.dataset}');
  if(agLook.style!=='obsidian' || agLook.accent!=='graphite')throw Error(`Obsidian did not switch to Monochrome default: ${JSON.stringify(agLook)}`);
});
for(const style of ['obsidian','prism'])for(const mode of ['light','dark']){
  await step(`${style}/${mode}`,async()=>{
    await appearance();await choose('style',style);await choose('mode',mode);await reading();
    const m=JSON.parse(await run(MEASURE));
    if(m.look.style!==style || m.look.mode!==mode || 'material' in m.look || 'backdrop' in m.look)throw Error(`Unexpected appearance ${JSON.stringify(m.look)}`);
    if(m.panel!==1 || m.side!==1)throw Error(`Chrome or the panel is translucent: ${JSON.stringify(m)}`);
    if(m.sideBlur!=='none' || m.panelBlur!=='none')throw Error(`Chrome blurs what is behind it: ${JSON.stringify(m)}`);
    for(const [what,alpha] of m.reading)if(alpha!==null && alpha!==1)throw Error(`Translucent reading surface ${what}`);
    if(m.low.length)throw Error(`Text below 4.5:1: ${m.low.join('; ')}`);
    const frame=await run("const f=document.getElementById('fview');return {mode:f.contentDocument.documentElement.style.colorScheme,text:f.contentDocument.body.textContent,sandbox:f.getAttribute('sandbox')} ");
    if(frame.mode!==mode || !frame.text.includes('Actual uploaded document.') || frame.sandbox!=='allow-same-origin')throw Error('Document theme/data/sandbox changed');
    await css('#msg');
  });
}
await step('explicit expansion and restore keep conversation reachable',async()=>{
  await click(css('[aria-label="Expand work"]'));await css('.workspace-selectors');
  await click(xpath("//div[contains(@class,'workspace-selectors')]//button[normalize-space()='Conversation']"));await css('#msg');
  await click(xpath("//div[contains(@class,'workspace-selectors')]//button[normalize-space()='Work']"));await css('#fview');
  await click(css('[aria-label="Restore split view"]'));await css('#msg');
});
await step('theme, custom accent and no-motion preferences persist',async()=>{
  await appearance();await choose('style','prism');await choose('mode','dark');await choose('accent','emerald');await choose('motion','off');await reading();
  if(await run("return getComputedStyle(document.getElementById('send')).backgroundColor")!=='rgb(52, 211, 153)')throw Error('The chosen Emerald accent is not rendered on Send');
  if(await run('return document.getAnimations().length'))throw Error('Motion Off still animates');
  await run('location.reload()');await css('#msg');
  const look=await run('return {...document.documentElement.dataset}');
  if(look.style!=='prism' || look.mode!=='dark' || 'material' in look || look.accent!=='emerald' || look.motion!=='off')throw Error(`Lost preference ${JSON.stringify(look)}`);
});
await d.close();
console.log("looks passed");
