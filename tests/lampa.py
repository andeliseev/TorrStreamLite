#!/usr/bin/env python3
"""Lampa's TorrServer HTTP contract with a real local BitTorrent peer.

Request shapes follow yumata/lampa-source's torserver.js and reguest.js:
JSON bodies with jQuery's default form content type, and opaque TV origins.
This verifies the protocol, not the physical Samsung TV or its video decoder.
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

            def request(path, data=None, origin='null', method=None, headers=None, timeout=10):
                h = {'Origin': origin, 'Content-Type': 'application/x-www-form-urlencoded; charset=UTF-8'}
                h.update(headers or {})
                body = json.dumps(data).encode() if data is not None else None
                return opener.open(urllib.request.Request(base + path, data=body, headers=h,
                                                          method=method), timeout=timeout)

            def api(path, data=None, **kwargs):
                with request(path, data, **kwargs) as response:
                    assert response.headers['Access-Control-Allow-Origin'] == kwargs.get('origin', 'null')
                    return json.load(response)

            def error(code, path, **kwargs):
                try:
                    request(path, **kwargs)
                except urllib.error.HTTPError as exc:
                    assert exc.code == code, (path, exc.code, exc.read())
                else:
                    raise AssertionError(f'{path}: expected HTTP {code}')

            for origin in ('null', 'http://lampa.mx', 'https://lampa.mx', 'http://lampa.app', 'https://lampa.app'):
                with request('/', origin=origin) as response:
                    assert response.status == 200
                settings = api('/settings', {'action': 'get'}, origin=origin)
                assert settings['CacheSize'] == 6 * 1048576 and not settings['UseDisk']
            mark('Lampa ping and playback connection accept form-labelled JSON and TV/web origins')
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
            error(403, '/settings', origin='https://unrelated.example', data={'action': 'get'})
            error(403, '/settings', origin='https://lampa.mx.unrelated.example', data={'action': 'get'})
            error(415, '/api/torrents', origin=base, data={'link': 'invalid'})
            error(422, '/settings', data=['not a request'])
            mark('Compatibility CORS does not grant access to native network controls or unrelated sites')

            torrent_url = swarm.tracker_url.replace('/announce', '/file.torrent')
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
            for _ in range(200):
                progress = api(stream + '&stat', timeout=2)
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
            assert api('/viewed', {'action': 'list', 'hash': hash_value}) == []
            assert not list(Path(tmp).iterdir())
            mark('Stopping preload releases RAM and does not write payload/history files')
            report.write_text(json.dumps({'passed': passed, 'physical_tv_tested': False}, indent=2) + '\n')
        finally:
            process.terminate()
            try:
                process.wait(timeout=8)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
            swarm.close()


if __name__ == '__main__':
    main()
