# SPDX-License-Identifier: AGPL-3.0-or-later
"""SDK adapter. Its stderr and any non-JSON stdout stay in quarantine."""
from __future__ import annotations

import contextlib
import hashlib
import http.client
import http.server
import importlib.metadata
import io
import json
import os
import pathlib
import secrets
import socket
import subprocess
import sys
import tempfile
import threading
import time

import anki
import anki.buildinfo
from anki.collection import Collection
from anki.errors import NetworkError, SyncError
from anki.sync import SyncAuth, SyncOutput
import zstandard

from validation import encoded, require, sha, same_value
from synthetic import FIXTURE, EDGE_FIXTURE

PHASE = "identity"
RECORDER_ERRORS = []


def runtime():
    global PHASE
    PHASE = 'runtime-location'
    folder = pathlib.Path(sys.modules['anki.collection'].__file__).parent
    libraries = list(folder.glob("_rsbridge*.so"))
    require(len(libraries) == 1, "reference native library identity unavailable")
    info = anki.buildinfo
    PHASE = 'runtime-version-build'
    version = str(getattr(info, "version", ""))
    build = str(getattr(info, "buildhash", ""))
    require(version in {"26.09.4", "26.9.4"} and bool(build), "unexpected reference release/build")
    digest = sha(libraries[0].read_bytes())
    PHASE = 'runtime-installation-receipt'
    installed = json.loads(pathlib.Path("/opt/reference/install-report.json").read_bytes())
    packages = []
    wheel_hash = None
    for entry in installed["install"]:
        meta = entry["metadata"]
        artifact = entry["download_info"]["archive_info"]["hashes"]["sha256"]
        packages.append({"name": meta["name"], "version": meta["version"], "sha256": artifact})
        if meta["name"].lower() == "anki":
            wheel_hash = artifact
    require(wheel_hash in {"52d319123a06d831fbc47cccfba087d75e00dc2e7eb4176c19a2901c525aad9e",
                           "67950105a13fe9894fe424eb799afe01b2a1762f402bf877aaa5b3cb879ec8cc"}, "unapproved reference wheel")
    return {"format": "meguri-sync-runtime-v1",
            "client": {"kind": "desktop", "version": "26.09.4", "build": build, "runtime_sha256": digest},
            "server": {"kind": "reference_runtime", "version": "26.09.4", "build": build, "runtime_sha256": digest},
            "wheel_sha256": wheel_hash, "packages": sorted(packages, key=lambda p: p["name"].lower()),
            "lock_sha256": sha(pathlib.Path('/opt/reference/requirements.lock').read_bytes())}


def free_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


@contextlib.contextmanager
def reference_server(root):
    port = free_port()
    username, password = "synthetic-" + secrets.token_hex(8), secrets.token_hex(24)
    env = {key: value for key, value in os.environ.items() if not key.startswith("SYNC_") and "proxy" not in key.lower()}
    env.update(SYNC_BASE=str(root), SYNC_HOST="127.0.0.1", SYNC_PORT=str(port), SYNC_USER1=f"{username}:{password}")
    process = subprocess.Popen([sys.executable, "-m", "anki.syncserver"], env=env,
                               stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        deadline = time.monotonic() + 20
        while True:
            require(process.poll() is None, "reference server exited before readiness")
            try:
                with socket.create_connection(("127.0.0.1", port), 0.2): break
            except OSError:
                require(time.monotonic() < deadline, "reference server readiness deadline")
                time.sleep(0.05)
        yield port, username, password
    finally:
        process.terminate()
        try: process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()


def unpack(data):
    if not data:
        return b""
    require(data.startswith(b'\x28\xb5\x2f\xfd'), "actual request is not raw zstd")
    with zstandard.ZstdDecompressor().stream_reader(io.BytesIO(data), read_across_frames=True) as stream:
        result = stream.read(314572801)
    require(len(result) <= 314572800, "reference decompression bound")
    return result


def file_stamp(data, matrix=False):
    import sqlite3
    with tempfile.NamedTemporaryFile(suffix='.anki2') as stream:
        stream.write(data); stream.flush()
        connection = sqlite3.connect(stream.name)
        connection.create_collation('unicase', lambda left, right: (left.casefold() > right.casefold()) - (left.casefold() < right.casefold()))
        try:
            names = ['mod', 'scm', 'usn', 'ls', 'crt', 'ver']
            stamp = dict(zip(names, connection.execute('select mod,scm,usn,ls,crt,ver from col').fetchone()))
            connection.execute('pragma trusted_schema=off')
            content = {}
            tables = connection.execute("select name from sqlite_master where type='table' and name <> 'col' and name not glob 'sqlite_*' order by name").fetchall()
            for (table,) in tables:
                rows = connection.execute('select * from "' + table.replace('"', '""') + '"').fetchall()
                normalized = [[{'blob': value.hex()} if isinstance(value, bytes) else value for value in row] for row in rows]
                content[table] = sorted(normalized, key=encoded)
            stamp['user_tables_sha256'] = sha(encoded(content))
            if matrix:
                from normal_matrix import database_state
                stamp['matrix_state'] = database_state(connection)
            return stamp
        finally: connection.close()


class Recorder(http.server.ThreadingHTTPServer):
    daemon_threads = True
    def __init__(self, upstream):
        self.upstream = upstream
        self.observations = []
        self.capture_normal = False
        self.capture_media = False
        self.errors = []
        self.empty_sk = False
        self.rewrite_session = None
        self.active_fault_session = None
        self.replay_uploads = False
        self.idle_seconds = 0
        self.expire_after_idle = False
        self.idle_claimed = False
        self.idle_completed = threading.Event()
        super().__init__(("127.0.0.1", 0), RecorderHandler)


class RecorderHandler(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    def log_message(self, *_): pass

    def _body(self):
        if self.headers.get("Transfer-Encoding", "").lower() == "chunked":
            output = bytearray()
            while True:
                line = self.rfile.readline(256)
                size = int(line.split(b";", 1)[0].strip(), 16)
                if not size:
                    require(self.rfile.readline(8192) == b"\r\n", "unexpected trailer")
                    return bytes(output)
                require(len(output) + size <= 104857600, "recording compressed bound")
                output.extend(self.rfile.read(size))
                require(self.rfile.read(2) == b"\r\n", "malformed chunk framing")
        length = int(self.headers.get("Content-Length", "0"))
        require(0 <= length <= 104857600, "recording compressed bound")
        return self.rfile.read(length)

    def do_POST(self):
        try:
            raw = self._body()
            header = json.loads(self.headers["anki-sync"])
            decoded = unpack(raw)
            detail = {"method": "POST", "route": self.path, "protocol": header["v"],
                      "header_members": sorted(header), "compressed_request_bytes": len(raw),
                      "request_bytes": len(decoded), "raw_zstd": True,
                      "framing": "chunked" if self.headers.get("Transfer-Encoding") else "content-length"}
            if self.server.capture_normal and self.path in {'/sync/start','/sync/applyGraves','/sync/applyChanges','/sync/chunk','/sync/applyChunk','/sync/sanityCheck2','/sync/finish'}:
                detail['normal_request'] = json.loads(decoded)
            if self.path.endswith(("/chunk", "/finish", "/abort", "/download")):
                detail["no_argument_body"] = json.loads(decoded)
            if self.path.startswith('/msync/') and not self.path.endswith('/uploadChanges'):
                detail['media_request_members'] = sorted(json.loads(decoded))
                if self.server.capture_media:
                    detail['media_request'] = json.loads(decoded)
            if self.path.endswith("/upload"):
                detail['upload_stamp'] = file_stamp(decoded)
            if self.path == '/sync/chunk' and self.server.idle_seconds and not self.server.idle_claimed:
                self.server.idle_claimed = True
                started = time.monotonic()
                time.sleep(self.server.idle_seconds)
                detail['idle_elapsed_ms'] = int((time.monotonic() - started) * 1000)
                detail['request_mutation'] = 'idle-expire' if self.server.expire_after_idle else 'idle-delay'
                if self.server.expire_after_idle:
                    temporary_auth = SyncAuth(hkey=header['k'], endpoint=f'http://127.0.0.1:{self.server.upstream}/')
                    detail['injected_abort_status'] = native_request(self.server.upstream, temporary_auth, 'abort', {'_pad':None}, header['s'])['status']
            connection = http.client.HTTPConnection("127.0.0.1", self.server.upstream, timeout=30)
            headers = {key: value for key, value in self.headers.items() if key.lower() not in {"host", "connection", "content-length", "transfer-encoding"}}
            if self.server.rewrite_session is not None:
                header['s'] = self.server.rewrite_session
                for key in list(headers):
                    if key.lower() == 'anki-sync': headers[key] = json.dumps(header)
                detail['session_rewritten_by_fault_driver'] = True
            if self.server.active_fault_session is not None and self.path in {'/sync/upload','/sync/download'}:
                detail['session_matches_fault_round'] = header['s'] == self.server.active_fault_session
            connection.request("POST", self.path, body=raw, headers=headers)
            response = connection.getresponse()
            output = response.read()
            if self.server.replay_uploads and self.path == '/msync/uploadChanges' and response.status == 200:
                repeated = http.client.HTTPConnection('127.0.0.1', self.server.upstream, timeout=30)
                repeated.request('POST', self.path, body=raw, headers=headers)
                repeated_response = repeated.getresponse()
                repeated_output = repeated_response.read()
                detail['request_mutation'] = 'replay-upload'
                detail['replay_status'] = repeated_response.status
                detail['replay_media_result'] = json.loads(unpack(repeated_output))['data'] if repeated_response.status == 200 else None
                repeated.close()
            changed_headers = {}
            if self.server.empty_sk and self.path == '/msync/begin' and response.status == 200:
                media = json.loads(unpack(output))
                media['data']['sk'] = ''
                plain = encoded(media)
                output = zstandard.ZstdCompressor().compress(plain)
                changed_headers['anki-original-size'] = str(len(plain))
                detail['response_mutation'] = 'empty-sk'
            detail.update(status=response.status, compressed_response_bytes=len(output))
            if 200 <= response.status < 300:
                body = unpack(output)
                detail["response_bytes"] = len(body)
                if self.server.capture_normal and self.path in {'/sync/start','/sync/applyGraves','/sync/applyChanges','/sync/chunk','/sync/applyChunk','/sync/sanityCheck2','/sync/finish'}:
                    detail['normal_response'] = json.loads(body)
                detail["original_size_matches"] = changed_headers.get('anki-original-size', response.getheader("anki-original-size")) == str(len(body))
                if self.path.endswith('/download'):
                    detail['download_stamp'] = file_stamp(body,getattr(self.server,'capture_matrix',False))
                elif self.path.endswith('/meta'):
                    meta = json.loads(body)
                    detail['meta'] = {key: meta[key] for key in ('mod','scm','usn','ts','cont','empty','media_usn','hostNum')}
                elif self.path.endswith('/upload'):
                    detail['upload_result'] = body.decode('utf-8')
                elif self.path.startswith('/msync/') and not self.path.endswith('/downloadFiles'):
                    media = json.loads(body)
                    detail['media_envelope_members'] = sorted(media)
                    detail['media_error_empty'] = media.get('err') == ''
                    if self.path.endswith('/begin'):
                        detail['media_begin'] = {'usn': media['data']['usn'], 'sk_type': type(media['data']['sk']).__name__, 'sk_empty': media['data']['sk'] == ''}
                    else:
                        detail['media_result'] = media['data']
            self.server.observations.append(detail)
            if detail.get('request_mutation') in {'idle-delay','idle-expire'}:
                self.server.idle_completed.set()
            self.send_response(response.status)
            for key, value in response.getheaders():
                if key.lower() not in {"connection", "content-length", "transfer-encoding"}:
                    self.send_header(key, changed_headers.get(key.lower(), value))
            self.send_header("Content-Length", str(len(output)))
            self.end_headers()
            self.wfile.write(output)
            connection.close()
        except Exception as error:
            self.server.errors.append(type(error).__name__)
            RECORDER_ERRORS.append(type(error).__name__)
            self.close_connection = True


@contextlib.contextmanager
def recorder(port):
    server = Recorder(port)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try: yield server
    finally:
        server.shutdown(); server.server_close(); thread.join()


def collection(root, name, fixture):
    directory = root / name
    directory.mkdir()
    col = Collection(str(directory / "collection.anki2"))
    col.db.execute("pragma secure_delete=on")
    model = col.models.new(fixture["notetype"])
    model.update(css="", latexPre="", latexPost="")
    for field in fixture["fields"]:
        col.models.add_field(model, col.models.new_field(field))
    template = col.models.new_template(fixture["template"]["name"])
    template.update(qfmt=fixture["template"]["question"], afmt=fixture["template"]["answer"])
    col.models.add_template(model, template)
    col.models.add(model)
    for other in col.models.all_names_and_ids():
        if other.id != model["id"]: col.models.remove(other.id)
    return col, model


def add_note(col, model, values):
    note = col.new_note(model)
    note.fields = values
    col.add_note(note, 1)
    return note.id


def full(col, auth, upload):
    col.close_for_full_sync()
    try: col.full_upload_or_download(auth=auth, server_usn=None, upload=upload)
    finally: col.reopen(after_full_sync=True)


def snapshot(col):
    return {"notes": col.db.all("select id,guid,mid,mod,tags,flds,flags,data from notes order by id"),
            "cards": col.db.all("select id,nid,did,ord,mod,type,queue,due,ivl,factor,reps,lapses,left,odue,odid,flags,data from cards order by id"),
            "reviews": col.db.all("select * from revlog order by id")}


def media_sync(col, auth, timeout=30):
    col.sync_media(auth)
    deadline = time.monotonic() + timeout
    while col.media_sync_status().active:
        if time.monotonic() >= deadline:
            col.abort_media_sync()
            raise TimeoutError('media sync deadline')
        time.sleep(0.01)


def native_request(port, auth, method, body, session, retain_input=False):
    """Job-owned fault driver; never claims its bytes were emitted by Desktop."""
    payload = zstandard.ZstdCompressor().compress(encoded(body))
    header = json.dumps({'v': 11, 'k': auth.hkey, 'c': 'meguri-reference-fault-driver', 's': session})
    connection = http.client.HTTPConnection('127.0.0.1', port, timeout=15)
    connection.request('POST', '/sync/' + method, body=payload, headers={'anki-sync': header})
    response = connection.getresponse()
    output = response.read()
    result = {'status': response.status, 'response_bytes': len(output)}
    if retain_input:
        result.update(input_json_hex=encoded(body).hex(),input_zstd_hex=payload.hex(),response_zstd_hex=output.hex())
    if 200 <= response.status < 300:
        result['value'] = json.loads(unpack(output))
    connection.close()
    return result


def noop_probe(root, fixture):
    with reference_server(root / 'server') as (port, username, password), recorder(port) as record:
        clients = []
        try:
            a, model = collection(root, 'a', fixture['base']); clients.append(a)
            add_note(a, model, fixture['base']['notes'][0])
            b, _ = collection(root, 'b', fixture['base']); clients.append(b)
            endpoint = f'http://127.0.0.1:{record.server_port}/'
            auth_a = a.sync_login(username=username,password=password,endpoint=endpoint)
            auth_b = b.sync_login(username=username,password=password,endpoint=endpoint)
            full(a,auth_a,True); full(b,auth_b,False)
            record.replay_uploads = True
            a.media.write_data(fixture['repeat_name'],fixture['repeat_contents'].encode())
            media_sync(a,auth_a); media_sync(b,auth_b)
            received_hash = sha((pathlib.Path(b.media.dir()) / fixture['repeat_name']).read_bytes())
            time.sleep(1.05)
            (pathlib.Path(a.media.dir()) / fixture['repeat_name']).unlink()
            media_sync(a,auth_a); media_sync(b,auth_b)
            deleted = not (pathlib.Path(b.media.dir()) / fixture['repeat_name']).exists()
            require(not record.errors, 'replay recorder incomplete')
            return {'traffic':record.observations,'received_hash':received_hash,'receiver_deleted':deleted}
        finally:
            for col in clients: col.close(downgrade=False)


def pagination_probe(root, fixture):
    with reference_server(root / 'server') as (port, username, password), recorder(port) as record:
        clients = []
        try:
            a, model = collection(root,'a',fixture['base']); clients.append(a)
            add_note(a,model,fixture['base']['notes'][0])
            endpoint = f'http://127.0.0.1:{record.server_port}/'
            auth_a = a.sync_login(username=username,password=password,endpoint=endpoint)
            full(a,auth_a,True)
            previous = 0
            results = []
            for count in fixture['page_counts']:
                if count > previous:
                    time.sleep(1.05)
                    for index in range(previous,count):
                        name = f"{fixture['media_prefix']}-{index:04}.bin"
                        a.media.write_data(name,(fixture['media_payload_prefix']+str(index)).encode())
                    media_sync(a,auth_a,timeout=120)
                b, _ = collection(root,'receiver-'+str(count),fixture['base']); clients.append(b)
                auth_b = b.sync_login(username=username,password=password,endpoint=endpoint)
                full(b,auth_b,False)
                before = len(record.observations)
                media_sync(b,auth_b,timeout=120)
                traffic = record.observations[before:]
                pages = [row['media_result'] for row in traffic if row['route'] == '/msync/mediaChanges']
                hashes = {f"{fixture['media_prefix']}-{index:04}.bin":sha((pathlib.Path(b.media.dir()) / f"{fixture['media_prefix']}-{index:04}.bin").read_bytes()) for index in range(count)}
                results.append({'count':count,'pages':pages,'file_hashes':hashes,'traffic':traffic})
                previous = count
            require(not record.errors, 'pagination recorder incomplete')
            return results
        finally:
            for col in clients: col.close(downgrade=False)


def idle_probe(root, fixture, expire):
    with reference_server(root / 'server') as (port,username,password), recorder(port) as record:
        clients = []
        try:
            a,model = collection(root,'a',fixture['base']); clients.append(a)
            add_note(a,model,fixture['base']['notes'][0])
            b,_ = collection(root,'b',fixture['base']); clients.append(b)
            endpoint = f'http://127.0.0.1:{record.server_port}/'
            login = a.sync_login(username=username,password=password,endpoint=endpoint)
            auth_a = SyncAuth(hkey=login.hkey,endpoint=endpoint,io_timeout_secs=fixture['sdk_timeout_seconds'])
            auth_b = b.sync_login(username=username,password=password,endpoint=endpoint)
            full(a,auth_a,True); full(b,auth_b,False)
            original = snapshot(a)
            time.sleep(1.05)
            note = a.get_note(a.find_notes('')[0]); note.fields[1] = fixture['idle_edit']; a.update_note(note)
            before = snapshot(a)
            record.idle_seconds = fixture['idle_seconds']
            record.expire_after_idle = expire
            outcome = {'requested_idle_seconds':fixture['idle_seconds'],'explicit_expiry_injected':expire}
            try:
                reply = a.sync_collection(auth_a,sync_media=False)
                outcome['client_result'] = 'no_changes' if reply.required == SyncOutput.NO_CHANGES else 'full_transfer_required'
            except (NetworkError, SyncError) as error:
                outcome['client_result'] = 'error'
                outcome['client_error_kind'] = type(error).__name__
            require(record.idle_completed.wait(timeout=fixture['idle_seconds']+10),'delayed SDK request did not complete')
            outcome['local_content_unchanged'] = snapshot(a) == before
            full(b,auth_b,False)
            outcome['server_pending_edit_rolled_back'] = snapshot(b) == original
            outcome['receiver_matches_sender'] = snapshot(b) == snapshot(a)
            require(not record.errors,'idle recorder incomplete')
            outcome['traffic'] = record.observations
            return outcome
        finally:
            for col in clients: col.close(downgrade=False)


def edges(fixture):
    global PHASE
    require(same_value(fixture,EDGE_FIXTURE),'unknown edge fixture')
    identity = runtime()
    with tempfile.TemporaryDirectory(prefix='current-reference-edges-') as directory:
        root = pathlib.Path(directory)
        for name in ('noops','pages','idle','expiry'): (root / name).mkdir()
        PHASE = 'media-noop-replays'
        noops = noop_probe(root / 'noops',fixture)
        PHASE = 'media-pagination'
        pages = pagination_probe(root / 'pages',fixture)
        PHASE = 'reference-idle-boundary'
        idle = idle_probe(root / 'idle',fixture,False)
        PHASE = 'injected-expiry-boundary'
        expiry = idle_probe(root / 'expiry',fixture,True)
        return {'probe_kind':'edges','runtime':identity,'fixture':fixture,'noops':noops,'pagination':pages,'idle':idle,'expiry':expiry}


def session_probes(port, a, b, auth_a, auth_b, record):
    start = {'minUsn': a.db.scalar('select usn from col'), 'lnewer': False, 'graves': None}
    session_a, session_b = secrets.token_hex(8), secrets.token_hex(8)
    result = {}
    result['first_start'] = native_request(port, auth_a, 'start', start, session_a)
    result['same_session_start'] = native_request(port, auth_a, 'start', start, session_a)
    result['other_session_start'] = native_request(port, auth_a, 'start', start, session_b)
    result['displaced_chunk'] = native_request(port, auth_a, 'chunk', {'_pad': None}, session_a)
    result['owning_abort'] = native_request(port, auth_a, 'abort', {'_pad': None}, session_b)
    result['absent_abort'] = native_request(port, auth_a, 'abort', {'_pad': None}, session_b)
    for direction in ('download', 'upload'):
        for same_session in (False, True):
            name = direction + ('_same_session' if same_session else '_other_session')
            session = secrets.token_hex(8)
            row = {'start': native_request(port, auth_a, 'start', start, session)}
            row['pending_grave'] = native_request(port, auth_a, 'applyGraves', {'chunk': {'cards': [FIXTURE['pending_grave_id']], 'notes': [], 'decks': []}}, session)
            record.rewrite_session = session if same_session else None
            record.active_fault_session = session
            before_transfer = len(record.observations)
            try:
                receiver = b if direction == 'download' else a
                full(receiver, auth_b if direction == 'download' else auth_a, direction == 'upload')
                row['full_transfer'] = 'ok'
                row['receiver_graves'] = receiver.db.scalar('select count(*) from graves')
            except Exception as error:
                row['full_transfer'] = type(error).__name__
            finally:
                record.rewrite_session = None
                record.active_fault_session = None
            row['full_transfer_sessions'] = [entry['session_matches_fault_round'] for entry in record.observations[before_transfer:]
                                             if entry['route'] == '/sync/'+direction and 'session_matches_fault_round' in entry]
            row['stale_finish'] = native_request(port, auth_a, 'finish', {'_pad': None}, session)
            result[name] = row
    return result


@contextlib.contextmanager
def probe_server(root, target):
    if target is None:
        with reference_server(root) as server: yield server
    else:
        require(set(target) == {'username','password','server'}, 'invalid candidate adapter inputs')
        yield 8080, target['username'], target['password']


def smoke(fixture, target=None, full_only=False):
    global PHASE
    require(same_value(fixture,FIXTURE), "unknown synthetic fixture")
    identity = runtime()
    if target is not None: identity['server'] = target['server']
    with tempfile.TemporaryDirectory(prefix="current-reference-") as scratch:
        root = pathlib.Path(scratch)
        PHASE = 'reference-server-start'
        with probe_server(root / "server",target) as (port, username, password), recorder(port) as record:
            clients = []
            try:
                PHASE = 'synthetic-client-setup'
                a, model = collection(root, "a", fixture); clients.append(a)
                add_note(a, model, fixture["notes"][0])
                b, _ = collection(root, "b", fixture); clients.append(b)
                endpoint = f"http://127.0.0.1:{record.server_port}/"
                PHASE = 'native-login'
                auth_a = a.sync_login(username=username, password=password, endpoint=endpoint)
                auth_b = b.sync_login(username=username, password=password, endpoint=endpoint)
                initial = a.sync_collection(auth_a, sync_media=False)
                initial_requirement = {SyncOutput.FULL_UPLOAD: 'full_upload', SyncOutput.FULL_DOWNLOAD: 'full_download',
                                       SyncOutput.FULL_SYNC: 'full_sync', SyncOutput.NO_CHANGES: 'no_changes'}[initial.required]
                PHASE = 'blank-account-download'
                blank, _ = collection(root, 'blank', fixture); clients.append(blank)
                auth_blank = blank.sync_login(username=username, password=password, endpoint=endpoint)
                full(blank, auth_blank, False)
                blank_counts = [blank.db.scalar('select count(*) from notes'), blank.db.scalar('select count(*) from cards')]
                PHASE = 'full-upload'
                full(a, auth_a, True)
                PHASE = 'full-download'
                full(b, auth_b, False)
                integrity = b.db.scalar('pragma integrity_check')
                full_equal = snapshot(a) == snapshot(b)
                full(b, auth_b, False)
                full_equal = full_equal and snapshot(a) == snapshot(b)
                if full_only:
                    require(not record.errors,'recording is incomplete')
                    return {'probe_kind':'candidate' if target is not None else 'reference_full','phase':'full','full_only':True,'runtime':identity,'fixture':fixture,'traffic':record.observations,
                            'outcomes':{'full_equal':full_equal,'integrity':integrity,'initial_requirement':initial_requirement,'blank_counts':blank_counts}}
                note = a.get_note(a.find_notes("")[0])
                time.sleep(1.05)
                note.fields = fixture["edit"]
                a.update_note(note)
                PHASE = 'normal-collection-sync'
                a.sync_collection(auth_a, sync_media=False)
                b.sync_collection(auth_b, sync_media=False)
                normal_equal = snapshot(a) == snapshot(b)
                a.sync_collection(auth_a, sync_media=False)
                before = len(record.observations)
                a.sync_collection(auth_a, sync_media=False)
                no_change = list(record.observations[before:])
                a.media.write_data("aster.bin", fixture["media"]["aster.bin"].encode())
                PHASE = 'forward-media-sync'
                media_sync(a, auth_a)
                media_sync(b, auth_b)
                media_forward = (pathlib.Path(b.media.dir()) / "aster.bin").read_bytes() == fixture["media"]["aster.bin"].encode()
                time.sleep(1.05)
                b.media.write_data("birch.bin", fixture["media"]["birch.bin"].encode())
                PHASE = 'reverse-media-sync'
                media_sync(b, auth_b)
                media_sync(a, auth_a)
                media_reverse = (pathlib.Path(a.media.dir()) / "birch.bin").read_bytes() == fixture["media"]["birch.bin"].encode()
                media_hashes = {name: sha((pathlib.Path(a.media.dir()) / name).read_bytes()) for name in fixture["media"]}
                before_media = len(record.observations)
                media_sync(a, auth_a)
                no_change_media = record.observations[before_media:]
                PHASE = 'fresh-receiver-empty-sk'
                record.empty_sk = True
                full(blank, auth_blank, False)
                media_sync(blank, auth_blank)
                fresh_media_hashes = {name: sha((pathlib.Path(blank.media.dir()) / name).read_bytes()) for name in fixture['media']}
                record.empty_sk = False
                PHASE = 'session-admission'
                admission = session_probes(port, a, b, auth_a, auth_b, record)
                require(not record.errors, "recording is incomplete")
                return {"runtime": identity, "fixture": fixture, "traffic": record.observations,
                        'probe_kind':'candidate' if target is not None else 'smoke','phase':'complete',
                        'auxiliary_scope':'Raw-only session-admission observations carry no Desktop case credit.',
                        'session_admission': admission,
                        "outcomes": {"full_equal": full_equal, "integrity": integrity, "normal_equal": normal_equal,
                                     'initial_requirement': initial_requirement, 'blank_counts': blank_counts,
                                     'fresh_media_hashes': fresh_media_hashes,
                                     'no_change_media_routes': [row['route'] for row in no_change_media],
                                     "no_change_routes": [row["route"] for row in no_change],
                                     "media_forward": media_forward, "media_reverse": media_reverse,
                                     "media_hashes": media_hashes}}
            finally:
                for col in clients: col.close(downgrade=False)


if __name__ == "__main__":
    try:
        request = json.loads(sys.stdin.buffer.read())
        if request['mode'] == 'identity': result = runtime()
        elif request['mode'] == 'edges': result = edges(request['fixture'])
        elif request['mode'] == 'normal-matrix':
            import normal_matrix
            result = normal_matrix.run(sys.modules[__name__],request['fixture'])
        elif request['mode'] == 'media':
            import media
            result = media.run(sys.modules[__name__],request['fixture'])
        elif request['mode'] in {'normal','normal-sdk'}:
            import normal
            result = normal.exact_numbers(normal.run(sys.modules[__name__],request['fixture'],sdk_only=request['mode'] == 'normal-sdk'))
        elif request['mode'] == 'candidate':
            require(isinstance(request.get('target'),dict),'candidate target missing')
            require(request['target'].get('server',{}).get('kind') == 'meguri_sync','candidate peer kind mismatch')
            require(type(request.get('full_only')) is bool,'candidate phase is malformed')
            require(request.get('phase') in {'full','normal','media','complete'},'candidate phase is unknown')
            if request['phase'] == 'media':
                import media
                result = media.run(sys.modules[__name__],request['fixture'],request['target'])
            elif request['phase'] == 'normal':
                import normal
                result = normal.exact_numbers(normal.run(sys.modules[__name__],request['fixture'],request['target'],sdk_only=True))
            else:
                result = smoke(request['fixture'],request['target'],request['full_only'])
        elif request['mode'] == 'smoke':
            require(type(request.get('full_only',False)) is bool,'reference phase is malformed')
            result = smoke(request['fixture'],full_only=request.get('full_only',False))
        else: raise ValueError('unknown SDK request mode')
        sys.stdout.buffer.write(encoded(result))
    except BaseException as error:
        # Never export source frames, backend messages or collection content.
        sys.stdout.buffer.write(encoded({"failure": {"kind": type(error).__name__, "phase": PHASE, "recording_errors": RECORDER_ERRORS, "driver_frames": [str(pathlib.Path(frame.filename).name)+":"+str(frame.lineno) for frame in __import__("traceback").extract_tb(error.__traceback__) if frame.filename.startswith("/opt/reference/")]}}))
        sys.stdout.buffer.flush()
        os._exit(1)
