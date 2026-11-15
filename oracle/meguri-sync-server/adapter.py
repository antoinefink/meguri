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
from anki.sync import SyncAuth, SyncOutput
import zstandard

from validation import encoded, require, sha
from synthetic import FIXTURE

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


def file_stamp(data):
    import sqlite3
    with tempfile.NamedTemporaryFile(suffix='.anki2') as stream:
        stream.write(data); stream.flush()
        connection = sqlite3.connect(stream.name)
        try:
            names = ['mod', 'scm', 'usn', 'ls', 'crt', 'ver']
            return dict(zip(names, connection.execute('select mod,scm,usn,ls,crt,ver from col').fetchone()))
        finally: connection.close()


class Recorder(http.server.ThreadingHTTPServer):
    daemon_threads = True
    def __init__(self, upstream):
        self.upstream = upstream
        self.observations = []
        self.errors = []
        self.empty_sk = False
        self.rewrite_session = None
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
            if self.path.endswith(("/chunk", "/finish", "/abort", "/download")):
                detail["no_argument_body"] = json.loads(decoded)
            if self.path.startswith('/msync/') and not self.path.endswith('/uploadChanges'):
                detail['media_request_members'] = sorted(json.loads(decoded))
            if self.path.endswith("/upload"):
                detail['upload_stamp'] = file_stamp(decoded)
            connection = http.client.HTTPConnection("127.0.0.1", self.server.upstream, timeout=30)
            headers = {key: value for key, value in self.headers.items() if key.lower() not in {"host", "connection", "content-length", "transfer-encoding"}}
            if self.server.rewrite_session is not None:
                header['s'] = self.server.rewrite_session
                for key in list(headers):
                    if key.lower() == 'anki-sync': headers[key] = json.dumps(header)
                detail['session_rewritten_by_fault_driver'] = True
            connection.request("POST", self.path, body=raw, headers=headers)
            response = connection.getresponse()
            output = response.read()
            changed_headers = {}
            if self.server.empty_sk and self.path == '/msync/begin' and response.status == 200:
                media = json.loads(unpack(output))
                media['data']['sk'] = ''
                plain = encoded(media)
                output = zstandard.ZstdCompressor().compress(plain)
                changed_headers['anki-original-size'] = str(len(plain))
            detail.update(status=response.status, compressed_response_bytes=len(output))
            if 200 <= response.status < 300:
                body = unpack(output)
                detail["response_bytes"] = len(body)
                detail["original_size_matches"] = changed_headers.get('anki-original-size', response.getheader("anki-original-size")) == str(len(body))
                if self.path.endswith('/download'):
                    detail['download_stamp'] = file_stamp(body)
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


def media_sync(col, auth):
    col.sync_media(auth)
    deadline = time.monotonic() + 30
    while col.media_sync_status().active:
        if time.monotonic() >= deadline:
            col.abort_media_sync()
            raise TimeoutError('media sync deadline')
        time.sleep(0.01)


def native_request(port, auth, method, body, session):
    """Job-owned fault driver; never claims its bytes were emitted by Desktop."""
    payload = zstandard.ZstdCompressor().compress(encoded(body))
    header = json.dumps({'v': 11, 'k': auth.hkey, 'c': 'meguri-reference-fault-driver', 's': session})
    connection = http.client.HTTPConnection('127.0.0.1', port, timeout=15)
    connection.request('POST', '/sync/' + method, body=payload, headers={'anki-sync': header})
    response = connection.getresponse()
    output = response.read()
    result = {'status': response.status, 'response_bytes': len(output)}
    if 200 <= response.status < 300:
        result['value'] = json.loads(unpack(output))
    connection.close()
    return result


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
            try:
                receiver = b if direction == 'download' else a
                full(receiver, auth_b if direction == 'download' else auth_a, direction == 'upload')
                row['full_transfer'] = 'ok'
                row['receiver_graves'] = receiver.db.scalar('select count(*) from graves')
            except Exception as error:
                row['full_transfer'] = type(error).__name__
            finally: record.rewrite_session = None
            row['stale_finish'] = native_request(port, auth_a, 'finish', {'_pad': None}, session)
            result[name] = row
    return result


def smoke(fixture):
    global PHASE
    require(fixture == FIXTURE, "unknown synthetic fixture")
    identity = runtime()
    with tempfile.TemporaryDirectory(prefix="current-reference-") as scratch:
        root = pathlib.Path(scratch)
        PHASE = 'reference-server-start'
        with reference_server(root / "server") as (port, username, password), recorder(port) as record:
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
        result = runtime() if request["mode"] == "identity" else smoke(request["fixture"])
        sys.stdout.buffer.write(encoded(result))
    except BaseException as error:
        # Never export source frames, backend messages or collection content.
        sys.stdout.buffer.write(encoded({"failure": {"kind": type(error).__name__, "phase": PHASE, "recording_errors": RECORDER_ERRORS}}))
        sys.stdout.buffer.flush()
        os._exit(1)
