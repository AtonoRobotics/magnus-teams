// Runs inside the browser jail, which has only a loopback network. Chromium
// needs a TCP proxy address, and the egress proxy is outside on a Unix socket,
// so this joins the two. It starts Chromium once it is listening.
// Usage: node relay.js <port> <proxy socket> <chrome> [chrome args...]
'use strict';
const net = require('net');
const { spawn } = require('child_process');

const [port, sock, chrome, ...args] = process.argv.slice(2);
net.createServer(c => {
  const up = net.connect(sock);
  c.on('error', () => up.destroy());
  up.on('error', () => c.destroy());
  c.pipe(up).pipe(c);
}).listen(Number(port), '127.0.0.1', () => {
  // Fds 3 and 4 are Playwright's control pipe to Chromium.
  const child = spawn(chrome, args, { stdio: [0, 1, 2, 3, 4] });
  child.on('exit', (code, signal) => process.exit(signal ? 1 : code));
  child.on('error', () => process.exit(1));
});
