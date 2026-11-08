// Two snapshots only; no ongoing collection while the peer panel is closed.
class PeerRates {
  constructor() {
    this.reset();
  }
  reset() {
    this.previous = null;
  }
  sample(current) {
    const previous = this.previous;
    const elapsed = previous
      ? current.sampled_at_ms - previous.sampled_at_ms
      : 0;
    const comparable =
      previous &&
      current.session === previous.session &&
      elapsed > 0 &&
      elapsed <= 10000;
    const peers = current.peers.map((peer) => {
      const old = comparable ? previous.peers.get(peer.address) : null;
      const valid =
        old &&
        old.connections === peer.connections &&
        peer.received_bytes >= old.received_bytes;
      return {
        ...peer,
        download_bps: valid
          ? ((peer.received_bytes - old.received_bytes) * 1000) / elapsed
          : null,
      };
    });
    this.previous = {
      session: current.session,
      sampled_at_ms: current.sampled_at_ms,
      peers: new Map(current.peers.map((peer) => [peer.address, peer])),
    };
    return peers;
  }
}
