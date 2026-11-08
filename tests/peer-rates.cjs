// Rate calculations used by the real UI, including reconnect/stale samples.
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");
const PeerRates = vm.runInNewContext(
  fs.readFileSync(path.join(__dirname, "../ui/peer-rates.js"), "utf8") +
    "\nPeerRates",
);
const rates = new PeerRates();
function sample(time, bytes, options = {}) {
  return rates.sample({
    session: options.session || "torrent:1",
    sampled_at_ms: time,
    peers:
      bytes === null
        ? []
        : [
            {
              address: "127.0.0.1:6881",
              received_bytes: bytes,
              connections: options.connections ?? 1,
            },
          ],
  });
}
assert.equal(
  sample(1000, 100)[0].download_bps,
  null,
  "First observation has no rate",
);
assert.equal(
  sample(3000, 1000100)[0].download_bps,
  500000,
  "Use measured elapsed time",
);
assert.equal(
  sample(4500, 1000100)[0].download_bps,
  0,
  "Idle connected peer is zero",
);
assert.equal(
  sample(6000, 2000100, { connections: 2 })[0].download_bps,
  null,
  "Reconnect resets rate",
);
assert.equal(
  sample(7500, 0, { connections: 2 })[0].download_bps,
  null,
  "Counter reset never makes a negative rate",
);
assert.equal(
  sample(9000, 2000000, { session: "torrent:2" })[0].download_bps,
  null,
  "Torrent changes reset rate",
);
assert.equal(
  sample(22000, 5000000, { session: "torrent:2" })[0].download_bps,
  null,
  "Background gaps are not current speed",
);
assert.equal(
  sample(22000, 5000001, { session: "torrent:2" })[0].download_bps,
  null,
  "Repeated timestamps cannot divide by zero",
);
assert.equal(
  sample(100, 5000002)[0].download_bps,
  null,
  "Server restart resets time",
);
assert.equal(sample(1500, null).length, 0, "Disconnected peers disappear");
assert.equal(
  sample(3000, 9000000)[0].download_bps,
  null,
  "Rejoining peer has no stale sample",
);
rates.reset();
assert.equal(
  sample(4500, 9900000)[0].download_bps,
  null,
  "Closing/reopening the panel resets history",
);
assert.equal(
  rates.previous.peers.size,
  1,
  "Only the latest bounded snapshot is retained",
);
console.log("PASS peer rates, reconnects, teardown, and stale samples");
