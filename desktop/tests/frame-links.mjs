import assert from 'node:assert/strict';
import {createRequire} from 'node:module';import {join,dirname} from 'node:path';import {fileURLToPath} from 'node:url';
import {tableMarkdown,inspectLinks} from './frame-links-fixture.mjs';
const root=join(dirname(fileURLToPath(import.meta.url)),'../..'),{chromium}=createRequire(import.meta.url)(join(root,'browser/node_modules/playwright-core'));
const browser=await chromium.launch({executablePath:process.env.MAGNUS_CHROMIUM,args:['--no-sandbox','--disable-dev-shm-usage']});
const page=await browser.newPage({viewport:{width:1280,height:900}}),errors=[];page.on('pageerror',e=>errors.push(e.message));
function documentFrame(){return page.locator('#fview').elementHandle();}
const read=async()=>page.evaluate(inspectLinks,await documentFrame());
const until=async predicate=>{for(const end=Date.now()+10000;Date.now()<end;await page.waitForTimeout(40)){const s=await read();if(s && predicate(s))return s;}throw Error(`Link layout did not reconcile: ${JSON.stringify(await read())}`);};
try{
 await page.goto(process.env.MAGNUS_URL);await page.getByLabel('Username').fill('alice');await page.getByLabel('Password').fill(process.env.MAGNUS_PASSWORD);await page.getByRole('button',{name:'Sign in',exact:true}).click();await page.locator('#msg').waitFor();
 const first='https://first.example/reference',last='https://last.example/reference',text=tableMarkdown(first,last);
 const file=await page.evaluate(body=>Magnus.api('POST','/v1/files',body),{name:'scroll-links.md',media_type:'text/markdown',data_b64:Buffer.from(text).toString('base64')});
 await page.getByRole('link',{name:'Files',exact:true}).click();await page.locator(`[data-open="file:${file.id}"]`).first().click();
 const initial=await until(s=>s.links.length===2 && s.links[0].visible && s.links[0].hit===first);assert.ok(initial.max>500);assert.ok(initial.panelOverflow<=2 && initial.pageOverflow<=0,'offscreen frame links cannot create panel/page overflow');
 await page.evaluate(()=>{const t=document.querySelector('#fview').contentDocument.querySelector('.markdown-table');t.scrollLeft=t.scrollWidth;});
 await until(s=>s.links[1].visible && s.links[1].aligned && s.links[1].hit===last && !s.links[0].visible);
 await page.evaluate(()=>document.querySelector('#fview').contentDocument.querySelector('.markdown-table').scrollLeft=0);await until(s=>s.links[0].hit===first);
 await page.locator('.frame-actions a').first().focus();await page.keyboard.press('Tab');const focused=await until(s=>s.scroll>0 && s.links[1].fullyVisible && s.links[1].hit===last);assert.ok(focused.panelOverflow<=2 && focused.pageOverflow<=0);
 assert.ok(await page.locator('.frame-actions a').last().evaluate(e=>e===document.activeElement));assert.deepEqual(await page.locator('.frame-actions a').last().evaluate(e=>{const s=getComputedStyle(e);return[s.outlineStyle,s.outlineWidth,s.outlineOffset];}),['solid','2px','2px']);assert.equal(await page.locator('#fview').getAttribute('sandbox'),'allow-same-origin');
 assert.deepEqual(errors,[]);console.log('real frame table scrolling, clipped link hit targets, keyboard reveal and sandbox preservation passed');
}finally{await browser.close();}
