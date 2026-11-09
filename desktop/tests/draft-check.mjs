// Unsent messages live in memory for the session (#544): one per
// conversation and one per project's new conversation, nothing written to
// the device, a different person starting empty, sign-out forgetting all.
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {randomUUID} from 'node:crypto';
import vm from 'node:vm';
const source=readFileSync(new URL('../ui/app.js',import.meta.url),'utf8');
const writes=[];
const context={crypto:{randomUUID},setTimeout,clearTimeout,console,
  localStorage:{getItem:()=>null,setItem:(k,v)=>writes.push([k,v]),removeItem:k=>writes.push([k])},
  document:{addEventListener(){}},window:{addEventListener(){}},location:{hash:''}};
vm.runInNewContext(source+';globalThis.store=MessageDrafts;',context);const S=context.store;
for (const gone of ['create','submit','acknowledge','reject','flush','saved','leave']) assert.equal(S[gone],undefined,`${gone} belongs to the removed draft journal`);
S.open('appliance-a','alice');
const t=S.forThread('thread-1');t.text='Reply in progress';t.files=[{uid:randomUUID(),id:'file-1',name:'notes.txt',state:'ready'}];
assert.equal(S.forThread('thread-1'),t,'a conversation keeps one draft');
const fresh=S.forThread(null),inProject=S.forThread(null,'project-1');
assert.notEqual(fresh,inProject,'each project has its own new-conversation draft');
fresh.text='New conversation text';assert.equal(S.forThread(null).text,'New conversation text');
let told=0;const off=S.subscribe(()=>told++);S.changed();assert.equal(told,1);off();
S.discard(fresh.id);assert.equal(S.forThread(null).text,'','a discarded draft starts over');
S.open('appliance-a','alice');assert.equal(S.forThread('thread-1').text,'Reply in progress','the same person keeps their drafts after signing in again');
S.open('appliance-a','bob');assert.equal(S.all().length,0,'another person starts with nothing');
S.forThread('thread-2').text='Bob typed';S.clear();assert.equal(S.all().length,0,'sign-out forgets everything');
assert.deepEqual(writes.filter(([k])=>/draft/i.test(k)),[],'drafts are never written to the device');
console.log('drafts: one per conversation, per-project new conversation, person switch, sign-out, nothing on the device passed');
