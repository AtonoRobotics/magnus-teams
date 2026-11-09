import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {join,dirname} from 'node:path';
import {fileURLToPath} from 'node:url';
const root=join(dirname(fileURLToPath(import.meta.url)),'../..');
const {chromium}=createRequire(import.meta.url)(join(root,'browser/node_modules/playwright-core'));
const browser=await chromium.launch({executablePath:process.env.MAGNUS_CHROMIUM,args:['--no-sandbox','--disable-dev-shm-usage']});
const context=await browser.newContext({permissions:['clipboard-read','clipboard-write']});
const page=await context.newPage(); const errors=[];page.on('pageerror',e=>errors.push(e.message));
try {
  await page.goto(process.env.MAGNUS_URL);
  await page.getByLabel('Username').fill(process.env.MAGNUS_PERSON);await page.getByLabel('Password').fill(process.env.MAGNUS_PASSWORD);
  await page.getByRole('button',{name:'Sign in',exact:true}).click();await page.locator('#msg').fill('Show the Markdown fixture');await page.locator('#send').click();
  const entry=page.locator('.entry[data-kind=answer]').last();await entry.getByRole('heading',{name:'Readable answer'}).waitFor();
  assert.equal(await page.getByRole('button',{name:'Send message',exact:true}).count(),1,'the send icon names the message operation independently');
  assert.equal(await entry.locator('.markdown strong').textContent(),'Bold');assert.equal(await entry.locator('table td').count(),2);
  assert.equal(await entry.locator('script, a[href^="javascript:"]').count(),0);assert.equal(await page.evaluate(()=>window.pwned),undefined);
  await entry.getByRole('button',{name:'Copy code'}).click();assert.equal(await page.evaluate(()=>navigator.clipboard.readText()),'const answer = 42;');
  assert.ok(await entry.locator('.msg-actions > button, .msg-actions > .message-menu-wrap > button').count()<=3);
  assert.equal(await entry.locator('[role=toolbar], .verdict').count(),0);
  await entry.evaluate(el=>{Theme.set({motion:'full'});el.classList.add('enter');});
  const more=entry.getByRole('button',{name:'More message actions'});await more.focus();await page.keyboard.press('Enter');
  const menu=entry.getByRole('menu');await menu.waitFor();const bounds=await menu.boundingBox();assert.ok(bounds.y>=0 && bounds.y+bounds.height<=720,'entry animation cannot displace the menu outside the viewport');assert.equal(await more.getAttribute('aria-expanded'),'true');
  assert.equal(await page.evaluate(()=>document.activeElement.textContent.trim()),'Retry');
  await page.keyboard.press('End');assert.equal(await page.evaluate(()=>document.activeElement.textContent.trim()),'Delete message');
  await page.keyboard.press('Home');await page.keyboard.press('ArrowDown');assert.equal(await page.evaluate(()=>document.activeElement.textContent.trim()),'Branch');
  await page.evaluate(()=>{ document.querySelector('#thread')._html=''; return App.redraw(); });
  assert.equal(await page.evaluate(()=>document.activeElement.textContent.trim()),'Branch','poll redraw preserves the open menu and its focused item');
  await page.keyboard.press('Escape');assert.equal(await more.getAttribute('aria-expanded'),'false');assert.ok(await more.evaluate(el=>el===document.activeElement));
  await more.click();await page.keyboard.press('Tab');assert.equal(await more.getAttribute('aria-expanded'),'false');assert.equal(await menu.isVisible(),false);
  await more.click();await page.locator('#msg').click();assert.equal(await menu.isVisible(),false);
  await more.click();await menu.getByRole('menuitem',{name:'Feedback',exact:true}).click();
  const dialog=page.getByRole('dialog',{name:'Feedback'});await dialog.waitFor();await dialog.getByRole('button',{name:'That was great'}).click();await dialog.waitFor({state:'hidden'});
  await more.click();await menu.getByRole('menuitem',{name:'Feedback',exact:true}).click();assert.equal(await dialog.getByRole('button',{name:'That was great'}).getAttribute('aria-pressed'),'true');
  await dialog.getByRole('button',{name:'Close feedback'}).click();
  // A cold hover, focus timing, Escape suppression and usable viewport bounds.
  const copy=entry.getByRole('button',{name:'Copy response',exact:true});await page.locator('#msg').focus();await page.mouse.move(0,0);await page.waitForTimeout(1100);
  await copy.hover();await page.waitForTimeout(200);assert.equal(await page.getByRole('tooltip').count(),0);
  await page.getByRole('tooltip').waitFor();assert.equal(await copy.getAttribute('title'),null);
  const box=await page.getByRole('tooltip').boundingBox();assert.ok(box.x>=0 && box.y>=0 && box.x+box.width<=1280);
  await page.keyboard.press('Escape');await page.waitForTimeout(200);assert.equal(await page.getByRole('tooltip').count(),0);
  await page.mouse.move(0,0);await copy.focus();await page.getByRole('tooltip').waitFor({timeout:1000});
  assert.ok((await copy.getAttribute('aria-describedby')).includes('magnus-tooltip'));
  await page.keyboard.press('Escape');await page.waitForTimeout(200);assert.equal(await page.getByRole('tooltip').count(),0);
  // Moving over the tooltip bridges its physical gap and keeps it readable.
  await page.locator('#msg').focus();await page.mouse.move(0,0);await copy.hover();await page.getByRole('tooltip').waitFor();const hoverTip=await page.getByRole('tooltip').boundingBox();await page.mouse.move(hoverTip.x+hoverTip.width/2,hoverTip.y+hoverTip.height/2);await page.waitForTimeout(220);assert.equal(await page.getByRole('tooltip').count(),1);
  await more.hover();await page.getByRole('tooltip').getByText('More message actions',{exact:true}).waitFor({timeout:250});
  // Long dynamic labels wrap and flip within the viewport at both extremes.
  await page.locator('#msg').focus();await page.mouse.move(0,0);await page.keyboard.press('Escape');
  const originalLabel=await copy.getAttribute('data-tooltip');
  for(const edge of ['top:0;right:0','bottom:0;left:0']){
    await copy.evaluate((el,edge)=>{el.style.cssText=`position:fixed;z-index:80;${edge}`;el.dataset.tooltip='Copy response with a long recognizable conversation label that wraps across multiple lines without clipping at a viewport edge';},edge);
    await copy.evaluate(el=>el.focus({preventScroll:true}));await page.getByRole('tooltip').waitFor({timeout:1000});const r=await page.getByRole('tooltip').boundingBox();assert.ok(r.x>=8 && r.y>=8 && r.x+r.width<=1272 && r.y+r.height<=712);assert.ok(r.width<=280 && r.height>30);assert.equal(await page.getByRole('tooltip').locator('button,a,input,[tabindex]').count(),0);assert.equal(await copy.getAttribute('aria-label'),'Copy response');await page.locator('#msg').focus();await page.mouse.move(640,0);await page.keyboard.press('Escape');
  }
  await copy.evaluate((el,label)=>{el.style.cssText='';el.dataset.tooltip=label;},originalLabel);
  // In-place removal cannot leave an orphaned tooltip.
  await page.locator('#msg').focus();await copy.focus();await page.getByRole('tooltip').waitFor();await copy.evaluate(el=>el.remove());await page.getByRole('tooltip').waitFor({state:'hidden'});
  // Controlled startup promises exercise UI ownership, independently of real native audio checks.
  await page.evaluate(async()=>{
    const stop=Magnus.stopSpeaking;const pending=[];Magnus.stopSpeaking=()=>new Promise(resolve=>pending.push(resolve));
    let firstText,secondText;
    const first=readAloud(new Promise(resolve=>firstText=resolve),'first');
    if(voice.entry!=='first')throw Error('Stop is unavailable during startup');
    const second=readAloud(new Promise(resolve=>secondText=resolve),'second');
    pending.shift()();firstText('Old reading.');await first;
    if(voice.entry!=='second')throw Error('Old startup cleared new reading');
    const stopped=hush();pending.shift()();secondText('New reading.');await second;
    pending.shift()();await stopped;
    if(voice.entry!==null)throw Error('Stop did not cancel queued startup');
    Magnus.stopSpeaking=stop;
  });
  // File previews remain bounded and their real bytes do not change with presentation.
  const fileText='```js\nconst original = 42;\n```\n\n| Row | Value |\n| --- | --- |\n'+Array.from({length:1000},(_,n)=>`| ${n} | original |`).join('\n');
  const previewFile=await page.evaluate(body=>Magnus.api('POST','/v1/files',body),{name:'bounded-preview.md',media_type:'text/markdown',data_b64:Buffer.from(fileText).toString('base64')});
  await page.evaluate(id=>openPanel(`file:${id}`),previewFile.id);
  const preview=page.frameLocator('#fview');await preview.getByText('The first 200 of 1000 rows; the rest are in the file.',{exact:true}).waitFor();assert.equal(await preview.locator('tbody tr').count(),200);
  assert.equal(await page.locator('#detail').getByRole('button',{name:'Download bounded-preview.md',exact:true}).count(),1,'the file-view download names the actual document');
  const frameCopy=page.locator('#detail .frame-actions button[data-code-copy]');await frameCopy.focus();await page.getByRole('tooltip').waitFor();await frameCopy.click();await page.getByRole('tooltip').waitFor({state:'hidden'});assert.equal(await page.evaluate(()=>navigator.clipboard.readText()),'const original = 42;');
  const bytes=await context.request.get(`${process.env.MAGNUS_URL}/v1/files/${previewFile.id}`);assert.equal(await bytes.text(),fileText);
  await page.evaluate(()=>closePanel());
  assert.deepEqual(errors,[]);console.log('browser: safe Markdown/copy, compact actions, feedback, menu keyboard/focus/outside click and tooltip timing/suppression/removal passed');
} finally {await browser.close();}
