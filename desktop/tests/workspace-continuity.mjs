import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {join,dirname} from 'node:path';
import {fileURLToPath} from 'node:url';
const root=join(dirname(fileURLToPath(import.meta.url)),'../..');
const {chromium}=createRequire(import.meta.url)(join(root,'browser/node_modules/playwright-core'));
const browser=await chromium.launch({executablePath:process.env.MAGNUS_CHROMIUM,args:['--no-sandbox','--disable-dev-shm-usage']});
const context=await browser.newContext();const page=await context.newPage();const errors=[];page.on('pageerror',e=>errors.push(e.message));
const base=process.env.MAGNUS_URL;
const signIn=async person=>{await page.getByLabel('Username').fill(person);await page.getByLabel('Password').fill(process.env.MAGNUS_PASSWORD);await page.getByRole('button',{name:'Sign in',exact:true}).click();await page.locator('#msg').waitFor();};
const until=async(fn,arg)=>{const end=Date.now()+30000;while(Date.now()<end){if(await page.evaluate(fn,arg))return;await page.waitForTimeout(40);}throw Error('UI condition timed out');};
// Unsent messages are kept for the session (#544): text and attachments
// survive moving around the app, nothing is saved on the device, and a
// send that fails or loses its reply leaves the message where it was.
const back=async()=>{await page.getByRole('link',{name:'New conversation',exact:true}).click();await until(()=>location.hash==='#/chat/new' && App.draftRecord?.id==='new:' && document.querySelector('#msg'));};
const invalidModel=()=>page.locator('#say [data-model]').evaluate(select=>{const option=new Option('Removed model',JSON.stringify(['missing-adapter','removed-model']));select.append(option);select.value=option.value;});
try {
 await page.goto(base);await signIn('alice');await back();
 // The durable-draft machinery is gone from the composer and the navigation.
 for (const gone of ['Draft saved','Draft not saved on this device','Discard local draft']) assert.equal(await page.getByText(gone,{exact:true}).count(),0,gone);
 assert.equal(await page.locator('nav.side [aria-label=Drafts], nav.side a[href^="#/draft/"]').count(),0,'no Drafts entries in navigation');
 await page.locator('#msg').fill('First retained draft');
 let releaseUpload,uploaded;const uploadHeld=new Promise(resolve=>releaseUpload=resolve);
 await page.route('**/v1/files',async route=>{if(route.request().method()!=='POST')return route.continue();const response=await route.fetch();uploaded=await response.json();await uploadHeld;await route.fulfill({response});});
 await page.locator('#say input[data-attach]').setInputFiles({name:'notes.txt',mimeType:'text/plain',buffer:Buffer.from('shared file bytes')});
 await page.locator('.attachment-chip').getByText('Uploading…').waitFor();
 await page.getByRole('link',{name:'Settings',exact:true}).click();releaseUpload();
 await until(()=>MessageDrafts.forThread(null).files[0]?.state==='ready');
 await back();assert.equal(await page.locator('#msg').inputValue(),'First retained draft');await page.locator('.attachment-chip').getByText('Ready',{exact:true}).waitFor();
 await page.unroute('**/v1/files');
 assert.equal(await page.evaluate(()=>JSON.stringify(localStorage).includes('First retained draft')),false,'drafts are not saved on the device');
 await page.getByRole('button',{name:'Remove notes.txt from message'}).click();assert.equal(await page.locator('.attachment-chip').count(),0);
 assert.equal((await context.request.get(`${base}/v1/files/${uploaded.id}/info`)).status(),200,'removing attachment must not delete shared file');
 // Opening and Back/close preserve the selected draft and use deliberate focus.
 await page.getByRole('link',{name:'Files',exact:true}).click();await page.locator('#detail [data-panel=files]').waitFor();
 await page.locator(`#detail [data-open="file:${uploaded.id}"]`).click();await page.locator('#detail [data-panel=file]').waitFor();
 await until(()=>document.activeElement?.tagName==='H2' && !!document.activeElement.closest('#detail'));
 assert.equal(await page.locator('#msg').inputValue(),'First retained draft');
 await page.goBack();await page.locator('#detail [data-panel=files]').waitFor();
 await page.getByRole('button',{name:'Close work',exact:true}).click();
 assert.ok(await page.evaluate(()=>document.activeElement===document.querySelector('#conv-head h1')));
 // A removed pending upload may finish, but must never reattach itself.
 let releaseLate,lateFile;const lateHeld=new Promise(resolve=>releaseLate=resolve);
 await page.route('**/v1/files',async route=>{if(route.request().method()!=='POST')return route.continue();const response=await route.fetch();lateFile=await response.json();await lateHeld;await route.fulfill({response});});
 await page.locator('#say input[data-attach]').setInputFiles({name:'late.txt',mimeType:'text/plain',buffer:Buffer.from('late completion')});
 await page.locator('.attachment-chip').getByText('Uploading…').waitFor();await page.getByRole('button',{name:'Remove late.txt from message'}).click();const lateFinished=page.waitForEvent('requestfinished',{predicate:r=>r.method()==='POST' && new URL(r.url()).pathname==='/v1/files'});releaseLate();await lateFinished;
 await until(()=>MessageDrafts.all().every(r=>r.files.every(f=>f.name!=='late.txt')));await page.unroute('**/v1/files');

 // IME Enter leaves the message and sends nothing.
 const before=(await(await context.request.get(`${base}/v1/chat`)).json()).threads.length;
 await page.locator('#msg').dispatchEvent('keydown',{key:'Enter',code:'Enter',isComposing:true});
 assert.equal((await(await context.request.get(`${base}/v1/chat`)).json()).threads.length,before);
 assert.equal(await page.locator('#msg').inputValue(),'First retained draft');
 // Text typed while a send is in flight survives its acknowledgement.
 let releaseSend,accepted,posts=0;const sendHeld=new Promise(resolve=>releaseSend=resolve);
 await page.route('**/v1/chat',async route=>{if(route.request().method()!=='POST')return route.continue();posts++;const response=await route.fetch();accepted=await response.json();await sendHeld;await route.fulfill({response});});
 await page.locator('#send').click();await until(()=>document.querySelector('#send').disabled);
 await page.locator('#msg').fill('Newer text while sending');releaseSend();
 await page.waitForURL(/#\/chat\//);await until(()=>document.querySelector('#msg')?.value==='Newer text while sending');assert.equal(posts,1);
 await page.unroute('**/v1/chat');await page.locator('#msg').fill('');
 // A lost reply says the outcome is not confirmed, keeps the text and never resends.
 await page.locator('#msg').fill('Response will be lost');let lost=0;
 await page.route('**/v1/chat',async route=>{if(route.request().method()!=='POST')return route.continue();lost++;await route.fetch();await route.abort('failed');});
 await page.locator('#send').click();await page.getByText(/^Send outcome not confirmed/).waitFor();
 assert.equal(await page.locator('#msg').inputValue(),'Response will be lost');await page.waitForTimeout(1000);assert.equal(lost,1,'no automatic resend');
 await page.unroute('**/v1/chat');
 const delivered=(await(await context.request.get(`${base}/v1/chat/${accepted.thread}`)).json()).entries.filter(e=>e.kind==='message' && e.text==='Response will be lost');assert.equal(delivered.length,1);
 // A rejected send keeps the message to fix and send again.
 const validModel=await page.locator('#say [data-model]').inputValue();
 await page.locator('#msg').fill('Rejected message stays');await invalidModel();await page.locator('#send').click();
 await until(()=>!document.querySelector('#send').disabled);assert.equal(await page.locator('#msg').inputValue(),'Rejected message stays');
 await page.locator('#say [data-model]').selectOption(validModel);await page.locator('#msg').fill('');
 // A send in flight stays in flight after leaving and coming back, and its
 // acknowledgement clears the composer now showing (#553 review).
 await page.locator('#msg').fill('Held reply');let held=0,releaseHeld;const replyHeld=new Promise(resolve=>releaseHeld=resolve);
 await page.route('**/v1/chat',async route=>{if(route.request().method()!=='POST')return route.continue();held++;const response=await route.fetch();await replyHeld;await route.fulfill({response});});
 await page.locator('#send').click();await until(()=>document.querySelector('#send').disabled);
 await page.getByRole('link',{name:'Settings',exact:true}).click();await until(()=>!document.querySelector('#msg'));
 await page.locator(`nav.side a[href="#/chat/${accepted.thread}"]`).click();await until(()=>document.querySelector('#msg')?.value==='Held reply');
 assert.equal(await page.locator('#send').isDisabled(),true,'Send stays disabled while the earlier send is in flight');
 await page.locator('#msg').press('Enter');await page.waitForTimeout(300);assert.equal(held,1,'no second POST');
 releaseHeld();await until(()=>document.querySelector('#msg')?.value==='' && !document.querySelector('#send').disabled);
 await page.getByRole('link',{name:'Settings',exact:true}).click();await page.locator(`nav.side a[href="#/chat/${accepted.thread}"]`).click();await page.locator('#msg').waitFor();
 assert.equal(await page.locator('#msg').inputValue(),'','the sent reply is not resurrected');await page.unroute('**/v1/chat');
 // Controlled observer delivery reproduces a deliberate scroll during resize restoration.
 await page.evaluate(()=>{
   const NativeObserver=window.ResizeObserver;let resize;
   window.ResizeObserver=class{constructor(fn){resize=fn;}observe(){}disconnect(){}};
   const root=document.createElement('div');root.style.cssText='position:fixed;left:0;top:0;width:300px;height:100px;overflow:auto';root.innerHTML=Array.from({length:30},(_,n)=>`<p data-key="race-${n}" style="height:40px">Readable row ${n}</p>`).join('');document.body.append(root);
   let unwatch;
   try{unwatch=ReadingAnchors.watch(root,{anchor:{follow:true}},false);resize();root.scrollTop=150;root.dispatchEvent(new Event('scroll'));resize();if(Math.abs(root.scrollTop-150)>1)throw Error('Resize restoration swallowed deliberate scrolling');}
   finally{unwatch?.();root.remove();window.ResizeObserver=NativeObserver;}
 });
 // Scroll anchoring uses an entry identity; new data never forces latest.
 for(let i=0;i<20;i++){const response=await page.evaluate(body=>Magnus.api('POST','/v1/chat',body),{thread:accepted.thread,text:`Anchor entry ${i}`});assert.equal(response.thread,accepted.thread);}
 await page.goto(`${base}/#/chat/${accepted.thread}`);const expectedMessages=await page.evaluate(async thread=>(await Magnus.api('GET',`/v1/chat/${thread}`)).entries.filter(e=>e.kind==='message').length,accepted.thread);await page.locator('.entry[data-kind=message]').nth(expectedMessages-1).waitFor();
 await page.locator('#thread').evaluate(el=>{el.scrollTop=120;});await page.waitForTimeout(350);
 const anchor=()=>page.locator('#thread').evaluate(el=>{const top=el.getBoundingClientRect().top;const n=[...el.querySelectorAll('[data-entry]')].find(n=>n.getBoundingClientRect().bottom>top);return{id:n.dataset.entry,offset:n.getBoundingClientRect().top-top,bottom:el.scrollHeight-el.scrollTop-el.clientHeight};});
 const previous=await anchor();assert.ok(previous.bottom>80);
 const response=await page.evaluate(body=>Magnus.api('POST','/v1/chat',body),{thread:accepted.thread,text:'Activity while reading above'});assert.equal(response.thread,accepted.thread);
 await page.locator('.entry[data-kind=message]').nth(expectedMessages).waitFor();const after=await anchor();assert.equal(after.id,previous.id);assert.ok(Math.abs(after.offset-previous.offset)<3);
 // A real uploaded image gains intrinsic size above the reader after hydration.
 const image=await page.evaluate(body=>Magnus.api('POST','/v1/files',body),{name:'anchor-pixel.png',media_type:'image/png',data_b64:'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jRZkAAAAASUVORK5CYII='});
 await page.evaluate(async id=>{const first=document.querySelector('#thread [data-entry]'),img=document.createElement('img');img.alt='Delayed intrinsic-size anchor fixture';img.style.width='160px';img.dataset.proseImage=`/v1/files/${id}`;first.append(img);loadMarkdownImages(first);await new Promise((resolve,reject)=>{img.onload=resolve;img.onerror=reject;});await new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)));},image.id);
 const afterImage=await anchor();assert.equal(afterImage.id,previous.id);assert.ok(Math.abs(afterImage.offset-previous.offset)<3,'late intrinsic image size preserves the reader anchor');
 await page.evaluate(()=>{const stamp=document.querySelector('#thread .entry .small');if(stamp)stamp.textContent='Magnus · refreshed timestamp';});await page.evaluate(()=>new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve))));const afterStamp=await anchor();assert.equal(afterStamp.id,previous.id);assert.ok(Math.abs(afterStamp.offset-previous.offset)<3,'timestamp-only changes do not follow latest');
 await page.getByRole('button',{name:'New activity · Jump to latest'}).click();assert.ok((await anchor()).bottom<=80);
 await page.evaluate(body=>Magnus.api('POST','/v1/chat',body),{thread:accepted.thread,text:'Follow new activity while already at bottom'});await page.locator('.entry[data-kind=message]').nth(expectedMessages+1).waitFor();await page.evaluate(()=>new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve))));assert.ok((await anchor()).bottom<=80,'readers at bottom keep following actual new content');

 // Sign-out forgets Alice's unsent messages; Bob sees none of them.
 await page.locator('#msg').fill('Alice was typing this');
 await page.getByRole('button',{name:'Sign out',exact:true}).click();await signIn('bob');
 assert.equal(await page.evaluate(()=>MessageDrafts.all().some(r=>r.text.includes('Alice'))),false);
 assert.deepEqual(errors,[]);console.log('browser: session drafts across navigation, uploads, nondeleting removal, focus/history, IME, newer text after send, lost reply without resend, rejection, anchors and sign-out passed');
} finally {await browser.close();}
