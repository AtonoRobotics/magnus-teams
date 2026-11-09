export function tableMarkdown(first,last) {
 const headers=Array.from({length:20},(_,n)=>`Column ${n+1}`);
 const row=headers.map((_,n)=>n===0?`[First supplier](${first})`:n===19?`[Last supplier](${last})`:`Value ${n+1}`);
 return `# Supplier links\n\n| ${headers.join(' | ')} |\n| ${headers.map(()=>'---').join(' | ')} |\n| ${row.join(' | ')} |`;
}
// Read real layout/hit targets; this does not click or replace any operation.
export function inspectLinks(frame) {
 const doc=frame?.contentDocument,table=doc?.querySelector('.markdown-table');if(!table)return null;
 const f=frame.getBoundingClientRect(),t=table.getBoundingClientRect(),controls=[...frame.parentElement.querySelectorAll('.frame-prose-link')];
 return {scroll:table.scrollLeft,max:table.scrollWidth-table.clientWidth,panelOverflow:document.querySelector('#detail').scrollWidth-document.querySelector('#detail').clientWidth,pageOverflow:document.documentElement.scrollWidth-innerWidth,
 links:[...doc.querySelectorAll('[data-prose-url]')].map((marker,n)=>{
  const m=marker.getBoundingClientRect(),c=controls[n]?.getBoundingClientRect();if(!c)return {missing:true};
  const left=Math.max(0,t.left+table.clientLeft,m.left),right=Math.min(frame.clientWidth,t.left+table.clientLeft+table.clientWidth,m.right),top=Math.max(0,t.top+table.clientTop,m.top),bottom=Math.min(frame.clientHeight,t.top+table.clientTop+table.clientHeight,m.bottom);
  const visible=right>left && bottom>top,x=f.left+(left+right)/2,y=f.top+(top+bottom)/2;
  return {href:marker.dataset.proseUrl,visible,fullyVisible:visible && m.left>=Math.max(0,t.left+table.clientLeft)-1 && m.right<=Math.min(frame.clientWidth,t.left+table.clientLeft+table.clientWidth)+1 && m.top>=Math.max(0,t.top+table.clientTop)-1 && m.bottom<=Math.min(frame.clientHeight,t.top+table.clientTop+table.clientHeight)+1,aligned:Math.abs(c.left-f.left-m.left)<2 && Math.abs(c.top-f.top-m.top)<2,hit:visible?document.elementFromPoint(x,y)?.closest('a')?.href:null};
 })};
}
