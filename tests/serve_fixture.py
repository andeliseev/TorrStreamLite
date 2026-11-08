#!/usr/bin/env python3
"""Serve a generated video as a local torrent for manual app testing."""
import argparse
from pathlib import Path
import signal
import subprocess
import tempfile
import time
from e2e import Swarm

p=argparse.ArgumentParser()
p.add_argument('--video',required=True)
p.add_argument('--binary',help='Also launch a standalone test server')
args=p.parse_args()
swarm=Swarm(0,source=args.video)
process=None
tmp=None
try:
    if args.binary:
        tmp=tempfile.TemporaryDirectory(prefix='torrstream-lite-control-test-')
        log=Path(tmp.name)/'server.log'
        with log.open('w') as output:
            process=subprocess.Popen([str(Path(args.binary).resolve()),'--listen','127.0.0.1:0','--idle-seconds','0'],cwd=tmp.name,stdout=output,stderr=output)
        for _ in range(100):
            lines=log.read_text().splitlines()
            address=next((l.split('=',1)[1] for l in lines if l.startswith('TORRSTREAM_LISTEN=')),None)
            if address: print('CONTROL_URL='+address,flush=True); break
            time.sleep(.05)
        else: raise RuntimeError(log.read_text())
    print('TORRENT_URL='+swarm.tracker_url.replace('/announce','/file.torrent'),flush=True)
    signal.signal(signal.SIGTERM,lambda *_:exit(0))
    signal.pause()
finally:
    if process:
        process.terminate()
        try: process.wait(timeout=5)
        except subprocess.TimeoutExpired: process.kill(); process.wait()
    swarm.close()
    if tmp: tmp.cleanup()
