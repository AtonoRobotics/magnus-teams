// A small RFB (VNC) client for takeover: raw and copy-rect frames, keys and
// the pointer. It holds no sockets: bytes come in through feed() and go out
// through send(), so it can be driven by a browser or the app's relay.
(function (root) {
  const SPECIAL = { Backspace: 0xff08, Tab: 0xff09, Enter: 0xff0d, Escape: 0xff1b, Delete: 0xffff, Home: 0xff50, ArrowLeft: 0xff51, ArrowUp: 0xff52, ArrowRight: 0xff53, ArrowDown: 0xff54, PageUp: 0xff55, PageDown: 0xff56, End: 0xff57, Insert: 0xff63, Shift: 0xffe1, Control: 0xffe3, Meta: 0xffe7, Alt: 0xffe9, F1: 0xffbe, F2: 0xffbf, F3: 0xffc0, F4: 0xffc1, F5: 0xffc2, F6: 0xffc3, F7: 0xffc4, F8: 0xffc5, F9: 0xffc6, F10: 0xffc7, F11: 0xffc8, F12: 0xffc9 };
  const SECURITY_NONE = 1, ENCODING_RAW = 0, ENCODING_COPY = 1, MSG_UPDATE = 0, MSG_COLORS = 1, MSG_BELL = 2, MSG_CUT = 3, RECT_HEADER_BYTES = 12, BYTES_PER_PIXEL = 4;

  function keysym(key) {
    if (SPECIAL[key]) return SPECIAL[key];
    const chars = [...key];
    if (chars.length !== 1) return null;
    const code = chars[0].codePointAt(0);
    return code < 256 ? code : 0x01000000 | code;
  }

  class Rfb {
    // send(Uint8Array); onSize(w, h); onRect(x, y, w, h, rgba); onCopy(x, y, w, h, fromX, fromY);
    // onReady(); onError(message)
    constructor(h) { this.h = h; this.chunks = []; this.length = 0; this.step = this.version; this.width = 0; this.height = 0; }

    // Bytes are kept as they came and joined only when a message spans
    // them: joining on every feed copied a 4 MB frame once per 64 KB chunk.
    feed(bytes) {
      if (bytes.length) { this.chunks.push(bytes); this.length += bytes.length; }
      try { while (this.step && this.step()); } catch (e) { this.step = null; this.h.onError?.(e.message); }
    }
    have(n) { return this.length >= n; }
    /** The first n bytes, which must be there, as one array; nothing is taken. */
    peek(n) {
      if (n === 0) return new Uint8Array(0);
      if (this.chunks[0].length < n) {
        const joined = new Uint8Array(n);
        let at = 0;
        while (at < n) {
          const c = this.chunks.shift(), use = Math.min(c.length, n - at);
          joined.set(c.subarray(0, use), at); at += use;
          if (use < c.length) this.chunks.unshift(c.subarray(use));
        }
        this.chunks.unshift(joined);
      }
      return this.chunks[0].subarray(0, n);
    }
    take(n) {
      const out = this.peek(n);
      if (n === 0) return out;
      this.chunks[0] = this.chunks[0].subarray(n);
      if (!this.chunks[0].length) this.chunks.shift();
      this.length -= n;
      return out;
    }
    view(bytes) { return new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength); }

    version() {
      if (!this.have(12)) return false;
      this.take(12);
      this.h.send(new TextEncoder().encode("RFB 003.008\n"));
      this.step = this.security;
      return true;
    }
    security() {
      if (!this.have(1)) return false;
      const n = this.peek(1)[0];
      if (!this.have(1 + n)) return false;
      if (n === 0) throw new Error("the display refused the connection");
      const types = this.take(1 + n).subarray(1);
      if (!types.includes(SECURITY_NONE)) throw new Error("the display asks for a password");
      this.h.send(Uint8Array.of(SECURITY_NONE));
      this.step = this.securityResult;
      return true;
    }
    securityResult() {
      if (!this.have(4)) return false;
      if (this.view(this.take(4)).getUint32(0) !== 0) throw new Error("the display refused the connection");
      this.h.send(Uint8Array.of(1)); // Shared: the agent's own capture goes on.
      this.step = this.init;
      return true;
    }
    init() {
      if (!this.have(24)) return false;
      const nameLength = this.view(this.peek(24).subarray(20, 24)).getUint32(0);
      if (!this.have(24 + nameLength)) return false;
      const head = this.view(this.take(24 + nameLength));
      this.width = head.getUint16(0); this.height = head.getUint16(2);
      this.h.onSize?.(this.width, this.height);
      // 32-bit little-endian true colour: red, green, blue in byte order.
      this.h.send(Uint8Array.of(0, 0, 0, 0, 32, 24, 0, 1, 0, 255, 0, 255, 0, 255, 0, 8, 16, 0, 0, 0));
      const encodings = new Uint8Array(12), e = this.view(encodings);
      e.setUint8(0, 2); e.setUint16(2, 2); e.setInt32(4, ENCODING_COPY); e.setInt32(8, ENCODING_RAW);
      this.h.send(encodings);
      this.request(false);
      this.h.onReady?.();
      this.step = this.message;
      return true;
    }
    request(incremental) {
      const m = new Uint8Array(10), v = this.view(m);
      v.setUint8(0, 3); v.setUint8(1, incremental ? 1 : 0); v.setUint16(6, this.width); v.setUint16(8, this.height);
      this.h.send(m);
    }
    message() {
      if (!this.have(1)) return false;
      switch (this.peek(1)[0]) {
        case MSG_UPDATE:
          if (!this.have(4)) return false;
          this.rects = this.view(this.take(4)).getUint16(2);
          this.step = this.rect;
          return true;
        case MSG_BELL: this.take(1); return true;
        case MSG_CUT: {
          if (!this.have(8)) return false;
          const length = this.view(this.peek(8).subarray(4, 8)).getUint32(0);
          if (!this.have(8 + length)) return false;
          this.take(8 + length);
          return true;
        }
        case MSG_COLORS: throw new Error("the display sent a colour map");
        default: throw new Error(`the display sent message ${this.peek(1)[0]}`);
      }
    }
    rect() {
      if (this.rects === 0) { this.request(true); this.step = this.message; return true; }
      if (!this.have(RECT_HEADER_BYTES)) return false;
      const v = this.view(this.peek(RECT_HEADER_BYTES));
      const [x, y, w, h, encoding] = [v.getUint16(0), v.getUint16(2), v.getUint16(4), v.getUint16(6), v.getInt32(8)];
      if (encoding === ENCODING_COPY) {
        if (!this.have(RECT_HEADER_BYTES + 4)) return false;
        this.take(RECT_HEADER_BYTES);
        const from = this.view(this.take(4));
        this.h.onCopy?.(x, y, w, h, from.getUint16(0), from.getUint16(2));
      } else if (encoding === ENCODING_RAW) {
        const size = w * h * BYTES_PER_PIXEL;
        if (!this.have(RECT_HEADER_BYTES + size)) return false;
        this.take(RECT_HEADER_BYTES);
        const pixels = new Uint8ClampedArray(this.take(size));
        for (let i = 3; i < size; i += BYTES_PER_PIXEL) pixels[i] = 255;
        this.h.onRect?.(x, y, w, h, pixels);
      } else throw new Error(`the display used encoding ${encoding}`);
      this.rects--;
      return true;
    }

    key(name, down) {
      const sym = keysym(name);
      if (sym === null) return false;
      const m = new Uint8Array(8), v = this.view(m);
      v.setUint8(0, 4); v.setUint8(1, down ? 1 : 0); v.setUint32(4, sym);
      this.h.send(m);
      return true;
    }
    pointer(x, y, buttons) {
      const m = new Uint8Array(6), v = this.view(m);
      v.setUint8(0, 5); v.setUint8(1, buttons); v.setUint16(2, x); v.setUint16(4, y);
      this.h.send(m);
    }
  }

  /** The subprotocol under which the server sends the screen deflated. */
  Rfb.DEFLATE = "magnus.display.deflate";
  /** Whether this browser can read a deflated screen. */
  Rfb.canInflate = () => typeof root.DecompressionStream === "function";
  /**
   * For a screen sent deflated: one zlib stream over the whole connection,
   * flushed at every chunk. Returns push(bytes); each push's bytes reach
   * onBytes in order, as soon as they are inflated.
   * @param {(bytes: Uint8Array) => void} onBytes
   * @param {(message: string) => void} onError
   */
  Rfb.inflater = (onBytes, onError) => {
    const stream = new root.DecompressionStream("deflate"), writer = stream.writable.getWriter(), reader = stream.readable.getReader();
    (async () => { for (;;) { const { value, done } = await reader.read(); if (done) return; onBytes(value); } })().catch(e => onError(`the screen could not be read: ${e.message}`));
    return bytes => { writer.write(bytes).catch(() => { /* The reader says why. */ }); };
  };
  root.Rfb = Rfb;
})(typeof window === "undefined" ? globalThis : window);
