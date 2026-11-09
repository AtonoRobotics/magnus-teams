import assert from 'node:assert/strict';
import {createRequire} from 'node:module';import {join,dirname} from 'node:path';import {fileURLToPath} from 'node:url';
const root=join(dirname(fileURLToPath(import.meta.url)),'../..'),{chromium}=createRequire(import.meta.url)(join(root,'browser/node_modules/playwright-core'));
const browser=await chromium.launch({executablePath:process.env.MAGNUS_CHROMIUM,args:['--no-sandbox']}),page=await browser.newPage({viewport:{width:1440,height:900}}),id=process.env.MAGNUS_OBSERVATION;
const until=async fn=>{for(let end=Date.now()+15000;Date.now()<end;await page.waitForTimeout(40))if(await fn())return;throw Error('opened presentation condition timed out');};
const item=()=>page.evaluate(async id=>(await Magnus.api('GET','/v1/inbox')).items.find(i=>i.id===id),id);
let attempts=0,release;const held=new Promise(resolve=>release=resolve);
try{
 await page.goto(process.env.MAGNUS_URL);await page.getByLabel('Username').fill('alice');await page.getByLabel('Password').fill(process.env.MAGNUS_PASSWORD);await page.getByRole('button',{name:'Sign in',exact:true}).click();await page.locator('#msg').waitFor();await page.evaluate(()=>{background.forEach(clearInterval);background=[];});
 await page.route('**/v1/inbox/opened',async route=>{attempts++;assert.ok(route.request().postDataJSON().ids.includes(id));if(attempts<=2)return route.fulfill({status:503,contentType:'application/json',body:'{"error":"temporary presentation reporting failure"}'});await held;await route.continue();});
 await page.getByRole('link',{name:'Activity',exact:true}).click();const updates=page.locator('details[data-key=other]');await updates.waitFor();await page.evaluate(()=>refresh());assert.equal((await item()).opened_at,null);assert.equal(attempts,0,'collapsed updates are not presented');
 await updates.locator('summary').click();await page.locator(`article[data-id="${id}"]`).scrollIntoViewIfNeeded();await until(()=>attempts===1);assert.equal((await item()).opened_at,null);await until(()=>page.evaluate(()=>App.reporting.size===0));await page.evaluate(()=>refresh());await until(()=>attempts===2);await until(()=>page.evaluate(()=>App.reporting.size===0));assert.equal((await item()).opened_at,null);
 await updates.locator('summary').click();await page.evaluate(()=>refresh());assert.equal(attempts,2,'failed reports are not retried for collapsed content');
 await updates.locator('summary').click();await page.locator(`article[data-id="${id}"]`).scrollIntoViewIfNeeded();await until(()=>attempts===3);await page.evaluate(()=>refresh());await page.evaluate(()=>refresh());assert.equal(attempts,3,'overlapping redraws do not repeat in-flight reports');release();await until(async()=>typeof (await item()).opened_at==='string');const first=(await item()).opened_at;
 await page.evaluate(()=>refresh());await updates.locator('summary').click();await updates.locator('summary').click();await page.evaluate(()=>refresh());assert.equal(attempts,3);assert.equal((await item()).opened_at,first);assert.equal((await item()).status,'open');assert.equal(await page.evaluate(()=>needsMe().some(i=>i.kind==='observation')),false);
 console.log('actual Activity first-opened metadata, collapsed-content protection, retry, in-flight deduplication and unchanged decision state passed');
}finally{release();await browser.close();}
