#!/usr/bin/env python3
"""Watch a running server from another computer: speed, peers, cache, memory and processor load.

For the checks on a router (docs/router.md): run it on the Mac while a film plays on the TV.

  python3 tests/watch.py http://192.168.1.1:8090 | tee -i router-run.txt

It reads /api/status every few seconds and prints a line; Ctrl-C, or --minutes, ends it
with a summary (tee's -i keeps tee alive on Ctrl-C, so the summary gets through).
It only reads: nothing is added, stopped or changed on the server.
"""
import argparse
import json
import time
import urllib.request

MIB = 1048576


def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    parser.add_argument('server', help='the server address, e.g. http://192.168.1.1:8090')
    parser.add_argument('--every', type=float, default=5, help='seconds between readings (default 5)')
    parser.add_argument('--minutes', type=float, default=0, help='stop after this long (default: until Ctrl-C)')
    args = parser.parse_args()
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    url = args.server.rstrip('/') + '/api/status'
    started = time.monotonic()
    samples = failed = 0
    last = None  # (when, cpu_seconds) of the reading before
    memory, peak, loads, speeds, version = [], 0, [], [], None

    print(f'{"time":8}  {"state":9}  {"Mbps":>6}  {"peers":>5}  {"cache MiB":>9}  {"memory MiB":>10}  {"CPU %":>6}', flush=True)
    try:
        while not args.minutes or time.monotonic() - started < args.minutes * 60:
            asked = time.monotonic()
            clock = time.strftime('%H:%M:%S')
            try:
                with opener.open(url, timeout=max(2, args.every)) as response:
                    status = json.load(response)
            except (OSError, ValueError) as error:
                failed += 1
                last = None
                print(f'{clock}  no answer: {error}', flush=True)
            else:
                samples += 1
                version = status.get('version')
                torrent = status.get('torrent')
                playing = bool(torrent and torrent.get('active_readers'))
                state = ('playing' if playing else 'loaded' if torrent
                         else 'resolving' if status.get('resolving') else 'idle')
                resident = (status.get('resident_bytes') or 0) / MIB
                memory.append(resident)
                peak = max(peak, (status.get('peak_resident_bytes') or 0) / MIB)
                cpu = status.get('cpu_seconds')
                load = None
                if cpu is not None and last and asked > last[0]:
                    load = max(0.0, (cpu - last[1]) / (asked - last[0]) * 100)
                last = (asked, cpu) if cpu is not None else None
                mbps = torrent['download_speed'] * 8 / 1e6 if torrent else None
                if playing:
                    speeds.append(mbps)
                    if load is not None:
                        loads.append(load)
                print(f'{clock}  {state:9}  {"-" if mbps is None else format(mbps, ".1f"):>6}  '
                      f'{torrent["active_peers"] if torrent else "-":>5}  '
                      f'{status["cache"]["reserved_bytes"] / MIB:9.0f}  {resident:10.1f}  '
                      f'{"-" if load is None else format(load, ".1f"):>6}', flush=True)
            time.sleep(max(0, args.every - (time.monotonic() - asked)))
    except KeyboardInterrupt:
        print()
    minutes = (time.monotonic() - started) / 60
    print(f'\n{samples} readings in {minutes:.1f} min from TorrStream Lite {version or "?"}; '
          f'{failed} without an answer')
    if memory:
        print(f'Memory: highest reading {max(memory):.1f} MiB, last {memory[-1]:.1f} MiB'
              + (f"; the server's own peak since it started {peak:.1f} MiB" if peak else ''))
    if speeds:
        # It pauses whenever the part being watched is in the cache, so the average is
        # about the film's own bitrate, and the highest shows what was to spare.
        print(f'Download while playing: average {sum(speeds) / len(speeds):.1f} Mbps, '
              f'highest {max(speeds):.1f}')
    if loads:
        print(f'Processor while playing: average {sum(loads) / len(loads):.1f}% of one core, '
              f'highest {max(loads):.1f}%')
    elif speeds:
        print('Processor: this server version does not report it')


if __name__ == '__main__':
    main()
