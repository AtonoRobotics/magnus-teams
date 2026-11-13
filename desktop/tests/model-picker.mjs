// The composer's model picker follows the appliance default (#540). Run by
// tests/scratch/tests/model_picker.rs against a real appliance and browser.
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {join,dirname} from 'node:path';
import {fileURLToPath} from 'node:url';
// A step that never finishes fails the check by name, before the test's own
// limit (BROWSER_CHECK_LIMIT_SECS, 300) stops it with nothing to say.
const CHECK_LIMIT_MS=240000;let step='starting the browser';
const at=s=>{step=s;console.error(`step: ${s}`);};
setTimeout(()=>{console.error(`stuck at: ${step}`);process.exit(1);},CHECK_LIMIT_MS).unref();
const root=join(dirname(fileURLToPath(import.meta.url)),'../..');
const {chromium}=createRequire(import.meta.url)(join(root,'browser/node_modules/playwright-core'));
const browser=await chromium.launch({executablePath:process.env.MAGNUS_CHROMIUM,args:['--no-sandbox','--disable-dev-shm-usage']});
const page=await (await browser.newContext()).newPage();const errors=[];page.on('pageerror',e=>errors.push(e.message));

const value=(adapter,model)=>JSON.stringify([adapter,model]);
const firstDefault=value('anthropic','claude-test'),picked=value('oa','m-a'),nextDefault=value('oa','m-b');
const picker=page.locator('#say [data-model]');
const until=async(fn,arg)=>{const end=Date.now()+30000;while(Date.now()<end){if(await page.evaluate(fn,arg))return;await page.waitForTimeout(40);}throw Error('UI condition timed out');};
const reopen=async()=>{await page.reload();await page.locator('#msg').waitFor();};
// The default model is an admin change: the server holds it for the password
// again (step-up) unless one was confirmed in the last 5 minutes. Answer the
// sheet as the admin would, when it opens.
const setDefault=async m=>{
  const changed=page.evaluate(([adapter,model])=>api('PUT','/v1/model',{adapter,model}).then(()=>true),JSON.parse(m));
  const sheet=page.getByRole('dialog',{name:'Confirm your password'});
  const asked=await Promise.race([changed,sheet.waitFor().then(()=>false,()=>true)]);
  if(!asked){await sheet.getByLabel('Password').fill(process.env.MAGNUS_PASSWORD);await sheet.getByRole('button',{name:'Confirm'}).click();}
  await changed;
};
const send=async text=>{
  const answered=page.waitForResponse(r=>r.request().method()==='POST'&&r.url().endsWith('/v1/chat'));
  await page.locator('#msg').fill(text);await page.locator('#send').click();
  const response=await answered;assert.equal(response.status(),201);
  const {adapter,model}=response.request().postDataJSON().model;return value(adapter,model);
};
try {
  at('signing in');await page.goto(process.env.MAGNUS_URL);
  await page.getByLabel('Username').fill(process.env.MAGNUS_PERSON);await page.getByLabel('Password').fill(process.env.MAGNUS_PASSWORD);
  await page.getByRole('button',{name:'Sign in',exact:true}).click();await page.locator('#msg').waitFor();

  at('picking a model');assert.equal(await picker.inputValue(),firstDefault,'a new viewer starts on the default');
  await picker.selectOption(picked);
  assert.equal(await send('On the model I picked'),picked);
  await reopen();
  assert.equal(await picker.inputValue(),picked,'a pick holds while the default stays');

  at('changing the default');await setDefault(nextDefault);at('reopening under the new default');await reopen();
  assert.equal(await picker.inputValue(),nextDefault,'a changed default replaces the earlier pick');
  assert.equal(await send('On the new default'),nextDefault,'the message goes to the new default');

  // A draft keeps its pick under the same default and gives it up when that changes.
  // Drafts live for the session, so going elsewhere and back stands in for reopening.
  const fresh=async()=>{await page.getByRole('link',{name:'New conversation',exact:true}).click();await until(()=>location.hash==='#/chat/new' && App.draftRecord?.id==='new:' && document.querySelector('#msg'));};
  const away=async()=>{await page.getByRole('link',{name:'Settings',exact:true}).click();await until(()=>!document.querySelector('#msg'));await fresh();};
  at('starting a draft');await fresh();
  await picker.selectOption(picked);await page.locator('#msg').fill('A draft on the picked model');
  at('leaving the draft and coming back');await away();
  assert.equal(await page.locator('#msg').inputValue(),'A draft on the picked model');
  assert.equal(await picker.inputValue(),picked,'a draft keeps its pick while the default stays');
  at('changing the default under a draft');await setDefault(firstDefault);at('coming back to the draft');await away();
  assert.equal(await page.locator('#msg').inputValue(),'A draft on the picked model','the words stay');
  assert.equal(await picker.inputValue(),firstDefault,'a draft from under an older default uses the current one');

  // What an app saved before this fix names no default, so it does not hold.
  at('reading an older saved choice');await page.evaluate(()=>save('model-choice',{adapter:'oa',model:'m-a'}));await reopen();
  assert.equal(await picker.inputValue(),firstDefault,'a choice saved by an older app gives way to the default');

  assert.deepEqual(errors,[]);console.log('browser: picks hold under their default and give way when it changes, for messages and drafts');
} finally {await browser.close();}
