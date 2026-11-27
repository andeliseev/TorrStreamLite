#!/usr/bin/env python3
"""A paused player with peers that close idle connections, then playing on and a seek.

qBittorrent's engine closes a connection it has unchoked that asks for nothing for 60 s
(libtorrent's timed_out_no_request), and a paused player asks for nothing. The local peers
here do the same after a few seconds. No public swarm is contacted.
"""
import argparse
import contextlib
import json
from pathlib import Path
import subprocess
import tempfile
import time
import urllib.request

from e2e import PIECE, Swarm, payload

p = argparse.ArgumentParser()
p.add_argument('--binary', default='target/release/torrstream-lite')
p.add_argument('--peers', type=int, default=3)
p.add_argument('--peer-mbps', type=float, default=8, help='speed limit of each peer')
p.add_argument('--idle-kick', type=float, default=3, help='seconds a peer lets an idle connection live')
p.add_argument('--pause', type=float, default=30, help='seconds the player stays paused')
p.add_argument('--report', default='test-results/pause.json')
args = p.parse_args()
binary = str(Path(args.binary).resolve())
report = Path(args.report).resolve(); report.parent.mkdir(parents=True, exist_ok=True)
opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
passed = []
CACHE_MIB = 6

def mark(message):
    passed.append(message)
    print('PASS', message, flush=True)

def wait_for(check, seconds, failure):
    deadline = time.monotonic() + seconds
    while not check():
        assert time.monotonic() < deadline, failure
        time.sleep(.1)

@contextlib.contextmanager
def server(swarm, name):
    """Runs the server with the swarm's torrent loaded; yields `request(path, headers=None)`."""
    with tempfile.TemporaryDirectory(prefix='torrstream-pause-') as tmp:
        log = report.with_suffix(f'.{name}.log')
        with log.open('w') as output:
            process = subprocess.Popen([binary, '--listen', '127.0.0.1:0', '--cache-mib', str(CACHE_MIB), '--no-dht', '--idle-seconds', '0'],
                                       cwd=tmp, stdout=output, stderr=output)
        try:
            for _ in range(200):
                base = next((l.split('=', 1)[1] for l in log.read_text().splitlines()
                             if l.startswith('TORRSTREAM_LISTEN=')), None)
                if base: break
                if process.poll() is not None: raise RuntimeError(log.read_text())
                time.sleep(.05)
            else: raise RuntimeError('Server startup timeout')
            def request(path, data=None, headers=None):
                return opener.open(urllib.request.Request(base + path, data=data, headers=headers or {}), timeout=90)
            with request('/api/torrents/file', swarm.torrent, {'Content-Type': 'application/x-bittorrent'}) as r:
                json.load(r)
            yield request
        finally:
            process.terminate()
            try: process.wait(timeout=8)
            except subprocess.TimeoutExpired: process.kill(); process.wait()
            swarm.close()

def read(swarm, response, offset, length):
    """Reads on from `offset`; returns the longest wait for a chunk."""
    longest = 0.0; total = 0
    while total < length:
        asked = time.monotonic()
        chunk = response.read(min(64 * 1024, length - total))
        longest = max(longest, time.monotonic() - asked)
        assert chunk, f'Truncated at {offset + total}'
        assert chunk == payload(swarm.prefix + offset + total, len(chunk)), f'Data mismatch at {offset + total}'
        total += len(chunk)
    return longest

swarm = Swarm(64, peers=args.peers, peer_mbps=args.peer_mbps, idle_kick=args.idle_kick)
with server(swarm, 'paused') as request:
    def live():
        with request('/api/status') as r: return json.load(r)['torrent']['active_peers']

    # A player that reads a little and pauses: the read-ahead fills the cache, then
    # nothing is asked of the peers, and each closes its connection.
    held = request('/api/stream/2')
    read(swarm, held, 0, PIECE)
    started = time.monotonic()
    wait_for(lambda: swarm.kicked >= args.peers, args.idle_kick + 20, 'The peers kept their idle connections')
    connected = swarm.connections
    time.sleep(max(0, args.pause - (time.monotonic() - started)))
    reconnects = swarm.connections - connected
    # Coming back to a peer with nothing to send only gets the connection closed again.
    assert reconnects == 0, f'{reconnects} reconnections while paused for {time.monotonic() - started:.0f} s'
    mark('peers that closed an idle connection are left alone while the player is paused')

    # Play on, faster than one peer sends and past what the cache holds.
    resumed = time.monotonic()
    wait_playing = read(swarm, held, PIECE, 16 * PIECE)
    took_playing = time.monotonic() - resumed
    assert wait_playing < 5, f'Playing on after the pause waited {wait_playing:.1f} s for data'
    one_peer = (16 - CACHE_MIB) * 1048576 / (args.peer_mbps * 1e6 / 8)
    assert took_playing < .75 * one_peer, f'16 MiB took {took_playing:.1f} s; one peer alone needs {one_peer:.1f} s'
    assert live() == args.peers, f'Only {live()} of {args.peers} peers came back'
    mark('playing on after a pause brings back as many peers as it takes')
    held.close()

    # A second pause, then a seek to a place that isn't cached.
    held = request('/api/stream/2', headers={'Range': f'bytes={20 * PIECE}-'})
    read(swarm, held, 20 * PIECE, PIECE)
    kicked = swarm.kicked
    wait_for(lambda: swarm.kicked >= kicked + args.peers, args.idle_kick + 20, 'The peers kept their idle connections')
    held.close()
    offset = swarm.movie_size // 2 + 77
    asked = time.monotonic()
    with request('/api/stream/2', headers={'Range': f'bytes={offset}-{offset + PIECE + 4}'}) as r:
        read(swarm, r, offset, PIECE + 5)
    took_seek = time.monotonic() - asked
    assert took_seek < 5, f'A seek after the pause took {took_seek:.1f} s'
    wait_for(lambda: live() == args.peers, 3, f'Only some of the {args.peers} peers came back for the seek')
    mark('a seek after a pause starts at once, with every peer')
    closed = swarm.kicked

# The end of a film, with a peer that takes the first piece and then sends nothing. The
# other two finish theirs within a second, have nothing more to do, and close their
# connections before the silent peer's piece counts as stuck. Nothing new is wanted after
# the last piece, so that stuck piece is all that's left to bring one of them back. (On a
# machine slow enough for the two to be still connected then, one of them takes the piece
# over, and `came_back` in the report is 0.)
swarm = Swarm(64, peers=2, peer_mbps=args.peer_mbps, idle_kick=1, stalling_peer=True)
with server(swarm, 'stalled') as request:
    # Without a reader the two close their connections; the silent one stays.
    wait_for(lambda: swarm.kicked >= 2, 21, 'The peers kept their idle connections')
    tail = 2 * PIECE
    asked = time.monotonic()
    with request('/api/stream/2', headers={'Range': f'bytes={swarm.movie_size - tail}-'}) as r:
        read(swarm, r, swarm.movie_size - tail, tail)
    took_tail = time.monotonic() - asked
    # Two connections before the stream and two for it; any more came back for the piece.
    came_back = swarm.connections - 4
    assert swarm.stalled_requests, 'The silent peer was never asked for a piece'
    assert took_tail < 25, f'The last {tail // PIECE} MiB took {took_tail:.1f} s'
    mark('a piece stuck with a silent peer brings back a peer that was set aside')

out = {'passed': passed, 'peers': args.peers, 'peer_mbps': args.peer_mbps, 'idle_kick_seconds': args.idle_kick,
       'pause_seconds': args.pause, 'idle_connections_closed': closed, 'reconnections_while_paused': reconnects,
       'longest_wait_playing_on_seconds': round(wait_playing, 3), 'playing_on_16_mib_seconds': round(took_playing, 3),
       'one_peer_would_take_seconds': round(one_peer, 3), 'seek_seconds': round(took_seek, 3),
       'tail_with_a_silent_peer_seconds': round(took_tail, 3), 'came_back_for_the_stuck_piece': came_back}
report.write_text(json.dumps(out, indent=2) + '\n'); print(json.dumps(out, indent=2))
