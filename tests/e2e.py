#!/usr/bin/env python3
"""Real BitTorrent -> RAM -> HTTP integration test. Generates data in memory.

No public swarm is contacted. Exercises metadata exchange, hash verification,
cross-file pieces, eviction, seeks, overlapping readers and cache teardown.
"""
import argparse
import concurrent.futures
import hashlib
import http.server
import json
import os
from pathlib import Path
import socket
import socketserver
import struct
import subprocess
import tempfile
import threading
import time
import urllib.error
import urllib.parse
import urllib.request

BLOCK = 16384
PIECE = 1024 * 1024

def bencode(v):
    if isinstance(v, int): return b'i' + str(v).encode() + b'e'
    if isinstance(v, str): return bencode(v.encode())
    if isinstance(v, bytes): return str(len(v)).encode() + b':' + v
    if isinstance(v, list): return b'l' + b''.join(map(bencode, v)) + b'e'
    return b'd' + b''.join(bencode(k) + bencode(v[k]) for k in sorted(v)) + b'e'

def bdecode(data, at=0):
    c = data[at:at+1]
    if c == b'i':
        end = data.index(b'e', at); return int(data[at+1:end]), end+1
    if c in (b'l', b'd'):
        items = []; at += 1
        while data[at:at+1] != b'e':
            value, at = bdecode(data, at); items.append(value)
        return (items if c == b'l' else dict(zip(items[::2], items[1::2]))), at+1
    colon = data.index(b':', at); n = int(data[at:colon]); at = colon+1
    return data[at:at+n], at+n

def payload(offset, length):
    out = bytearray()
    while length:
        block, within = divmod(offset, BLOCK)
        pattern = hashlib.sha256(struct.pack('!Q', block)).digest() * (BLOCK//32)
        n = min(length, BLOCK-within)
        out.extend(pattern[within:within+n]); offset += n; length -= n
    return bytes(out)

def recv_exact(sock, n):
    out = bytearray()
    while len(out) < n:
        part = sock.recv(n-len(out))
        if not part: raise EOFError()
        out.extend(part)
    return bytes(out)

class Swarm:
    def __init__(self, size_mib, corrupt=False, source=None, stalling_peer=False, peers=1, peer_mbps=0):
        self.source = Path(source) if source else None
        self.prefix = 0 if source else PIECE + 123
        self.movie_size = self.source.stat().st_size if source else size_mib * 1024 * 1024
        self.total = self.prefix + self.movie_size
        self.hashes = b''.join(hashlib.sha1(self.data_at(p, min(PIECE,self.total-p))).digest() for p in range(0,self.total,PIECE))
        files = [{b'length':self.movie_size,b'path':[self.source.name.encode()]}] if source else [{b'length':self.prefix,b'path':[b'header.bin']},{b'length':self.movie_size,b'path':[b'movie.mkv']}]
        self.info = bencode({b'name':b'RAM test', b'piece length':PIECE, b'pieces':self.hashes,
            b'files':files})
        self.info_hash = hashlib.sha1(self.info).digest()
        self.corrupt = corrupt; self.corrupted = False; self.lock = threading.Lock()
        self.bytes_served = 0; self.stalled_requests = 0
        rate = peer_mbps*1_000_000/8  # bytes per second per connection, 0 for no limit
        count=len(self.hashes)//20
        bits=bytearray([255]*((count+7)//8))
        if count%8: bits[-1]=(255 << (8-count%8))&255
        swarm = self
        class Peer(socketserver.BaseRequestHandler):
            def handle(self):
                s=self.request; s.settimeout(30)
                def send(data): s.sendall(struct.pack('!I',len(data))+data)
                try:
                    handshake=recv_exact(s,68)
                    if handshake[28:48]!=swarm.info_hash: return
                    s.sendall(b'\x13BitTorrent protocol'+b'\0\0\0\0\0\x10\0\0'+swarm.info_hash+b'-PY0001-123456789012')
                    send(b'\x05'+bits); send(b'\x01')
                    send(b'\x14\x00'+bencode({b'm':{b'ut_metadata':1},b'metadata_size':len(swarm.info),b'reqq':16}))
                    meta_id=1; ready=time.monotonic()
                    while True:
                        n=struct.unpack('!I',recv_exact(s,4))[0]
                        if n==0: continue
                        if n>128*1024: return
                        msg=recv_exact(s,n)
                        if msg[0]==6:
                            piece,off,length=struct.unpack('!III',msg[1:])
                            if length>BLOCK or piece*PIECE+off+length>swarm.total: return
                            data=swarm.data_at(piece*PIECE+off,length)
                            with swarm.lock:
                                if swarm.corrupt and not swarm.corrupted:
                                    data=bytes([data[0]^255])+data[1:]; swarm.corrupted=True
                                swarm.bytes_served+=length
                            if rate:
                                # A steady limit: time spent idle doesn't build up a burst.
                                ready=max(ready,time.monotonic())
                                time.sleep(ready-time.monotonic()) if ready>time.monotonic() else None
                                ready+=length/rate
                            send(b'\x07'+struct.pack('!II',piece,off)+data)
                        elif msg[0]==20:
                            value, used=bdecode(msg[2:])
                            if msg[1]==0: meta_id=value.get(b'm',{}).get(b'ut_metadata',1)
                            elif value.get(b'msg_type')==0:
                                i=value[b'piece']; part=swarm.info[i*BLOCK:(i+1)*BLOCK]
                                send(bytes([20,meta_id])+bencode({b'msg_type':1,b'piece':i,b'total_size':len(swarm.info)})+part)
                except (EOFError, OSError, ValueError): pass
        class Staller(socketserver.BaseRequestHandler):
            # Like a peer that chokes us mid-piece: it unchokes us, answers no request,
            # and stays connected with keep-alives.
            def handle(self):
                s=self.request; s.settimeout(30); lock=threading.Lock(); done=threading.Event()
                def send(data):
                    with lock: s.sendall(struct.pack('!I',len(data))+data)
                def keep_alive():
                    while not done.wait(4):
                        try: send(b'')
                        except OSError: return
                try:
                    handshake=recv_exact(s,68)
                    if handshake[28:48]!=swarm.info_hash: return
                    s.sendall(b'\x13BitTorrent protocol'+b'\0'*8+swarm.info_hash+b'-PY0002-123456789012')
                    send(b'\x05'+bits); send(b'\x01')
                    threading.Thread(target=keep_alive,daemon=True).start()
                    s.settimeout(None)
                    while True:
                        n=struct.unpack('!I',recv_exact(s,4))[0]
                        if n and recv_exact(s,n)[0]==6:
                            with swarm.lock: swarm.stalled_requests+=1
                except (EOFError, OSError, ValueError): pass
                finally: done.set()
        class Server(socketserver.ThreadingTCPServer):
            allow_reuse_address=True; daemon_threads=True
        self.serving=[Server(('127.0.0.1',0),Peer) for _ in range(peers)]
        self.staller=Server(('127.0.0.1',0),Staller) if stalling_peer else None
        peers=([self.staller] if self.staller else [])+self.serving
        class Tracker(http.server.BaseHTTPRequestHandler):
            def log_message(self,*args): pass
            def do_GET(self):
                if self.path.startswith('/file.torrent'):
                    data=swarm.torrent
                else:
                    compact=b''.join(socket.inet_aton('127.0.0.1')+struct.pack('!H',p.server_address[1]) for p in peers)
                    data=bencode({b'interval':2,b'complete':len(peers),b'incomplete':0,b'peers':compact})
                self.send_response(200); self.send_header('Content-Length',str(len(data))); self.end_headers(); self.wfile.write(data)
        self.tracker=http.server.ThreadingHTTPServer(('127.0.0.1',0),Tracker)
        self.tracker_url=f'http://127.0.0.1:{self.tracker.server_address[1]}/announce'
        self.torrent=bencode({b'announce':self.tracker_url.encode(),b'info':bdecode(self.info)[0]})
        self.servers=peers+[self.tracker]
        for server in self.servers: threading.Thread(target=server.serve_forever,daemon=True).start()

    def close(self):
        for s in self.servers: s.shutdown(); s.server_close()

    def data_at(self, offset, length):
        if not self.source: return payload(offset,length)
        with self.source.open('rb') as f:
            f.seek(offset); return f.read(length)

def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('--binary',default='target/release/torrstream-lite')
    parser.add_argument('--cache-mib',type=int,default=6)
    parser.add_argument('--size-mib',type=int,default=64)
    parser.add_argument('--rate-mbps',type=float,default=0)
    parser.add_argument('--corrupt-first',action='store_true')
    parser.add_argument('--stalling-peer',action='store_true',help='add a peer that answers no requests')
    parser.add_argument('--peers',type=int,default=1,help='peers that serve the data')
    parser.add_argument('--peer-mbps',type=float,default=0,help='speed limit of each peer')
    parser.add_argument('--report',default='test-results/e2e.json')
    args=parser.parse_args()
    binary=str(Path(args.binary).resolve()); report=Path(args.report).resolve(); report.parent.mkdir(parents=True,exist_ok=True)
    swarm=Swarm(args.size_mib,args.corrupt_first,stalling_peer=args.stalling_peer,peers=args.peers,peer_mbps=args.peer_mbps)
    # The Mac app passes a pipe as standard input; when the app ends, however it ends, so does the server.
    with tempfile.TemporaryDirectory() as tmp:
        orphan=subprocess.Popen([binary,'--listen','127.0.0.1:0','--no-dht','--exit-on-stdin-close'],cwd=tmp,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,text=True)
        try:
            for _ in range(100):
                line=orphan.stdout.readline()
                if not line or line.startswith('TORRSTREAM_LISTEN='): break
            assert line.startswith('TORRSTREAM_LISTEN='), 'Server did not start'
            orphan.stdin.close()
            orphan.wait(timeout=5)
        finally:
            if orphan.poll() is None: orphan.kill(); raise AssertionError('The server outlived its app')
    print('PASS the server exits when its app goes away',flush=True)
    with tempfile.TemporaryDirectory(prefix='torrstream-lite-e2e-') as tmp:
        log_path=report.with_suffix('.log')
        with log_path.open('w') as log:
            process=subprocess.Popen([binary,'--listen','127.0.0.1:0','--cache-mib',str(args.cache_mib),'--no-dht','--idle-seconds','0'],cwd=tmp,stdout=log,stderr=log)
        samples=[]; stop_poll=threading.Event(); results=[]
        try:
            base=None
            for _ in range(200):
                for line in log_path.read_text().splitlines():
                    if line.startswith('TORRSTREAM_LISTEN='): base=line.split('=',1)[1]
                if base: break
                if process.poll() is not None: raise RuntimeError(log_path.read_text())
                time.sleep(.05)
            assert base, 'Server startup timeout'
            def request(path,method='GET',data=None,headers=None):
                return urllib.request.urlopen(urllib.request.Request(base+path,data=data,headers=headers or {},method=method),timeout=65)
            def js(path='/api/status',**kw):
                with request(path,**kw) as r: return json.load(r)
            def mark(name): results.append(name); print('PASS',name,flush=True)
            def poll():
                while not stop_poll.wait(.1):
                    try: samples.append(js())
                    except Exception: pass
            poller=threading.Thread(target=poll,daemon=True); poller.start()
            idle=js(); assert idle['cache']['allocated_bytes']==0
            t=js('/api/torrents/file',method='POST',data=swarm.torrent,headers={'Content-Type':'application/x-bittorrent'})
            assert len(t['file_stats'])==2 and t['file_stats'][1]['length']==swarm.movie_size
            time.sleep(.2)
            assert js()['cache']['reserved_bytes']==0, 'Downloaded without a reader'
            mark('metadata and demand-only scheduling')
            status=js()
            assert status['torrent']['file_count']==2 and 'file_stats' not in status['torrent'] and status['memory_bytes']>0
            listed=js('/api/files')
            assert listed['hash']==t['hash'] and listed['files'][1]['length']==swarm.movie_size
            mark('status leaves out the file list, which /api/files returns')
            with request('/api/stream/2',method='HEAD') as r: assert int(r.headers['Content-Length'])==swarm.movie_size
            assert js()['cache']['reserved_bytes']==0
            mark('HEAD does not download')
            longest_wait=[0.0]
            def read_range(offset,length,path='/api/stream/2',rate=0):
                started=time.monotonic(); total=0
                with request(path,headers={'Range':f'bytes={offset}-{offset+length-1}'}) as r:
                    assert r.status==206
                    assert r.headers['Content-Range']==f'bytes {offset}-{offset+length-1}/{swarm.movie_size}'
                    while total<length:
                        asked=time.monotonic()
                        chunk=r.read(min(64*1024,length-total))
                        longest_wait[0]=max(longest_wait[0],time.monotonic()-asked)
                        assert chunk, f'Truncated at {total}'
                        assert chunk==payload(swarm.prefix+offset+total,len(chunk)), f'Data mismatch at {offset+total}'
                        total+=len(chunk)
                        if rate:
                            delay=total/(rate*1_000_000/8)-(time.monotonic()-started)
                            if delay>0: time.sleep(delay)
                return time.monotonic()-started
            elapsed=read_range(0,swarm.movie_size,rate=args.rate_mbps)
            mark('full file larger than cache, byte-for-byte verified')
            assert js()['cache']['evictions']>0
            read_range(0,2*PIECE+97)
            read_range(swarm.movie_size-PIECE-23,PIECE+23)
            read_range(7*PIECE+59,PIECE+300)
            mark('backward and forward seeks re-download evicted pieces')
            with request('/api/stream/2',headers={'Range':'bytes=-123'}) as r:
                assert r.read()==payload(swarm.total-123,123)
            mark('suffix range')
            with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
                futures=[pool.submit(read_range,offset,3*PIECE+101) for offset in [0,swarm.movie_size-5*PIECE]]
                for f in futures: f.result(timeout=65)
            mark('overlapping head/tail readers')
            read_range(PIECE+11,PIECE+13,f'/stream?link={t["hash"]}&index=2&play')
            with request(f'/stream?link={t["hash"]}&m3u') as r: assert b'index=2&play' in r.read()
            listed=js('/torrents',method='POST',data=b'{"action":"list"}',headers={'Content-Type':'application/json'})
            assert listed[0]['hash']==t['hash']
            mark('TorrServer stream, playlist and list subset')
            for path,h,expected in [('/api/stream/2',{'Range':'bytes=999999999999-'},416),('/api/stream/0',{},400)]:
                try: request(path,headers=h); raise AssertionError('Expected error')
                except urllib.error.HTTPError as e: assert e.code==expected
            try: js('/api/torrents/file',method='POST',data=swarm.torrent); raise AssertionError('Second torrent admitted')
            except urllib.error.HTTPError as e: assert e.code==409
            mark('invalid ranges and second torrent rejected')
            snapshot=js()
            js('/api/torrents',method='DELETE')
            for _ in range(50):
                if js()['cache']['allocated_bytes']==0: break
                time.sleep(.1)
            assert js()['cache']['allocated_bytes']==0
            stopped_rss=js()['resident_bytes']
            if args.cache_mib>=32:
                assert snapshot['resident_bytes']-stopped_rss > args.cache_mib*1048576*.7, 'Cache pages were not returned to OS'
            mark('stop releases RAM')
            magnet='magnet:?xt=urn:btih:'+swarm.info_hash.hex()+'&tr='+urllib.parse.quote(swarm.tracker_url,safe='')
            t=js('/api/torrents',method='POST',data=json.dumps({'link':magnet}).encode(),headers={'Content-Type':'application/json'})
            read_range(12345,PIECE+17)
            mark('magnet metadata exchange and playback')
            held=request('/api/stream/2')
            held.read(1024)
            time.sleep(.2)
            js('/api/torrents',method='DELETE')
            assert js()['cache']['allocated_bytes']==0, 'Slow reader held cache after stop'
            held.close()
            mark('stop cancels a slow HTTP reader and frees its cache')
            assert not list(Path(tmp).iterdir()), 'Server wrote files in RAM-only mode'
            mark('no payload or session files written')
            if args.corrupt_first: assert swarm.corrupted; mark('corrupt piece rejected and recovered')
            if args.peers>4 and args.peer_mbps:
                # Before, at most four pieces downloaded at once, so four peers at most sent data.
                four=4*args.peer_mbps
                assert swarm.movie_size*8/elapsed/1e6>1.25*four, f'No faster than four peers: {swarm.movie_size*8/elapsed/1e6:.0f} Mbps'
                mark(f'{args.peers} peers send at once')
            if args.stalling_peer:
                assert swarm.stalled_requests, 'The stalling peer was never asked for a piece'
                assert longest_wait[0]<15, f'A read waited {longest_wait[0]:.1f} s'
                mark('pieces held by a peer that stopped sending go to another peer')
            assert samples and all(s['cache']['reserved_bytes']<=args.cache_mib*1048576 and s['cache']['allocated_bytes']<=args.cache_mib*1048576 for s in samples)
            mark('cache budget respected throughout')
            out={'passed':results,'cache_mib':args.cache_mib,'movie_mib':args.size_mib,'rate_limit_mbps':args.rate_mbps,'peers':args.peers,'peer_mbps':args.peer_mbps,
                'transfer_seconds':round(elapsed,3),'effective_mbps':round(swarm.movie_size*8/elapsed/1e6,2),
                'idle_rss_mib':round(idle['resident_bytes']/1048576,2),'peak_rss_mib':round(max(s['resident_bytes'] or 0 for s in samples)/1048576,2),
                'rss_after_stop_mib':round(stopped_rss/1048576,2),'longest_read_wait_seconds':round(longest_wait[0],3),
                'stalled_requests':swarm.stalled_requests,
                'peak_reserved_mib':max(s['cache']['peak_reserved_bytes'] for s in samples)/1048576,
                'evictions_before_stop':snapshot['cache']['evictions'],'samples':len(samples), 'payload_written_to_disk':False}
            report.write_text(json.dumps(out,indent=2)+'\n'); print(json.dumps(out,indent=2))
        finally:
            stop_poll.set(); process.terminate()
            try: process.wait(timeout=8)
            except subprocess.TimeoutExpired: process.kill(); process.wait()
            swarm.close()

if __name__=='__main__': main()
