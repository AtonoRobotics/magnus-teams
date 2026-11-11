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

// A full frame in small chunks, as a slow link brings it: the bytes are
// joined once, not once per chunk (a 4 MB frame was copied per 64 KB).
{
  let allocated = 0;
  class Counted extends Uint8Array { constructor(...a) { super(...a); if (typeof a[0] === "number") allocated += a[0]; } }
  const counted = { TextEncoder, Uint8Array: Counted, Uint8ClampedArray, DataView };
  counted.globalThis = counted;
  vm.runInNewContext(readFileSync(join(here, "../ui/rfb.js"), "utf8"), counted);
  const [w, h] = [1280, 800];
  let drawn = null;
  const big = new counted.Rfb({ send() {}, onRect: (x, y, rw, rh, px) => { drawn = [rw, rh, px[0], px.length]; }, onError: m => errors.push(m) });
  const handshake = bytes([...new TextEncoder().encode("RFB 003.008\n")], [1, 1], be32(0), be16(w), be16(h), new Array(16).fill(0), be32(0));
  big.feed(Counted.from(handshake));
  const frame = new Uint8Array(4 + 12 + w * h * 4).fill(7);
  frame.set(bytes(header(1), be16(0), be16(0), be16(w), be16(h), be32(0)));
  const CHUNK = 1024;
  allocated = 0;
  for (let at = 0; at < frame.length; at += CHUNK) big.feed(Counted.from(frame.subarray(at, at + CHUNK)));
  assert.deepEqual(errors, []);
  assert.deepEqual(drawn, [w, h, 7, w * h * 4], "the frame was not drawn");
  // The chunks themselves, one join of the frame, and its pixels.
  assert.ok(allocated < frame.length * 4, `feeding a frame allocated ${allocated} bytes for ${frame.length}`);
}

// A deflated screen: chunks of one zlib stream come out whole and in order.
{
  const { deflateSync } = await import("node:zlib");
  const inflating = { DecompressionStream, Uint8Array, DataView, TextEncoder, Uint8ClampedArray };
  inflating.globalThis = inflating;
  vm.runInNewContext(readFileSync(join(here, "../ui/rfb.js"), "utf8"), inflating);
  assert.ok(inflating.Rfb.canInflate());
  const plain = Uint8Array.from({ length: 200000 }, (_, i) => (i % 512 < 8 ? i % 251 : 42));
  const squeezed = deflateSync(plain);
  const out = [];
  let failure = null;
  const push = inflating.Rfb.inflater(b => out.push(...b), m => { failure = m; });
  for (let at = 0; at < squeezed.length; at += 333) push(squeezed.subarray(at, at + 333));
  for (let i = 0; i < 100 && out.length < plain.length; i++) await new Promise(r => setTimeout(r, 10));
  assert.equal(failure, null);
  assert.deepEqual(Uint8Array.from(out), plain, "the inflated screen is not the screen");
  const broken = inflating.Rfb.inflater(() => {}, m => { failure = m; });
  broken(Uint8Array.of(1, 2, 3, 4));
  for (let i = 0; i < 100 && !failure; i++) await new Promise(r => setTimeout(r, 10));
  assert.match(failure || "", /could not be read/, "a damaged screen stream was not reported");
}
console.log("rfb ok");
