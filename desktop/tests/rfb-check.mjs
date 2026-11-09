// The takeover client against a scripted server, split at awkward places.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const here = dirname(fileURLToPath(import.meta.url));
const sandbox = { TextEncoder, Uint8Array, Uint8ClampedArray, DataView };
sandbox.globalThis = sandbox;
vm.runInNewContext(readFileSync(join(here, "../ui/rfb.js"), "utf8"), sandbox);

const sent = [], rects = [], copies = [], errors = [];
let size, ready = false;
const client = new sandbox.Rfb({
  send: b => sent.push([...b]),
  onSize: (w, h) => { size = [w, h]; },
  onRect: (x, y, w, h, px) => rects.push([x, y, w, h, [...px]]),
  onCopy: (...a) => copies.push(a),
  onReady: () => { ready = true; },
  onError: m => errors.push(m),
});
const be16 = n => [n >> 8, n & 255], be32 = n => [0, 0, n >> 8, n & 255];
const bytes = (...p) => Uint8Array.from(p.flat(Infinity));
const feedSlowly = b => { for (const x of b) client.feed(Uint8Array.of(x)); };

feedSlowly(new TextEncoder().encode("RFB 003.008\n"));
assert.equal(String.fromCharCode(...sent.shift()), "RFB 003.008\n");
feedSlowly(bytes([2, 5, 1]));
assert.deepEqual(sent.shift(), [1], "it must choose no authentication");
feedSlowly(bytes(be32(0)));
assert.deepEqual(sent.shift(), [1], "it must ask to share the screen");
const name = [...new TextEncoder().encode("vm")];
feedSlowly(bytes(be16(4), be16(2), new Array(16).fill(0), be32(name.length), name));
assert.deepEqual(size, [4, 2]);
assert.ok(ready);
const format = sent.shift();
assert.equal(format[0], 0);
assert.equal(format[4], 32, "32 bits a pixel");
assert.equal(sent.shift()[0], 2, "encodings");
assert.deepEqual(sent.shift(), [3, 0, 0, 0, 0, 0, 0, 4, 0, 2], "a full first frame");

const raw = (x, y, w, h, px) => [be16(x), be16(y), be16(w), be16(h), be32(0), px];
const header = n => [0, 0, be16(n)];
feedSlowly(bytes(header(2), raw(1, 0, 1, 1, [9, 8, 7, 0]), [be16(0), be16(1), be16(2), be16(1), be32(1), be16(1), be16(0)]));
assert.deepEqual(rects, [[1, 0, 1, 1, [9, 8, 7, 255]]], "pixels are opaque");
assert.deepEqual(copies, [[0, 1, 2, 1, 1, 0]]);
assert.deepEqual(sent.shift(), [3, 1, 0, 0, 0, 0, 0, 4, 0, 2], "then only what changes");

client.feed(bytes([2], [3, 0, 0, 0], be32(2), [65, 66]));
assert.deepEqual(errors, [], "a bell and a clipboard are no error");

assert.ok(client.key("a", true));
assert.deepEqual(sent.shift(), [4, 1, 0, 0, 0, 0, 0, 97]);
assert.ok(client.key("Enter", false));
assert.deepEqual(sent.shift(), [4, 0, 0, 0, 0, 0, 0xff, 0x0d]);
assert.ok(client.key("€", true));
assert.deepEqual(sent.shift(), [4, 1, 0, 0, 1, 0, 0x20, 0xac]);
assert.equal(client.key("Dead", true), false, "a key with no meaning is not sent");
client.pointer(300, 2, 1);
assert.deepEqual(sent.shift(), [5, 1, 1, 44, 0, 2]);

const refused = new sandbox.Rfb({ send() {}, onError: m => errors.push(m) });
refused.feed(new TextEncoder().encode("RFB 003.008\n"));
refused.feed(bytes([1, 2]));
assert.match(errors.pop(), /password/);
console.log("rfb ok");
