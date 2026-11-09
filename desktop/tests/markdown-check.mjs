import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import vm from 'node:vm';
const ctx={URL, location:{origin:'https://magnus.example'}};
vm.runInNewContext(readFileSync(new URL('../ui/compose.js',import.meta.url),'utf8')+';globalThis.renderer=Compose;',ctx);
const {markdown,safeUrl,documentFor}=ctx.renderer;
assert.equal(safeUrl('javascript:alert(1)'),null);assert.equal(safeUrl('data:text/html,evil'),null);assert.equal(safeUrl('java\nscript:evil'),null);
const source='# Heading\n\n**bold** and *emphasis* with `inline`. [safe](https://example.com) [unsafe](javascript:alert(1))\n\n- first\n- second\n\n> quote\n\n| A | B |\n| --- | --- |\n| 1 | 2 |\n\n```js\nconst x = "<script>";\n```\n\n<script>globalThis.pwned=true</script>\n<img src=x onerror=alert(1)>';
const html=markdown(source);
for(const part of ['<h1>Heading</h1>','<strong>bold</strong>','<em>emphasis</em>','<code>inline</code>','<ul>','<blockquote>','<table>','data-code-copy','&lt;script&gt;'])assert.ok(html.includes(part),part);
assert.ok(!html.includes('<script>'));assert.ok(!html.includes('href="javascript:'));assert.ok(!html.includes('<img src=x'));
assert.ok(markdown('```\n<img onerror=evil>\n').includes('&lt;img onerror=evil&gt;'));
const frame=documentFor([{type:'file',file:'result.md'}],{'result.md':{text:source,media_type:'text/markdown'}},'',{chars:160000,rows:200});
assert.ok(frame.includes('data-code-copy'));assert.ok(frame.includes("default-src 'none'"));assert.ok(!frame.includes('<script>'));
console.log('shared Markdown: required structures, raw HTML/URL rejection, incomplete fences and sandboxed file previews passed');

for(const origin of ['https://magnus.example','tauri://localhost','null']) {
 const native={URL,location:{origin}};vm.runInNewContext(readFileSync(new URL('../ui/compose.js',import.meta.url),'utf8')+';globalThis.renderer=Compose;',native);
 assert.equal(native.renderer.safeUrl('https://example.com/reference'),'https://example.com/reference',origin);
 assert.ok(native.renderer.markdown('[Supplier reference](https://example.com/reference)').includes('data-prose-link'),origin);
 for(const scheme of ['javascript:evil','data:text/html,evil','file:///etc/passwd'])assert.equal(native.renderer.safeUrl(scheme),null,origin);
 const rendered=native.renderer.markdown('![Revenue chart](/v1/files/file_chart)');assert.ok(rendered.includes('data-prose-image="/v1/files/file_chart"'),origin);assert.ok(rendered.includes('alt="Revenue chart"'));
 for(const url of ['/v1/files/../../outside','//evil.example/v1/files/chart','javascript:evil','data:image/svg+xml,evil'])assert.ok(!native.renderer.markdown(`![unsafe](${url})`).includes('data-prose-image'),`${origin}/${url}`);
}
console.log('local image hydration paths work for HTTP, Tauri and opaque origins without widening URL/sandbox permissions');

const largeTable='| Row | Value |\n| --- | --- |\n'+Array.from({length:1000},(_,n)=>`| ${n} | safe |`).join('\n');
const limited=documentFor([{type:'file',file:'large.md'}],{'large.md':{text:largeTable,media_type:'text/markdown'}},'',{chars:160000,rows:200});
assert.equal((limited.match(/<tr>/g)||[]).length,201,'preview must keep its configured row bound');
assert.ok(limited.includes('The first 200 of 1000 rows; the rest are in the file.'));
assert.equal((markdown(largeTable).match(/<tr>/g)||[]).length,1001,'ordinary answers do not acquire a file preview cap');
console.log('Markdown file table previews preserve the row bound and remainder notice');
