#!/usr/bin/env python3
"""Decode a generated 4K clip through the running Mac app's torrent server."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import urllib.request
from e2e import Swarm

p=argparse.ArgumentParser()
p.add_argument('--source',required=True)
p.add_argument('--base',required=True)
p.add_argument('--report',default='test-results/video-4k.json')
args=p.parse_args()
def api(path,method='GET',data=None):
    req=urllib.request.Request(args.base+path,method=method,data=data,headers={'Content-Type':'application/x-bittorrent'})
    with urllib.request.urlopen(req,timeout=60) as r: return json.load(r)
def frames(source,seek=None):
    command=['ffmpeg','-nostdin','-v','error','-threads','2']
    if seek: command+=['-ss',str(seek)]
    command+=['-i',source,'-map','0:v:0']
    if seek: command+=['-t','1']
    command+=['-f','framemd5','-']
    out=subprocess.run(command,check=True,capture_output=True,timeout=120).stdout
    return [line for line in out.splitlines() if line and not line.startswith(b'#')]
assert api('/api/status')['torrent'] is None, 'Use an idle test app'
swarm=Swarm(0,source=args.source)
try:
    torrent=api('/api/torrents/file','POST',swarm.torrent)
    url=args.base+'/stream?link='+torrent['hash']+'&index=1&play'
    expected=frames(args.source)
    streamed=frames(url)
    assert expected==streamed, 'Decoded frame hashes differ'
    assert frames(args.source,8)==frames(url,8), 'Seek decoded frame hashes differ'
    probe=json.loads(subprocess.run(['ffprobe','-v','error','-select_streams','v:0','-show_entries','stream=width,height,codec_name','-of','json',args.source],capture_output=True,check=True).stdout)
    status=api('/api/status')
    report={'passed':True,'decoded_frames':len(streamed),'seek_seconds':8,'video':probe['streams'][0],
        'decoded_frames_sha256':hashlib.sha256(b'\n'.join(streamed)).hexdigest(),
        'cache':status['cache'],'server_rss_mib':round(status['resident_bytes']/1048576,2),
        'verification':'decoded frame hashes and seek; native player interaction not exercised'}
    Path(args.report).write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps(report,indent=2))
finally:
    api('/api/torrents','DELETE')
    swarm.close()
