import {env,open,pinAndSignIn} from './appdriver.mjs';
const [wd,app,url,fingerprint,provider,person]=['MAGNUS_WEBDRIVER','MAGNUS_APP','MAGNUS_URL','MAGNUS_FINGERPRINT','MAGNUS_PROVIDER','MAGNUS_PERSON'].map(env);
const d=await open(wd,app,process.env.MAGNUS_SHOTS,'continuity');await pinAndSignIn(d,{url,fingerprint,provider,person});
await d.step('start real work and see it complete',async()=>{
 await d.type(d.css('#msg'),'Create anchor work');await d.click(d.css('#send'));
 await d.xpath("//div[@id='thread']//li[@data-chat-task][@data-status='completed']");
});
await d.step('open the completed task after acting in Needs you',async()=>{
 await d.click(d.xpath("//div[@id='thread']//li[@data-chat-task][@data-status='completed']//a"));
 await d.until('the composed task result',()=>d.run("return document.querySelector('#detail iframe.composed')?.contentDocument?.body.textContent.includes('Actual stored anchor deliverable')"));
});
await d.step('an older completed-work link remains reachable after incoming content',async()=>{
 await d.run("window.__anchorWrites=null;(async()=>{for(let n=0;n<15;n++)await Magnus.api('POST','/v1/chat',{thread:App.thread,text:'Later conversation entry '+n+' '+('Readable content. '.repeat(15))});await refresh();window.__anchorWrites=true;})().catch(e=>window.__anchorWrites={error:e.message});");
 const wrote=await d.until('actual later messages',()=>d.run('return window.__anchorWrites'));if(wrote.error)throw Error(wrote.error);
 await d.xpath("//div[@id='thread']//div[@data-kind='message'][contains(.,'Later conversation entry 14')]");
 await d.click(d.xpath("//div[@id='thread']//li[@data-chat-task][@data-status='completed']//a"));
 await d.until('the actual composed deliverable',()=>d.run("return document.querySelector('#detail iframe.composed')?.contentDocument?.body.textContent.includes('Actual stored anchor deliverable')"));
});
await d.close();console.log('native completed-work navigation passed');
