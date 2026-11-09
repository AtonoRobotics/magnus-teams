// Where the browser may connect (#185). Every request Chromium makes, the
// page it was told to open, each redirect and every image, script, fetch,
// frame and WebSocket a page starts, goes through the egress proxy below. It
// resolves the name once, checks every address, and connects only to an address
// it checked, so a name cannot pass as public and then be reached as private
// (DNS rebinding).
//
// Public internet addresses are allowed. Loopback, link-local (and with it
// the cloud metadata address), private networks, shared address space (CGNAT,
// Tailscale), multicast and reserved ranges are not, also when written as
// IPv6 (`::ffff:127.0.0.1`, NAT64, 6to4). An internal site opens only when
// the appliance's owner lists it in `[browser] allow`: a name or address,
// with or without `:port`, or a range (`10.1.0.0/16`).
'use strict';
const dns = require('dns');
const http = require('http');
const net = require('net');

// An address as its bytes (4 or 16), or null.
function bytes(ip) {
  ip = String(ip).replace(/^\[|\]$/g, '').replace(/%.*$/, '');
  if (net.isIPv4(ip)) return Uint8Array.from(ip.split('.').map(Number));
  if (!net.isIPv6(ip)) return null;
  const dotted = /(\d+\.\d+\.\d+\.\d+)$/.exec(ip);
  if (dotted) {
    const v4 = bytes(dotted[1]);
    ip = ip.slice(0, -dotted[1].length) + `${((v4[0] << 8) | v4[1]).toString(16)}:${((v4[2] << 8) | v4[3]).toString(16)}`;
  }
  const [head, tail] = ip.split('::');
  const h = head ? head.split(':') : [];
  const t = tail === undefined ? [] : tail ? tail.split(':') : [];
  const groups = tail === undefined ? h : [...h, ...Array(8 - h.length - t.length).fill('0'), ...t];
  const out = new Uint8Array(16);
  groups.forEach((g, i) => { const n = parseInt(g, 16); out[2 * i] = n >> 8; out[2 * i + 1] = n & 255; });
  return out;
}

function range(cidr) {
  const [ip, len] = cidr.split('/');
  const b = bytes(ip);
  const n = len === undefined ? (b ? b.length * 8 : NaN) : Number(len);
  if (!b || !Number.isInteger(n) || n < 0 || n > b.length * 8) throw new Error(`not an address or range: ${cidr}`);
  return [b, n];
}

function within(b, [p, n]) {
  if (b.length !== p.length) return false;
  for (let i = 0; i < n; i++) if (((b[i >> 3] ^ p[i >> 3]) >> (7 - (i & 7))) & 1) return false;
  return true;
}

// IANA's special-purpose registries, less the globally reachable entries.
const NOT_PUBLIC = [
  ['0.0.0.0/8', 'an unspecified'], ['10.0.0.0/8', 'a private network'], ['100.64.0.0/10', 'a shared (CGNAT or VPN) network'],
  ['127.0.0.0/8', 'a loopback'], ['169.254.0.0/16', 'a link-local (cloud metadata)'], ['172.16.0.0/12', 'a private network'],
  ['192.0.0.0/24', 'a reserved'], ['192.0.2.0/24', 'a documentation'], ['192.88.99.0/24', 'a reserved'],
  ['192.168.0.0/16', 'a private network'], ['198.18.0.0/15', 'a benchmarking'], ['198.51.100.0/24', 'a documentation'],
  ['203.0.113.0/24', 'a documentation'], ['224.0.0.0/4', 'a multicast'], ['240.0.0.0/4', 'a reserved'],
  ['::1/128', 'a loopback'], ['::/96', 'an unspecified or IPv4-compatible'], ['64:ff9b:1::/48', 'a local NAT64'],
  ['100::/64', 'a discard'], ['2001::/23', 'a reserved (IETF)'], ['2001:db8::/32', 'a documentation'],
  ['fc00::/7', 'a private network'], ['fe80::/10', 'a link-local'], ['fec0::/10', 'a site-local'], ['ff00::/8', 'a multicast'],
].map(([r, why]) => [range(r), why]);
// IPv6 forms that reach an IPv4 address: judged as that address.
const CARRIES_IPV4 = [[range('::ffff:0:0/96'), 12], [range('64:ff9b::/96'), 12], [range('2002::/16'), 2]];

function unwrap(b) {
  if (b.length === 16) for (const [r, at] of CARRIES_IPV4) if (within(b, r)) return b.slice(at, at + 4);
  return b;
}

// Why an address is not on the public internet, or null when it is.
function notPublic(ip) {
  const b = bytes(ip);
  if (!b) return 'not an';
  const u = unwrap(b);
  for (const [r, why] of NOT_PUBLIC) if (within(u, r)) return why;
  return null;
}

// `[browser] allow` entries: `name`, `name:port`, `1.2.3.4`, `[::1]:8080`,
// `10.0.0.0/8`.
function allowRange(cidr) {
  const [b, n] = range(cidr);
  return n >= 96 && within(b, CARRIES_IPV4[0][0]) ? [b.slice(12), n - 96] : [b, n];
}

function parseAllow(entry) {
  const s = String(entry).trim().toLowerCase().replace(/\.$/, '');
  if (s.includes('/') || net.isIPv6(s)) return { range: allowRange(s), port: null };
  const m = /^(\[[^\]]+\]|[^:[\]]+)(?::(\d{1,5}))?$/.exec(s);
  if (!m) throw new Error(`not a name, address or range: ${entry}`);
  const host = m[1].replace(/^\[|\]$/g, '');
  const port = m[2] ? Number(m[2]) : null;
  return net.isIP(host) ? { range: allowRange(host), port } : { name: host, port };
}

class Refused extends Error {}

function policy(allowEntries) {
  const allow = allowEntries.map(parseAllow);
  const allowed = (name, ip, port) => allow.some(a => (a.port === null || a.port === port) &&
    (a.name ? a.name === name : within(unwrap(bytes(ip)), a.range)));

  // The addresses host:port may be reached at, checked, in the resolver's
  // order; throws Refused when there are none.
  async function destination(host, port) {
    const name = String(host).replace(/^\[|\]$/g, '').toLowerCase().replace(/\.$/, '');
    let addrs;
    if (net.isIP(name)) addrs = [name];
    else {
      try { addrs = (await dns.promises.lookup(name, { all: true, verbatim: true })).map(a => a.address); } catch { throw new Refused(`${name} does not resolve`); }
    }
    let why;
    const ok = [];
    for (const ip of addrs) {
      const w = notPublic(ip);
      if (!w || allowed(name, ip, port)) ok.push(ip);
      else why = why || `${name === ip ? name : `${name} (${ip})`} is ${w} address`;
    }
    if (ok.length) return ok;
    throw new Refused(`The browser opens only public websites: ${why}. An internal site opens only when the appliance's owner lists it in [browser] allow.`);
  }

  // A connection to the first of the checked addresses that accepts one,
  // tried in order as Chromium would; never an address outside the list.
  // Each attempt gets CONNECT_MS: without a cap a dead address holds the
  // whole navigation for the kernel's minutes-long SYN timeout (#426: the
  // CI runner's own name resolves to several addresses, and a stale one
  // blackholed SYNs past the page budget). `opts` ({ dial, ms }) is the
  // tests' seam; production passes none.
  const CONNECT_MS = 10000;
  function connect(addrs, port, done, opts) {
    const dial = (opts && opts.dial) || net.connect;
    const ms = (opts && opts.ms) || CONNECT_MS;
    let i = 0;
    const next = () => {
      if (i >= addrs.length) { done(new Error('no address accepted the connection')); return; }
      const s = dial(port, addrs[i++]);
      const timer = setTimeout(() => {
        s.removeAllListeners('error');
        s.on('error', () => {});
        s.destroy();
        next();
      }, ms);
      s.once('error', () => { clearTimeout(timer); next(); });
      s.once('connect', () => { clearTimeout(timer); s.removeAllListeners('error'); done(null, s); });
    };
    next();
  }

  async function check(url) {
    const u = new URL(url);
    await destination(u.hostname, Number(u.port) || (u.protocol === 'https:' ? 443 : 80));
  }

  // The egress proxy Chromium is launched with: every connection it makes.
  function proxy() {
    const HOP = new Set(['connection', 'keep-alive', 'proxy-connection', 'proxy-authorization', 'proxy-authenticate', 'te', 'trailer', 'upgrade']);
    const keep = raw => raw.filter((_, i) => i % 2 === 0).flatMap((k, i) => HOP.has(k.toLowerCase()) ? [] : [k, raw[2 * i + 1]]);
    const server = http.createServer(async (req, res) => {
      let u;
      try { u = new URL(req.url); } catch { res.writeHead(400).end(); return; }
      if (u.protocol !== 'http:') { res.writeHead(400).end(); return; }
      const port = Number(u.port) || 80;
      let addrs;
      try { addrs = await destination(u.hostname, port); } catch (e) {
        const body = Buffer.from(e.message);
        res.writeHead(403, { 'content-type': 'text/plain; charset=utf-8', 'content-length': body.length, 'x-magnus-refused': '1' }).end(body);
        return;
      }
      connect(addrs, port, (err, sock) => {
        if (err) { res.writeHead(502).end(); return; }
        const up = http.request({ createConnection: () => sock, method: req.method, path: u.pathname + u.search, headers: keep(req.rawHeaders), setHost: false }, r => {
          res.writeHead(r.statusCode, r.statusMessage, keep(r.rawHeaders));
          // An origin that stops before its body is complete: the browser
          // sees the cut instead of waiting for bytes that never come.
          r.on('close', () => { if (!r.complete) res.destroy(); });
          r.pipe(res);
        });
        up.on('error', () => { if (!res.headersSent) res.writeHead(502).end(); else res.destroy(); });
        req.pipe(up);
      });
    });
    // HTTPS and WebSockets: a tunnel to the checked address.
    server.on('connect', async (req, client, head) => {
      client.on('error', () => {});
      const m = /^(\[[^\]]+\]|[^:]+):(\d+)$/.exec(req.url);
      let addrs;
      try { if (!m) throw new Refused('bad CONNECT target'); addrs = await destination(m[1], Number(m[2])); } catch (e) {
        client.end(`HTTP/1.1 403 Forbidden\r\ncontent-type: text/plain\r\ncontent-length: ${Buffer.byteLength(e.message)}\r\nconnection: close\r\n\r\n${e.message}`);
        return;
      }
      connect(addrs, Number(m[2]), (err, up) => {
        if (err) { client.destroy(); return; }
        if (client.destroyed) { up.destroy(); return; }
        up.on('error', () => client.destroy());
        client.on('close', () => up.destroy());
        client.write('HTTP/1.1 200 Connection Established\r\n\r\n');
        if (head.length) up.write(head);
        up.pipe(client);
        client.pipe(up);
      });
    });
    server.on('upgrade', (req, socket) => socket.destroy());
    return server;
  }

  return { destination, check, connect, proxy };
}

module.exports = { policy, notPublic, Refused };
