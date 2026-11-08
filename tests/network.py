#!/usr/bin/env python3
"""Exercise TV listeners over the Mac's LAN address using a local torrent peer."""
import argparse
import http.client
import json
from pathlib import Path
import socket
import subprocess
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request

from e2e import Swarm, payload

p = argparse.ArgumentParser()
p.add_argument('--binary', default='target/release/torrstream-lite')
p.add_argument('--report', default='test-results/tv-access.json')
args = p.parse_args()
binary = str(Path(args.binary).resolve())
opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
passed = []

def mark(message):
    passed.append(message)
    print('PASS', message, flush=True)

def request(base, path, data=None, headers=None, method=None):
    return opener.open(urllib.request.Request(base + path, data=data, headers=headers or {}, method=method), timeout=8)

def api(base, path='/api/network', data=None):
    with request(base, path, json.dumps(data).encode() if data is not None else None,
                 {'Content-Type': 'application/json'}) as response:
        return json.load(response)

def unused_port():
    with socket.socket() as sock:
        sock.bind(('127.0.0.1', 0))
        return sock.getsockname()[1]

swarm = Swarm(32)
with tempfile.TemporaryDirectory(prefix='torrstream-network-') as tmp:
    log = Path(tmp) / 'server.log'
    with log.open('w') as output:
        process = subprocess.Popen([binary, '--listen', '127.0.0.1:0', '--cache-mib', '6', '--no-dht'],
                                   cwd=tmp, stdout=output, stderr=output)
    try:
        for _ in range(200):
            base = next((l.split('=', 1)[1] for l in log.read_text().splitlines()
                         if l.startswith('TORRSTREAM_LISTEN=')), None)
            if base: break
            if process.poll() is not None: raise RuntimeError(log.read_text())
            time.sleep(.05)
        else: raise RuntimeError('Server startup timeout')
        n = api(base)
        assert not n['enabled'] and not n['addresses'] and n['local_control']
        mark('TV access starts disabled')
        with request(base, '/api/torrents/file', swarm.torrent, {'Content-Type': 'application/x-bittorrent'}) as r:
            torrent = json.load(r)
        n = api(base, data={'enabled': True, 'port': unused_port()})
        assert n['addresses'], ('No usable LAN address; test requires Wi-Fi/Ethernet', n)
        lan = n['addresses'][0]['url']
        assert api(lan)['local_control'] is False
        assert api(base, '/api/status')['torrent']['hash'] == torrent['hash']
        mark('LAN address is reachable without restarting the torrent')
        try: api(lan, data={'enabled': False})
        except urllib.error.HTTPError as e: assert e.code == 403
        else: raise AssertionError('Remote client changed LAN settings')
        mark('TV clients cannot change local network settings')
        for origin in (base, lan):
            with request(origin, '/api/stream/2', headers={'Range': 'bytes=11-1048586'}) as r:
                assert r.status == 206
                assert r.read() == payload(swarm.prefix + 11, 1048576)
        with request(lan, '/stream?link=' + torrent['hash'] + '&m3u') as r:
            assert lan.encode() in r.read()
        mark('Byte ranges and playlist URLs work over LAN')
        parsed = urllib.parse.urlsplit(lan)
        connection = http.client.HTTPConnection(parsed.hostname, parsed.port, timeout=5)
        connection.request('GET', '/api/stream/2')
        response = connection.getresponse()
        response.read(1024)
        n = api(base, data={'enabled': False})
        assert not n['addresses']
        partial = 1024
        try: partial += len(response.read())
        except http.client.IncompleteRead as error: partial += len(error.partial)
        connection.close()
        assert partial < swarm.movie_size, 'TV response continued after disabling access'
        try: api(lan)
        except (urllib.error.URLError, OSError): pass
        else: raise AssertionError('TV listener still accepts connections')
        assert api(base, '/api/status')['torrent']['hash'] == torrent['hash']
        with request(base, '/api/stream/2', headers={'Range': 'bytes=-4096'}) as r:
            assert r.read() == payload(swarm.total - 4096, 4096)
        mark('Disabling TV access closes TV streams and preserves Mac playback')
        try: api(base, data={'enabled': True, 'port': 0})
        except urllib.error.HTTPError as e: assert e.code == 400
        else: raise AssertionError('Invalid TV port accepted')
        assert not api(base)['enabled']
        mark('Invalid port rejected without changing settings')
        with socket.socket() as conflict:
            conflict.bind((parsed.hostname, 0)); conflict.listen()
            port = conflict.getsockname()[1]
            n = api(base, data={'enabled': True, 'port': port})
            assert n['errors'], n
            assert not any(a['url'] == f'http://{parsed.hostname}:{port}' for a in n['addresses'])
        for _ in range(50):
            n = api(base)
            if any(a['url'] == f'http://{parsed.hostname}:{port}' for a in n['addresses']): break
            time.sleep(.1)
        else: raise AssertionError('Listener did not recover after port was freed')
        mark('Port conflict is reported and listener recovers')
        api(base, data={'enabled': False})
        result = {'passed': passed, 'tested_lan_url': lan, 'cache_mib': 6,
                  'torrent_survived_tv_toggle': True, 'physical_tv_tested': False}
        Path(args.report).parent.mkdir(exist_ok=True, parents=True)
        Path(args.report).write_text(json.dumps(result, indent=2) + '\n')
    finally:
        process.terminate()
        try: process.wait(timeout=5)
        except subprocess.TimeoutExpired: process.kill(); process.wait()
        swarm.close()
