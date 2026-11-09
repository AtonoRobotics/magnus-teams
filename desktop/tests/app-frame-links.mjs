import {env,open,pinAndSignIn} from './appdriver.mjs';
import {tableMarkdown,inspectLinks} from './frame-links-fixture.mjs';
const [wd,app,url,fingerprint,provider,person,first,last]=['MAGNUS_WEBDRIVER','MAGNUS_APP','MAGNUS_URL','MAGNUS_FINGERPRINT','MAGNUS_PROVIDER','MAGNUS_PERSON','MAGNUS_FIRST_LINK','MAGNUS_LAST_LINK'].map(env);
const d=await open(wd,app,process.env.MAGNUS_SHOTS,'frame-links');await pinAndSignIn(d,{url,fingerprint,provider,person});
const read=()=>d.run(`return (${inspectLinks.toString()})(document.querySelector('#fview'))`);
const check=async(which)=>d.until('visible aligned link hit target',async()=>{const s=await read();const link=s?.links[which];return link?.visible && link.aligned && link.hit===[first,last][which] && s.panelOverflow<=2 && s.pageOverflow<=0 ? s:null;},10000).catch(async error=>{throw Error(`${error.message}; actual layout ${JSON.stringify(await read())}`);});
await d.step('an actual uploaded overflowing table',async()=>{
 await d.run("window.__linkUpload=null;Magnus.api('POST','/v1/files',arguments[0]).then(r=>window.__linkUpload=r,e=>window.__linkUpload={error:e.message});",{name:'scroll-links.md',media_type:'text/markdown',data_b64:Buffer.from(tableMarkdown(first,last)).toString('base64')});
 const file=await d.until('file uploaded',()=>d.run('return window.__linkUpload'));if(file.error)throw Error(file.error);
 await d.click(d.xpath("//nav//a[normalize-space()='Files']"));await d.click(d.xpath(`//a[@data-open='file:${file.id}']`));
 const s=await check(0);if(s.max<500)throw Error('table does not actually overflow');
});
await d.step('scrolling preserves the visible link and its real external handoff',async()=>{
 await d.run("const t=document.querySelector('#fview').contentDocument.querySelector('.markdown-table');t.scrollLeft=t.scrollWidth;");await check(1);
 await d.click(d.css('.frame-actions a:last-child'));
});
await d.step('focusing an offscreen link reveals its own cell',async()=>{
 await d.run("document.querySelector('.frame-actions a:first-child').focus()");const s=await check(0);if(!s.links[0].fullyVisible)throw Error('focus did not reveal the complete first link');
 if(!await d.run("return document.activeElement===document.querySelector('.frame-actions a:first-child')"))throw Error('focus was not kept on the link');
 await d.click(d.css('.frame-actions a:first-child'));
 await d.call('POST',d.s('/actions'),{actions:[{type:'key',id:'keyboard',actions:[{type:'keyDown',value:'\uE004'},{type:'keyUp',value:'\uE004'}]}]});
 const tabbed=await check(1);if(!tabbed.links[1].fullyVisible || !await d.run("return document.activeElement===document.querySelector('.frame-actions a:last-child')"))throw Error('Tab did not reveal and focus the last link');
 if(await d.attr(d.css('#fview'),'sandbox')!=='allow-same-origin')throw Error('frame sandbox changed');
});
await d.close();console.log('native scrolling, clipped links, focus reveal and external browser handoff passed');
