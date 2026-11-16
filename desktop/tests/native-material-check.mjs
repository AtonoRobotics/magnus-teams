// Ordering of actual Theme.apply bridge calls; this is a controlled host-response unit fixture.
import assert from 'node:assert/strict';import {readFileSync} from 'node:fs';import vm from 'node:vm';
const calls=[],waiting=[],store=new Map();
const host={localStorage:{getItem:k=>store.get(k)??null,setItem:(k,v)=>store.set(k,v)},document:{documentElement:{style:{setProperty(){}},dataset:{}},querySelectorAll:()=>[]},Magnus:{setMaterial:(...args)=>{calls.push(args);return new Promise((resolve,reject)=>waiting.push({resolve,reject}));}},addEventListener(){},dispatchEvent(){},CustomEvent:class{constructor(type,options){this.type=type;this.detail=options.detail;}}};host.window=host;
vm.runInNewContext(readFileSync(new URL('../ui/theme.js',import.meta.url),'utf8'),host);const T=host.Theme,settle=async()=>{for(let i=0;i<5;i++)await Promise.resolve();};
assert.equal(calls.length,1);T.scope('appliance','alice');T.set({mode:'light'});T.set({accent:'amber'});T.set({conversationWidthPx:480,navigation:'compact'});
assert.equal(T.look.mode,'light','tokens apply immediately while a host command is pending');assert.equal(calls.length,1,'native operations must not overlap');
waiting.shift().resolve({native:false});await settle();assert.equal(calls.length,2,'coalesce pending choices to the latest revision');assert.deepEqual(Array.from(calls[1][2]),[248,248,246,219]);assert.equal(calls[1][0],false);assert.equal(calls[1][1],false);
waiting.shift().resolve({native:false});await settle();assert.equal(host.document.documentElement.dataset.backdrop,'drawn');T.set({conversationWidthPx:500});assert.equal(calls.length,2,'layout preferences must not repeat expensive native material effects');
T.set({mode:'dark'});assert.equal(calls.length,3);waiting.shift().reject(Error('host unavailable'));await settle();assert.equal(host.document.documentElement.dataset.backdrop,'drawn');
T.set({mode:'light',material:'glass'});assert.equal(calls.length,4);waiting.shift().resolve({native:false});await settle();assert.equal(host.document.documentElement.dataset.backdrop,'drawn');assert.equal(T.look.mode,'light');assert.equal(T.prefs().material,'solid');
T.set({mode:'dark'});T.set({mode:'light'});T.set({mode:'dark'});assert.equal(calls.length,5);waiting.shift().resolve({native:false});await settle();assert.equal(calls.length,5,'returning to the in-flight effect must not queue a duplicate command');
console.log('native material: serialized commands, latest-choice coalescing, stale response suppression and failure recovery passed');
