#!/usr/bin/env python3
"""Lampa's TorrServer HTTP contract with a real local BitTorrent peer.

Request shapes follow yumata/lampa-source's torserver.js and reguest.js:
JSON bodies with jQuery's default form content type, from Lampa's web origin.
The opaque "null" origin of TV apps is accepted only on listeners that face the
home network, so tests/network.py checks it on the TV listener; here it must be
refused. This verifies the protocol, not the physical Samsung TV or its decoder.
"""
import argparse
import json
from pathlib import Path
import subprocess
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request

from e2e import Swarm, payload

LAMPA = 'https://lampa.mx'


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--binary', default='target/release/torrstream-lite')
    parser.add_argument('--report', default='test-results/lampa.json')
    args = parser.parse_args()
    binary = str(Path(args.binary).resolve())
    report = Path(args.report).resolve()
    report.parent.mkdir(parents=True, exist_ok=True)
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    passed = []
    swarm = Swarm(32)
    # Two more torrents: the next one Lampa opens, and one whose magnet link resolves slowly.
    other = Swarm(24)
    slow = Swarm(16, metadata_delay=3)
    # Delay payload, not metadata. Preload must acknowledge before data arrives.
    data_at = swarm.data_at
    def delayed_payload(offset, length):
        time.sleep(.015)
        return data_at(offset, length)
    swarm.data_at = delayed_payload

    def mark(name):
        passed.append(name)
        print('PASS', name, flush=True)

    with tempfile.TemporaryDirectory(prefix='torrstream-lampa-') as tmp:
        log = report.with_suffix('.log')
        with log.open('w') as output:
            process = subprocess.Popen([binary, '--listen', '127.0.0.1:0',
                                        '--cache-mib', '6', '--no-dht'],
                                       cwd=tmp, stdout=output, stderr=output)
        try:
            for _ in range(200):
                base = next((line.split('=', 1)[1] for line in log.read_text().splitlines()
                             if line.startswith('TORRSTREAM_LISTEN=')), None)
                if base:
                    break
                if process.poll() is not None:
                    raise RuntimeError(log.read_text())
                time.sleep(.05)
            else:
                raise AssertionError('Server did not start')

            def request(path, data=None, origin=LAMPA, method=None, headers=None, timeout=10):
                h = {'Content-Type': 'application/x-www-form-urlencoded; charset=UTF-8'}
                if origin:
                    h['Origin'] = origin
                h.update(headers or {})
                body = json.dumps(data).encode() if data is not None else None
                return opener.open(urllib.request.Request(base + path, data=body, headers=h,
                                                          method=method), timeout=timeout)

            def api(path, data=None, **kwargs):
                with request(path, data, **kwargs) as response:
                    assert response.headers['Access-Control-Allow-Origin'] == kwargs.get('origin', LAMPA)
                    return json.load(response)

            def error(code, path, **kwargs):
                try:
                    request(path, **kwargs)
                except urllib.error.HTTPError as exc:
                    assert exc.code == code, (path, exc.code, exc.read())
                else:
                    raise AssertionError(f'{path}: expected HTTP {code}')

            empty_peers = api('/api/peers', origin=base)
            assert empty_peers['session'] is None and empty_peers['peers'] == []
            with request('/licenses/app', origin=base) as response:
                license_text = response.read().decode()
                assert 'MIT License' in license_text and '2026 Andrei Eliseev' in license_text
            with request('/licenses/third-party', origin=base) as response:
                notices = response.read().decode()
                assert 'Copyright 2021 Igor Katson' in notices
                assert 'Version 2.0, January 2004' in notices
            mark('Offline app and engine license notices; idle peer response')

            for origin in ('https://lampa.mx', 'https://lampa.app'):
                with request('/', origin=origin) as response:
                    assert response.status == 200
                settings = api('/settings', {'action': 'get'}, origin=origin)
                assert settings['CacheSize'] == 6 * 1048576 and not settings['UseDisk']
            # Plain HTTP can be faked on the way: the Mac's own server takes Lampa only over HTTPS.
            for origin in ('http://lampa.mx', 'http://lampa.app'):
                error(403, '/settings', origin=origin, data={'action': 'get'})
            mark("Lampa ping and playback connection accept form-labelled JSON and Lampa's HTTPS origins")
            # Any web page can send "null" from a sandboxed frame; the loopback server refuses it.
            error(403, '/settings', origin='null', data={'action': 'get'})
            error(403, '/torrents', origin='null', data={'action': 'list'})
            # A page that rebinds its own domain to this address still sends that domain.
            error(403, '/settings', data={'action': 'get'}, headers={'Host': 'rebind.example:8090'})
            error(403, '/api/status', origin=None, headers={'Host': 'rebind.example'})
            mark('Loopback server refuses the null origin and unknown host names')
            with request('/settings', method='OPTIONS', headers={
                'Access-Control-Request-Method': 'POST',
                'Access-Control-Request-Headers': 'content-type',
                'Access-Control-Request-Private-Network': 'true',
            }) as response:
                assert response.status == 204
                assert response.headers['Access-Control-Allow-Private-Network'] == 'true'
            for origin in ('null', 'https://lampa.mx'):
                error(403, '/api/network', origin=origin, data={'enabled': True})
                error(403, '/api/torrents', origin=origin, data={'link': 'invalid'})
                error(403, '/api/peers', origin=origin)
            error(403, '/settings', origin='https://unrelated.example', data={'action': 'get'})
            error(403, '/settings', origin='https://lampa.mx.unrelated.example', data={'action': 'get'})
            error(415, '/api/torrents', origin=base, data={'link': 'invalid'})
            error(422, '/settings', data=['not a request'])
            mark('Compatibility CORS does not grant access to native network controls or unrelated sites')

            torrent_url = swarm.tracker_url.replace('/announce', '/file.torrent')
            # An <img> or <video> tag on another site: no Origin header, cross-site fetch metadata.
            error(403, '/stream?' + urllib.parse.urlencode({'link': torrent_url, 'index': 1, 'play': ''}),
                  origin=None, headers={'Sec-Fetch-Site': 'cross-site'})
            assert api('/api/status', origin=base)['torrent'] is None
            mark('A stream URL loaded from another site cannot add a torrent')
            torrent = api('/torrents', {'action': 'add', 'link': torrent_url,
                                      'title': '[LAMPA] Local movie', 'poster': '',
                                      'data': '{}', 'category': 'movie', 'save_to_db': 'false'})
            hash_value = torrent['hash']
            details = api('/torrents', {'action': 'get', 'hash': hash_value}, timeout=2)
            assert details['file_stats'][1]['id'] == 2
            assert details['file_stats'][1]['length'] == swarm.movie_size
            assert api('/torrents', {'action': 'list'})[0]['hash'] == hash_value
            mark('Lampa torrent URL add, file listing and library responses')

            stream = '/stream/movie.mkv?' + urllib.parse.urlencode({'link': hash_value, 'index': 2})
            started = time.monotonic()
            progress = api(stream + '&preload', timeout=2)
            assert time.monotonic() - started < 2
            assert 0 < progress['preload_size'] <= 6 * 1048576 // 4
            assert progress['preloaded_bytes'] < progress['preload_size']
            # A repeated preload must not allocate a second reader/window.
            api(stream + '&preload', timeout=2)
            peer_samples = []
            for _ in range(200):
                progress = api(stream + '&stat', timeout=2)
                sample = api('/api/peers', origin=base)
                assert len(sample['peers']) <= sample['peer_limit']
                assert sample['session'].startswith(hash_value + ':')
                peer_samples.append(sample)
                assert not progress['preload_error'], progress
                assert progress['active_readers'] <= 1
                if progress['preloaded_bytes'] == progress['preload_size']:
                    break
                time.sleep(.1)
            else:
                raise AssertionError('Preload never completed')
            cache = api('/cache', {'action': 'get', 'hash': hash_value})
            assert 0 < cache['Filled'] <= cache['Capacity'] == 6 * 1048576
            assert cache['Torrent']['hash'] == hash_value
            mark('Preload returns within Lampa timeout, reports real progress and keeps bounded cache')
            received = [sum(p['received_bytes'] for p in s['peers']) for s in peer_samples]
            assert max(received) > min(received), received
            assert all(b['sampled_at_ms'] > a['sampled_at_ms'] for a, b in zip(peer_samples, peer_samples[1:]))
            with request('/api/peers', origin=base) as response:
                assert response.headers['Cache-Control'] == 'no-store'
            mark('Real peer byte counters advance with monotonic sample time and a bounded connection list')

            for offset in (0, swarm.movie_size - 10003):
                with request(stream + '&play', headers={'Range': f'bytes={offset}-{offset + 10002}'}) as response:
                    assert response.status == 206
                    assert response.read() == payload(swarm.prefix + offset, 10003)
            with request(stream + '&play', method='HEAD') as response:
                assert int(response.headers['Content-Length']) == swarm.movie_size
            mark('TV playback and seek responses are byte-for-byte correct')

            api('/viewed', {'action': 'set', 'hash': hash_value, 'file_index': 2, 'timecode': 123.5})
            assert api('/viewed', {'action': 'list', 'hash': hash_value}) == [
                {'hash': hash_value, 'file_index': 2, 'timecode': 123.5}]
            error(400, '/viewed', data={'action': 'set', 'hash': hash_value, 'file_index': 99999})
            api('/viewed', {'action': 'rem', 'hash': hash_value, 'file_index': -1})
            assert api('/viewed', {'action': 'list', 'hash': hash_value}) == []
            mark('Bounded in-memory Lampa playback positions')

            # Stop a fresh, delayed preload and prove it cannot retain the slab.
            api(stream + '&preload', timeout=2)
            api('/torrents', {'action': 'drop', 'hash': hash_value})
            status = api('/api/status', origin=base)
            assert status['cache']['allocated_bytes'] == 0 and status['torrent'] is None
            assert api('/api/peers', origin=base)['peers'] == []
            assert api('/viewed', {'action': 'list', 'hash': hash_value}) == []
            assert not list(Path(tmp).iterdir())
            mark('Stopping preload releases RAM and does not write payload/history files')

            # Lampa plays by hash. After "drop" (or the idle release) the hash brings the torrent back.
            with request(stream + '&play', headers={'Range': 'bytes=0-10002'}) as response:
                assert response.status == 206
                assert response.read() == payload(swarm.prefix, 10003)
            assert api('/api/status', origin=base)['torrent']['hash'] == hash_value
            mark('A dropped torrent comes back when Lampa plays it by hash')

            # Lampa adds the next torrent before it drops the one it played last.
            def lampa_add(link, title):
                return {'action': 'add', 'link': link, 'title': '[LAMPA] ' + title, 'poster': '',
                        'data': '{}', 'category': 'movie', 'save_to_db': 'false'}
            other_url = other.tracker_url.replace('/announce', '/file.torrent')
            next_one = api('/torrents', lampa_add(other_url, 'Next movie'))
            assert next_one['hash'] == other.info_hash.hex() and next_one['file_stats']
            # A late "drop" of the last one leaves the new one alone.
            error(404, '/torrents', data={'action': 'drop', 'hash': hash_value})
            assert api('/api/status', origin=base)['torrent']['hash'] == other.info_hash.hex()
            mark('Opening another torrent in Lampa replaces the one it played before')

            # Lampa gives "add" 30 s, then asks "get" every 2 s until the files are there,
            # because TorrServer answers at once and resolves a magnet link in the background.
            slow_hash = slow.info_hash.hex()
            magnet = 'magnet:?xt=urn:btih:' + slow_hash + '&tr=' + urllib.parse.quote(slow.tracker_url, safe='')
            started = time.monotonic()
            pending = api('/torrents', lampa_add(magnet, 'Slow movie'), timeout=5)
            assert time.monotonic() - started < 2, 'Adding waited for the magnet link to resolve'
            assert pending['hash'] == slow_hash and not pending.get('file_stats'), pending
            assert [t['hash'] for t in api('/torrents', {'action': 'list'})] == [slow_hash]
            assert api('/api/status', origin=base)['resolving']
            while not api('/torrents', {'action': 'get', 'hash': slow_hash}, timeout=2).get('file_stats'):
                assert time.monotonic() - started < 30, 'The magnet link never resolved'
                time.sleep(.2)
            assert time.monotonic() - started >= slow.metadata_delay
            mark("Lampa's add answers at once while a magnet link resolves, and the files follow")

            # Lampa's "drop" and the app's Stop cancel a torrent that's still resolving.
            api('/torrents', {'action': 'rem', 'hash': slow_hash})
            def native_stop():
                with request('/api/torrents', origin=base, method='DELETE') as response:
                    assert response.status == 200
            for stop in (lambda: api('/torrents', {'action': 'drop', 'hash': slow_hash}), native_stop):
                assert not api('/torrents', lampa_add(magnet, 'Slow movie'), timeout=5).get('file_stats')
                started = time.monotonic()
                stop()
                assert time.monotonic() - started < 2, 'Stopping waited for the magnet link'
                status = api('/api/status', origin=base)
                assert status['torrent'] is None and not status['resolving'], status
                assert status['cache']['allocated_bytes'] == 0, status
            mark('Dropping or stopping a torrent that is still resolving cancels it at once')
            report.write_text(json.dumps({'passed': passed, 'physical_tv_tested': False}, indent=2) + '\n')
        finally:
            process.terminate()
            try:
                process.wait(timeout=8)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
            for s in (swarm, other, slow):
                s.close()


if __name__ == '__main__':
    main()
