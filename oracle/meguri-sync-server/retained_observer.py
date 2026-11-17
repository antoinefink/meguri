# SPDX-License-Identifier: AGPL-3.0-or-later
"""Finite retained-peer wire observer. Bodies exist only while forwarding."""
import contextlib
import http.client
import http.server
import io
import os
import pathlib
import signal
import socket
import stat
import threading
import urllib.parse

import zstandard
from validation import Artifacts, DIGEST, decode, encoded, require, sha

TARGET=('127.0.0.1',8080)
LISTENER=('127.0.0.1',18080)
BODY_LIMIT=314572800
PATHS=frozenset('/sync/'+name for name in ('hostKey','meta','start','applyGraves','applyChanges','chunk','applyChunk','sanityCheck2','finish','abort','upload','download')) | frozenset('/msync/'+name for name in ('begin','mediaChanges','uploadChanges','downloadFiles','mediaSanity'))
WORKER_ACTIONS={
    'authentication':('authenticate',),
    'full-upload':('import_baseline','upload'),
    'full-download':('download',),
    'normal-no-change':('import_baseline','establish_baseline','settle_normal','normal_unchanged'),
    'normal-bidirectional':('import_baseline','establish_baseline','normal_send','normal_receive'),
    'media-bidirectional':('media_baseline','media_receive','media_send'),
    'media-no-change':('media_baseline','media_unchanged'),
    'manual-uncertain-recovery':('uncertain_upload',),
}
HOP=frozenset(('connection','proxy-connection','proxy-authorization','proxy-authenticate','keep-alive','te','trailer','transfer-encoding','upgrade','host','content-length'))


def public_file(path,limit=4194304):
    descriptor=os.open(path,os.O_RDONLY|os.O_NOFOLLOW)
    try:
        info=os.fstat(descriptor)
        require(stat.S_ISREG(info.st_mode) and info.st_nlink == 1 and info.st_size <= limit,'invalid retained input file')
        with os.fdopen(descriptor,'rb',closefd=False) as source:return source.read(limit+1)
    finally:os.close(descriptor)


def action_input(root):
    value=decode(public_file(root/'action.json'))
    require(isinstance(value,dict) and set(value) == {'case','action'} and all(type(item) is str for item in value.values()),'invalid retained action shape')
    return value


def route(method,target):
    require(method == 'POST','unsupported retained protocol method')
    parsed=urllib.parse.urlsplit(target)
    require(not parsed.fragment and not parsed.username and not parsed.password,'invalid retained proxy destination')
    if parsed.scheme or parsed.netloc:
        require(parsed.scheme == 'http' and parsed.netloc == '127.0.0.1:8080','foreign retained proxy destination')
    else:require(target.startswith('/') and not target.startswith('//'),'invalid retained protocol target')
    require(parsed.path in PATHS,'unsupported retained protocol route')
    return parsed.path,parsed.path+('?' + parsed.query if parsed.query else '')


def accepted_upload(body):
    if not body.startswith(b'\x28\xb5\x2f\xfd'):return False
    try:
        with zstandard.ZstdDecompressor().stream_reader(io.BytesIO(body),read_across_frames=True) as stream:
            return stream.read(3) == b'OK' and stream.read(1) == b''
    except Exception:return False


class WireServer(http.server.ThreadingHTTPServer):
    daemon_threads=False
    allow_reuse_address=True

    def __init__(self,case,action,actor,invocation_sha256,armed=False):
        self.case,self.action,self.actor,self.invocation_sha256=case,action,actor,invocation_sha256
        self.rows=[];self.lock=threading.Lock();self.upstream_complete=set();self.delivered_complete=set();self.failed=False;self.pending=0
        self.fault={'actor':'fault_driver','armed':armed,'accepted_upload_ack':False,'reply_dropped':False,'upload_request_count':0,'upload_replay_count':0}
        super().__init__(LISTENER,WireHandler)

    def handle_error(self,request,client_address):self.failed=True

    def begin(self,method,path):
        with self.lock:
            row={'sequence':len(self.rows)+1,'method':method,'path':path,'request_body_bytes':0,
                 'upstream_status':None,'upstream_body_bytes':0,'delivered_body_bytes':0,'disposition':'incomplete'}
            self.rows.append(row)
            if path == '/sync/upload':
                self.fault['upload_request_count']+=1
                self.fault['upload_replay_count']=max(0,self.fault['upload_request_count']-1)
            return row

    def wire(self):
        rows=sorted(self.rows,key=lambda row:row['sequence'])
        totals={'request_count':len(rows),'upstream_completed_count':len(self.upstream_complete),
            'delivered_completed_count':len(self.delivered_complete),
            'request_body_bytes':sum(row['request_body_bytes'] for row in rows),
            'upstream_body_bytes':sum(row['upstream_body_bytes'] for row in rows),
            'delivered_body_bytes':sum(row['delivered_body_bytes'] for row in rows),
            'dropped_body_bytes':sum(row['upstream_body_bytes']-row['delivered_body_bytes'] for row in rows)}
        return {'format':'meguri-retained-wire-v1','case':self.case,'action':self.action,'actor':self.actor,
            'invocation_sha256':self.invocation_sha256,'requests':rows,'totals':totals,'fault':dict(self.fault),'case_credit':False}


class WireHandler(http.server.BaseHTTPRequestHandler):
    protocol_version='HTTP/1.1'

    def setup(self):
        super().setup();self.connection.settimeout(45)
        self.pending=True
        with self.server.lock:self.server.pending+=1

    def finish(self):
        try:super().finish()
        finally:
            if self.pending:
                with self.server.lock:self.server.pending-=1

    def log_message(self,*_):pass

    def send_error(self,*_):
        self.server.failed=True;self.close_connection=True
        try:
            self.send_response(400);self.send_header('Content-Length','0');self.send_header('Connection','close');self.end_headers()
        except OSError:pass

    def chunks(self):
        transfer=self.headers.get_all('Transfer-Encoding',[]);length=self.headers.get_all('Content-Length',[])
        require(not transfer or not length,'ambiguous retained request framing')
        require(len(transfer) <= 1 and len(length) <= 1,'duplicate retained framing')
        total=0
        if transfer:
            require(transfer == ['chunked'],'unsupported retained transfer coding')
            while True:
                line=self.rfile.readline(256)
                require(line.endswith(b'\r\n') and len(line) < 256,'invalid retained chunk header')
                number=line[:-2].split(b';',1)[0]
                require(number and all(byte in b'0123456789abcdefABCDEF' for byte in number),'invalid retained chunk size')
                count=int(number,16)
                if count == 0:
                    require(self.rfile.readline(8192) == b'\r\n','retained trailers refused');break
                require(total+count <= BODY_LIMIT,'retained body limit')
                remaining=count
                while remaining:
                    data=self.rfile.read(min(65536,remaining));require(bool(data),'incomplete retained request')
                    total+=len(data);remaining-=len(data);yield data
                require(self.rfile.read(2) == b'\r\n','invalid retained chunk framing')
        else:
            require(not length or length[0].isdigit(),'invalid retained request length')
            remaining=int(length[0]) if length else 0
            require(remaining <= BODY_LIMIT,'retained body limit')
            while remaining:
                data=self.rfile.read(min(65536,remaining));require(bool(data),'incomplete retained request')
                remaining-=len(data);yield data

    def forward(self):
        row=None;upstream=None
        try:
            path,target=route(self.command,self.path)
            require(self.headers.get('Host') in {'127.0.0.1:8080','127.0.0.1:18080'},'foreign retained host')
            row=self.server.begin(self.command,path)
            with self.server.lock:self.server.pending-=1;self.pending=False
            upstream=http.client.HTTPConnection(*TARGET,timeout=45)
            upstream.putrequest(self.command,target,skip_host=True,skip_accept_encoding=True)
            upstream.putheader('Host','127.0.0.1:8080')
            connection_tokens={token.strip().lower() for token in self.headers.get('Connection','').split(',')}
            for key,value in self.headers.items():
                if key.lower() not in HOP and key.lower() not in connection_tokens:upstream.putheader(key,value)
            chunked=bool(self.headers.get('Transfer-Encoding'))
            upstream.putheader('Transfer-Encoding','chunked') if chunked else upstream.putheader('Content-Length',self.headers.get('Content-Length','0'))
            upstream.endheaders()
            for data in self.chunks():
                row['request_body_bytes']+=len(data)
                upstream.send((hex(len(data))[2:].encode()+b'\r\n'+data+b'\r\n') if chunked else data)
            if chunked:upstream.send(b'0\r\n\r\n')
            response=upstream.getresponse();row['upstream_status']=response.status
            pieces=[]
            while True:
                data=response.read1(65536)
                if not data:break
                row['upstream_body_bytes']+=len(data)
                require(row['upstream_body_bytes'] <= BODY_LIMIT,'retained response limit')
                pieces.append(data)
            require(response.length in {None,0},'incomplete retained response')
            body=b''.join(pieces)
            self.server.upstream_complete.add(row['sequence'])
            drop=False
            if self.server.fault['armed'] and path == '/sync/upload' and response.status == 200 and accepted_upload(body):
                with self.server.lock:
                    if not self.server.fault['reply_dropped']:
                        self.server.fault.update(accepted_upload_ack=True,reply_dropped=True);drop=True
            if drop:
                row['disposition']='upload_reply_dropped';self.close_connection=True
                self.connection.shutdown(socket.SHUT_RDWR)
                return
            self.send_response_only(response.status)
            response_connection={token.strip().lower() for token in response.getheader('Connection','').split(',')}
            for key,value in response.getheaders():
                if key.lower() not in HOP and key.lower() not in response_connection:self.send_header(key,value)
            self.send_header('Content-Length',str(len(body)));self.send_header('Connection','close');self.end_headers()
            view=memoryview(body)
            while view:
                count=self.connection.send(view[:65536]);require(count > 0,'incomplete retained delivery')
                row['delivered_body_bytes']+=count;view=view[count:]
            self.server.delivered_complete.add(row['sequence']);row['disposition']='forwarded'
        except BaseException:
            self.server.failed=True
            if row is None:self.send_error(400)
        finally:
            self.close_connection=True
            if upstream is not None:upstream.close()

    do_POST=forward


@contextlib.contextmanager
def observing(case,action,actor,invocation_sha256,armed=False):
    server=WireServer(case,action,actor,invocation_sha256,armed)
    thread=threading.Thread(target=server.serve_forever);thread.start()
    try:yield server
    finally:
        server.shutdown();server.server_close();thread.join()


def verify_worker_input(root,action,invocation):
    from candidate import retained_dependencies
    require(not root.is_symlink() and not (root/'artifacts').is_symlink(),'linked retained observer inputs')
    required={'action.json','invocation.json','preflight.json','artifacts'}
    names={path.name for path in root.iterdir()}
    require(required <= names <= required|{'client-build.json','input-manifest.json'},'invalid retained observer input layout')
    keys={'format','run_id','root','server_arguments','data_volume','owner_nonce','case','action','actor',
          'client_build_sha256','input_manifest_sha256','preflight_peer_sha256'}
    require(isinstance(invocation,dict) and set(invocation) == keys and invocation['format'] == 'meguri-retained-invocation-v1','invalid retained worker invocation')
    require(invocation['actor'] == 'client_a' and all(invocation[key] == action[key] for key in ('case','action')),'retained observer action identity mismatch')
    require(invocation['run_id'] == os.environ.get('MG_RUN_ID'),'retained observer run identity mismatch')
    preflight_bytes=public_file(root/'preflight.json')
    require(sha(preflight_bytes) == invocation['preflight_peer_sha256'],'retained observer preflight bytes changed')
    preflight=decode(preflight_bytes);store=Artifacts(root/'artifacts')
    outer={key:invocation[key] for key in ('run_id','root','server_arguments','data_volume','owner_nonce')}
    outer.update({key:preflight['server'][key] for key in ('source_commit','source_sha256','lock_sha256','executable_sha256')})
    retained_dependencies(preflight,store,outer,preflight=True)
    for key in ('client_build_sha256','input_manifest_sha256'):store.read(invocation[key])


def main(arguments):
    require(arguments == ['--input','/in'],'fixed retained observer paths required')
    root=pathlib.Path('/in');out=pathlib.Path('/out')
    require(not list(out.iterdir()),'retained observer output already populated')
    action=action_input(root)
    require(action['action'] in WORKER_ACTIONS.get(action['case'],()),'unknown retained observer action')
    invocation_bytes=public_file(root/'invocation.json');invocation=decode(invocation_bytes)
    verify_worker_input(root,action,invocation)
    armed=(action['case'],action['action']) == ('manual-uncertain-recovery','uncertain_upload')
    server=WireServer(action['case'],action['action'],'client_a',sha(invocation_bytes),armed)
    try:
        with socket.create_connection(TARGET,timeout=5):pass
    except BaseException:
        server.server_close();raise
    previous={}
    def stop(*_):
        with server.lock:
            if server.pending or any(row['disposition'] == 'incomplete' for row in server.rows):server.failed=True
        threading.Thread(target=server.shutdown).start()
    for number in (signal.SIGTERM,signal.SIGINT):previous[number]=signal.signal(number,stop)
    try:
        (out/'ready.json').write_bytes(encoded({'format':'meguri-retained-observer-ready-v1',**action,
            'run_id':invocation['run_id'],'invocation_sha256':sha(invocation_bytes)}))
        server.serve_forever()
    finally:
        server.server_close()
        for number,handler in previous.items():signal.signal(number,handler)
        (out/'wire.json').write_bytes(encoded(server.wire()))
    return 1 if server.failed or not server.rows or any(row['disposition'] == 'incomplete' for row in server.rows) else 0
