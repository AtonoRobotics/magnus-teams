// The four looks, in the installed app (U7, amended): light and dark, each
// solid and glass, chosen in Settings > Appearance. In each look the chrome
// (the sidebar, the panel beside the conversation) is glass only when glass
// is chosen; the conversation and what is read in it stay solid; and every
// text on screen reaches 4.5:1 against what is under it, glass measured over
// the worst case behind it: the system's material over a black or white
// desktop, or the backdrop the page draws where there is no material. Then: the accent, the
// motion setting, and the choice kept on this device across a restart of the
// page. Runs the same on Windows, where glass is the system's Mica or Acrylic.
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
// background (glass is any surface not opaque), and each text's contrast
// against the surfaces under it, laid over every backdrop that can be
// behind them wherever they let it through.
const MEASURE = `
  const rgba = c => { const p = (c.match(/[\\d.]+/g) || []).map(Number); return p.length === 3 ? [...p, 1] : p; };
  const over = (t, u) => [0, 1, 2].map(i => t[i] * t[3] + u[i] * (1 - t[3])).concat(1);
  const lum = c => { const [r, g, b] = c.slice(0, 3).map(v => { v /= 255; return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4; }); return 0.2126 * r + 0.7152 * g + 0.0722 * b; };
  const ratio = (a, b) => { const [x, y] = [lum(a), lum(b)].sort((p, q) => q - p); return (x + 0.05) / (y + 0.05); };
  // The layers under an element, top first, down to the first opaque one.
  const under = el => { const ls = []; for (let e = el; e && e !== document.documentElement; e = e.parentElement) { const c = rgba(getComputedStyle(e).backgroundColor); if (c[3] === 0) continue; ls.push(c); if (c[3] === 1) return ls; } return ls; };
  // Behind glass: the system's material over anything on the desktop (its
  // tint over black, grey and white), or the drawn wallpaper's colors where
  // the page draws its own backdrop.
  const glass = Theme.GLASS[document.documentElement.dataset.mode];
  const behind = document.documentElement.dataset.backdrop === "native"
    ? [[0, 0, 0, 1], [128, 128, 128, 1], [255, 255, 255, 1]].map(d => over(rgba(glass.system), d))
    : [...glass.backdrop.matchAll(/#([0-9a-f]{6})/gi)].map(m => [0, 2, 4].map(i => parseInt(m[1].slice(i, i + 2), 16)).concat(1));
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
await step('Appearance offers mode, material, accent and motion only',async()=>{
  await appearance();
  for(const value of ['light','dark','system'])await xpath(`//button[@data-look='mode' and @data-value='${value}']`);
  if(await run("return document.querySelectorAll('[data-look=theme],[data-follow-computer]').length"))throw Error('Named presets or Follow computer are still offered');
  for(const value of ['solid','glass'])await xpath(`//button[@data-look='material' and @data-value='${value}']`);
  for(const value of ['full','reduced','off'])await xpath(`//button[@data-look='motion' and @data-value='${value}']`);
});
for(const mode of ['light','dark'])for(const material of ['solid','glass']){
  await step(`${mode} ${material}`,async()=>{
    await appearance();await choose('mode',mode);await choose('material',material);await reading();
    const m=JSON.parse(await run(MEASURE));
    if(m.look.mode!==mode || m.look.material!==material)throw Error(`Unexpected appearance ${JSON.stringify(m.look)}`);
    if(material==='glass' && !(m.side<1 && m.sideBlur.includes('blur')))throw Error(`Chrome glass lost: ${JSON.stringify(m)}`);
    if(m.panel!==1 || (material==='solid' && m.side!==1))throw Error('Reading panel or solid chrome is translucent');
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
await step('custom accent and no-motion preferences persist',async()=>{
  await appearance();await choose('mode','dark');await choose('accent','teal');await choose('motion','off');await reading();
  if(await run("return getComputedStyle(document.getElementById('send')).backgroundColor")!=='rgb(31, 122, 109)')throw Error('The chosen Teal accent is not rendered on Send');
  if(await run('return document.getAnimations().length'))throw Error('Motion Off still animates');
  await run('location.reload()');await css('#msg');
  const look=await run('return {...document.documentElement.dataset}');
  if(look.mode!=='dark' || look.material!=='glass' || look.accent!=='teal' || look.motion!=='off')throw Error(`Lost preference ${JSON.stringify(look)}`);
});
await d.close();
console.log("looks passed");
