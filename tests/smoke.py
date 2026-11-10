#!/usr/bin/env python3
"""Smoke-run a server binary: does it start, answer and stream where it's run?

Written for the Keenetic build, which can't run on the machine that builds it. The Release
workflow runs it on an emulated router processor:

  python3 tests/smoke.py --binary dist/torrstream-lite-linux-arm64 \\
      --runner 'qemu-aarch64 -cpu cortex-a53' --https-url https://github.com/robots.txt

It checks --version; that the server starts without root certificates on the system (the
router's case) and still refuses a certificate nobody vouches for; that /api/status answers;
that a torrent from a local peer streams byte for byte; that Stop and SIGTERM work; and that
it also starts and streams with IPv4 only, as on a router without IPv6.
--https-url adds one request to that address, as a torrent URL, to see the built-in root
certificates accept a real site. Nothing else leaves this machine, and no swarm is public.
An emulator runs the program on this machine's kernel, so it says nothing about a router's.
"""
import argparse
import http.server
import json
import os
from pathlib import Path
import shlex
import shutil
import signal
import ssl
import subprocess
import sys
import tempfile
import threading
import time
import tomllib
import urllib.error
import urllib.request

from e2e import PIECE, Swarm, payload

ROOT = Path(__file__).resolve().parents[1]
CACHE_MIB = 6
# An emulated processor is several times slower than a real one.
PATIENCE = 90


def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    parser.add_argument('--binary', default='target/release/torrstream-lite')
    parser.add_argument('--runner', default='',
                        help="a command to run the binary with, e.g. 'qemu-aarch64 -cpu cortex-a53'")
    parser.add_argument('--https-url', help='also fetch this HTTPS address, to check the root certificates')
    parser.add_argument('--report', default='test-results/smoke.json')
    args = parser.parse_args()
    command = shlex.split(args.runner) + [str(Path(args.binary).resolve())]
    report = Path(args.report).resolve()
    report.parent.mkdir(parents=True, exist_ok=True)
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    passed = []

    def mark(name):
        passed.append(name)
        print('PASS', name, flush=True)

    version = tomllib.loads((ROOT / 'Cargo.toml').read_text())['package']['version']
    printed = subprocess.run(command + ['--version'], capture_output=True, text=True, timeout=PATIENCE)
    said = f'{" ".join(command)} --version: exit code {printed.returncode}\n{printed.stdout}{printed.stderr}'
    assert printed.returncode == 0 and printed.stdout.strip() == f'torrstream-lite {version}', said
    mark(f'--version prints {version}')

    swarm = Swarm(8)
    torrent_url = swarm.tracker_url.replace('/announce', '/file.torrent')
    running = []
    secure = None
    with tempfile.TemporaryDirectory(prefix='torrstream-smoke-') as tmp:
        tmp = Path(tmp)
        # A system without root certificates: on Linux the server reads only this file.
        no_roots = tmp / 'no-root-certificates.pem'
        no_roots.write_bytes(b'')
        environment = {k: v for k, v in os.environ.items() if k != 'SSL_CERT_DIR'}
        environment.update(SSL_CERT_FILE=str(no_roots), NO_COLOR='1')
        working = tmp / 'working-directory'
        working.mkdir()

        def start(log, *options):
            """Starts a server; returns it and its address once it listens."""
            with log.open('w') as output:
                process = subprocess.Popen(
                    command + ['--listen', '127.0.0.1:0', '--cache-mib', str(CACHE_MIB), '--no-dht', *options],
                    cwd=working, env=environment, stdout=output, stderr=output)
            running.append((process, log))
            deadline = time.monotonic() + PATIENCE
            while True:
                base = next((line.split('=', 1)[1] for line in log.read_text().splitlines()
                             if line.startswith('TORRSTREAM_LISTEN=')), None)
                if base:
                    return process, base
                assert process.poll() is None, 'The server stopped:\n' + log.read_text()
                assert time.monotonic() < deadline, 'The server did not start:\n' + log.read_text()
                time.sleep(.1)

        def request(base, path, data=None, method=None, headers=None):
            headers = dict(headers or {})
            if data is not None:
                headers['Content-Type'] = 'application/json'
            body = json.dumps(data).encode() if data is not None else None
            return opener.open(urllib.request.Request(base + path, data=body, headers=headers,
                                                      method=method), timeout=PATIENCE)

        def api(base, path, data=None, method=None):
            with request(base, path, data, method) as response:
                return json.load(response)

        def refused(base, link):
            """Why the server didn't add the torrent at this address."""
            try:
                request(base, '/api/torrents', {'link': link})
            except urllib.error.HTTPError as error:
                return json.load(error)['error']
            raise AssertionError(f'{link} was added as a torrent')

        def read_range(base, offset, length):
            with request(base, '/api/stream/2',
                         headers={'Range': f'bytes={offset}-{offset + length - 1}'}) as response:
                assert response.status == 206, response.status
                data = response.read()
            assert data == payload(swarm.prefix + offset, length), f'Data mismatch from {offset}'

        def add_torrent(base):
            torrent = api(base, '/api/torrents', {'link': torrent_url})
            assert torrent['hash'] == swarm.info_hash.hex(), torrent
            assert torrent['file_stats'][1]['length'] == swarm.movie_size, torrent

        def end(process, log):
            process.send_signal(signal.SIGTERM)
            code = process.wait(timeout=PATIENCE)
            assert code == 0, f'Exit code {code} after SIGTERM:\n' + log.read_text()

        try:
            log = report.with_suffix('.log')
            process, base = start(log)
            status = api(base, '/api/status')
            assert (status['server'], status['version'], status['mode']) == (
                'TorrStream Lite', version, 'RAM only'), status
            assert status['torrent'] is None and status['cache']['allocated_bytes'] == 0, status
            assert status['memory_bytes'] > 0, status
            if sys.platform.startswith('linux'):
                # macOS keeps its root certificates in the Keychain, whatever the file says.
                assert 'no root certificates' in log.read_text(), log.read_text()
                mark('starts on a system without root certificates, with the built-in list')
            mark('/api/status answers')

            # A torrent URL on an HTTPS server whose certificate no authority signed.
            openssl = shutil.which('openssl')
            assert openssl, 'This check makes a test certificate with the openssl command'
            certificate, key = tmp / 'certificate.pem', tmp / 'key.pem'
            subprocess.run([openssl, 'req', '-x509', '-newkey', 'rsa:2048', '-nodes', '-days', '2',
                            '-keyout', str(key), '-out', str(certificate), '-subj', '/CN=127.0.0.1',
                            '-addext', 'subjectAltName=IP:127.0.0.1'],
                           check=True, capture_output=True, timeout=PATIENCE)

            class TorrentFile(http.server.BaseHTTPRequestHandler):
                def log_message(self, *args):
                    pass

                def do_GET(self):
                    self.send_response(200)
                    self.send_header('Content-Length', str(len(swarm.torrent)))
                    self.end_headers()
                    self.wfile.write(swarm.torrent)

            listening = http.server.ThreadingHTTPServer(('127.0.0.1', 0), TorrentFile)
            context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
            context.load_cert_chain(certificate, key)
            listening.socket = context.wrap_socket(listening.socket, server_side=True)
            threading.Thread(target=listening.serve_forever, daemon=True).start()
            # Only a server that serves can be shut down: shutdown() waits for its loop to end.
            secure = listening
            secure_url = f'https://127.0.0.1:{secure.server_address[1]}/file.torrent'
            # The test server itself works, for a client that trusts its certificate.
            trusting = urllib.request.build_opener(
                urllib.request.ProxyHandler({}),
                urllib.request.HTTPSHandler(context=ssl.create_default_context(cafile=str(certificate))))
            with trusting.open(secure_url, timeout=PATIENCE) as response:
                assert response.read() == swarm.torrent
            reason = refused(base, secure_url)
            assert 'certificate' in reason.lower(), reason
            assert api(base, '/api/status')['torrent'] is None
            mark('refuses a torrent URL whose HTTPS certificate no authority signed')

            if args.https_url:
                # Whatever the page is, it isn't a torrent. What matters is that the request
                # got an answer: "error sending request" is how a failed connection reads,
                # a refused certificate included.
                reason = refused(base, args.https_url)
                assert 'error sending request' not in reason and 'certificate' not in reason.lower(), reason
                mark(f'the root certificates accept {args.https_url}')

            # The whole file is larger than the cache; then back to a piece that was evicted.
            add_torrent(base)
            read_range(base, 0, swarm.movie_size)
            read_range(base, PIECE + 4321, PIECE + 77)
            status = api(base, '/api/status')
            assert 0 < status['cache']['allocated_bytes'] <= CACHE_MIB * 1048576, status['cache']
            assert status['cache']['evictions'] > 0, status['cache']
            resident_mib = round((status['resident_bytes'] or 0) / 1048576, 2)
            mark('a torrent from a local peer streams byte for byte, through a cache smaller than the file')

            api(base, '/api/torrents', method='DELETE')
            deadline = time.monotonic() + PATIENCE
            while api(base, '/api/status')['cache']['allocated_bytes']:
                assert time.monotonic() < deadline, 'The cache was not released'
                time.sleep(.1)
            assert not list(working.iterdir()), 'The server wrote files'
            mark('Stop releases the cache, and nothing was written to disk')

            end(process, log)
            mark('SIGTERM ends the server cleanly')

            # A router's kernel may have no IPv6. The server then opens IPv4 sockets only,
            # which this option asks for where there is IPv6.
            log = report.with_suffix('.ipv4.log')
            process, base = start(log, '--ipv4-only')
            add_torrent(base)
            read_range(base, 2 * PIECE + 5, PIECE + 3)
            end(process, log)
            mark('starts and streams with IPv4 only')

            report.write_text(json.dumps({
                'passed': passed, 'command': command, 'version': version,
                'resident_mib_after_streaming': resident_mib, 'https_url': args.https_url,
                'real_router_tested': False}, indent=2) + '\n')
        except BaseException:
            for _, log in running:
                print(f'{log.name}:\n{log.read_text()}', file=sys.stderr, flush=True)
            raise
        finally:
            for process, _ in running:
                if process.poll() is None:
                    process.kill()
                    process.wait()
            if secure:
                secure.shutdown()
                secure.server_close()
            swarm.close()


if __name__ == '__main__':
    main()
