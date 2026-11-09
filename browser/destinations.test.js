// The egress proxy tries a name's addresses in order (#426). A dead
// address used to hold the whole navigation for the kernel's minutes-long
// SYN timeout: the CI runner's own name resolves to several addresses, and
// a stale one blackholed SYNs past the page budget. Each attempt gets its
// own timeout now, and the proxy moves on to the next checked address.
'use strict';
const assert = require('node:assert/strict');
const net = require('node:net');
const { describe, it, before, after } = require('node:test');
const { policy } = require('./destinations');

const { connect } = policy([]);

// connect() settles (socket or error) well inside `budget`, or the test
// fails instead of hanging: without the per-attempt timeout the dead-first
// cases below never settle at all.
function settles(addrs, port, opts, budget = 20000) {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error('TIMED OUT: connect() never settled')), budget);
    connect(addrs, port, (err, sock) => {
      clearTimeout(timer);
      err ? reject(err) : resolve(sock);
    }, opts);
  });
}

describe('proxy connect()', () => {
  let server;
  let port;
  before(async () => {
    server = net.createServer(sock => sock.end());
    await new Promise(r => server.listen(0, '127.0.0.1', r));
    port = server.address().port;
  });
  after(() => server.close());

  // A dial that never settles: what a SYN blackhole looks like above TCP.
  const hang = () => new net.Socket();
  const dialOnce = made => (...args) => (made.used ? net.connect(...args) : (made.used = true, hang()));

  it('moves past a dead first address to the live one', async () => {
    const t0 = Date.now();
    const sock = await settles(['10.255.255.1', '127.0.0.1'], port, { dial: dialOnce({}), ms: 200 }, 20000);
    assert.ok(Date.now() - t0 < 5000, 'waited out the dead address instead of moving on');
    assert.ok(sock.connecting === false, 'not connected to the live address');
    sock.destroy();
  });

  it('errors instead of hanging when every address is dead', async () => {
    const t0 = Date.now();
    await assert.rejects(settles(['10.255.255.1'], port, { dial: () => hang(), ms: 200 }, 20000));
    assert.ok(Date.now() - t0 < 5000, 'hung instead of erroring');
  });
});
