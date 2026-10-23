#!/usr/bin/env python3
"""RF4-only loopback observer. No destinations, URLs or payloads in controls/logs.

The MG controller supplies the published port of its validated, owned sync rig.
Only exact protocol paths are forwarded there. Bodies stream in <=64 KiB pieces.
The first file request is held *before* any upstream connection until UI cancel;
the native test then aborts that held socket and explicitly releases for Resume.
"""
import hmac
import importlib.util
import select
import subprocess
from pathlib import Path
import http.client
import http.server
import json
import os
import secrets
import signal
import socket
import threading
import time

COLLECTION = frozenset('/sync/' + name for name in (
    'hostKey', 'meta', 'start', 'applyGraves', 'applyChanges', 'chunk',
    'applyChunk', 'sanityCheck2', 'finish', 'abort', 'upload', 'download'))
MEDIA = frozenset('/msync/' + name for name in (
    'begin', 'mediaChanges', 'uploadChanges', 'downloadFiles', 'mediaSanity'))
TRANSFERS = frozenset(('/sync/upload', '/sync/download'))
FILES = frozenset(('/msync/downloadFiles', '/msync/uploadChanges'))
CHECKPOINTS = ('startup', 'before-consent', 'cancelled', 'passive-history',
               'resume-decision', 'resumed', 'export-before-consent', 'exported')
HOP = frozenset(('host', 'connection', 'proxy-connection', 'keep-alive',
                 'transfer-encoding', 'content-length', 'te', 'trailer', 'upgrade',
                 'proxy-authorization', 'proxy-authenticate', 'x-rf4-control'))
BUFFER = 65536
BODY_LIMIT = 16 * 1024 * 1024  # synthetic C/D fixtures only, never arbitrary data
GATE_SECONDS = 90
IO_SECONDS = 10
REQUEST_SECONDS = 120
LIFETIME_SECONDS = 1800
READER_SECONDS = 8  # plus <=2 seconds to kill, drain and reap
READER_LIMIT = 65536
_spec = importlib.util.spec_from_file_location('rf4_reducer', Path(__file__).with_name('manual-transfer-config.py'))
reducer = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(reducer)


def drain_reader(child):
    try:
        os.killpg(child.pid, signal.SIGKILL)
    except ProcessLookupError:
        pass
    if child.stdin:
        child.stdin.close()
    deadline = time.monotonic() + 1
    while time.monotonic() < deadline:
        if select.select([child.stdout], [], [], 0.05)[0]:
            if not os.read(child.stdout.fileno(), 8192):
                break
    child.stdout.close()
    child.wait(timeout=1)


class RigServer(http.server.ThreadingHTTPServer):
    daemon_threads = True
    block_on_close = False

    def __init__(self, upstream_port, reader_command=None, reader_context=None):
        if type(upstream_port) is not int or not 1024 <= upstream_port <= 65535:
            raise ValueError('invalid owned rig port')
        super().__init__(('127.0.0.1', 0), Handler)
        self.upstream_port = upstream_port
        self.token = secrets.token_hex(32)
        self.condition = threading.Condition()
        self.slots = threading.BoundedSemaphore(8)
        self.counts = dict(media=0, collection=0, collection_transfers=0,
                           active=0, held=0, aborted=0, errors=0)
        self.checkpoints = {}
        self.gate = 'armed'
        self.stopping = False
        self.reader_command = reader_command
        self.reader_context = reader_context
        self.reader_slot = threading.Lock()
        self.reader_guard = threading.Lock()
        self.reader_process = None

    def process_request(self, request, address):
        if not self.slots.acquire(blocking=False):
            request.close()
            return
        try:
            super().process_request(request, address)
        except BaseException:
            self.slots.release()
            raise

    def process_request_thread(self, request, address):
        try:
            super().process_request_thread(request, address)
        finally:
            self.slots.release()

    def handle_error(self, request, client_address):
        # Never print exception context, headers, request lines or payloads.
        with self.condition:
            self.counts['errors'] += 1

    def stop_reader(self):
        with self.reader_guard:
            self.stopping = True
            if self.reader_process is not None:
                try:
                    os.killpg(self.reader_process.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
        # The request worker owns draining/reaping. Never release the controller
        # lease while that worker could still start or retain a reader.
        with self.reader_slot:
            pass

    def native_state(self, connection):
        if not self.reader_command or not self.reader_slot.acquire(blocking=False):
            return 409, {}
        child = None
        try:
            with self.reader_guard:
                if self.stopping:
                    return 409, {}
                child = subprocess.Popen(self.reader_command, stdin=subprocess.PIPE,
                                         stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                                         start_new_session=True, close_fds=True)
                self.reader_process = child
            deadline = time.monotonic() + READER_SECONDS
            output = bytearray()
            while True:
                if self.stopping or time.monotonic() >= deadline:
                    raise ValueError('reader stopped')
                readable, _, _ = select.select([child.stdout, connection], [], [], 0.05)
                if connection in readable:
                    # A control body is forbidden; EOF is cancellation.
                    raise ValueError('reader client cancelled')
                if child.stdout in readable:
                    piece = os.read(child.stdout.fileno(), min(8192, READER_LIMIT + 1 - len(output)))
                    if not piece:
                        break
                    output.extend(piece)
                    if len(output) > READER_LIMIT:
                        raise ValueError('reader output bound')
            remaining = deadline - time.monotonic()
            if remaining <= 0 or child.wait(timeout=remaining) != 0 or self.stopping:
                raise ValueError('reader failed')
            value = reducer.validate_native_state(reducer.strict_json(output), self.reader_context)
            return 200, value
        except (OSError, ValueError, TypeError, KeyError, RecursionError, subprocess.SubprocessError):
            return 503, {}
        finally:
            try:
                if child is not None:
                    drain_reader(child)
            finally:
                with self.reader_guard:
                    self.reader_process = None
                self.reader_slot.release()

    def stop(self):
        self.stop_reader()
        with self.condition:
            self.stopping = True
            self.condition.notify_all()
        self.shutdown()

    def snapshot(self):
        return dict(self.counts, gate=self.gate)


class Handler(http.server.BaseHTTPRequestHandler):
    protocol_version = 'HTTP/1.1'

    def log_message(self, *_args):
        pass

    def setup(self):
        super().setup()
        self.connection.settimeout(IO_SECONDS)

    def send_error(self, code, *_args):
        # Base implementation echoes request commands in some error bodies.
        self.reply(code, {})

    def reply(self, status, value):
        body = json.dumps(value, separators=(',', ':')).encode()
        self.send_response_only(status)
        self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(body)))
        self.send_header('Connection', 'close')
        self.end_headers()
        self.wfile.write(body)
        self.close_connection = True

    def valid_host(self):
        return self.headers.get_all('Host') in (
            ['127.0.0.1:18765'], ['127.0.0.1:%d' % self.server.server_port])

    def do_CONNECT(self):
        self.reply(403, {})

    def do_GET(self):
        self.reply(200 if self.path == '/' and self.valid_host() else 403, {})

    def do_POST(self):
        self.close_connection = True
        if not self.valid_host():
            self.reply(403, {})
            return
        if self.path.startswith('/__rf4/'):
            self.control()
            return
        # Exact origin paths only: no queries, escaping, CONNECT or absolute URLs.
        if self.path not in COLLECTION | MEDIA:
            self.reply(403, {})
            return
        state = self.server
        with state.condition:
            state.counts['media' if self.path in MEDIA else 'collection'] += 1
            state.counts['collection_transfers'] += int(self.path in TRANSFERS)
            state.counts['active'] += 1
        upstream = None
        try:
            if self.path in FILES:
                with state.condition:
                    if state.gate == 'armed':
                        state.gate = 'holding'
                        state.counts['held'] += 1
                        state.condition.notify_all()
                        opened = state.condition.wait_for(
                            lambda: state.gate != 'holding' or state.stopping,
                            timeout=GATE_SECONDS)
                        if not opened:
                            state.counts['errors'] += 1
                        if not opened or state.stopping or state.gate == 'aborted':
                            state.counts['aborted'] += 1
                            return  # close without touching upstream
                    elif state.gate != 'released':
                        raise ValueError('file request before explicit release')
            deadline = time.monotonic() + REQUEST_SECONDS
            upstream = http.client.HTTPConnection('127.0.0.1', state.upstream_port,
                                                  timeout=IO_SECONDS)
            upstream.putrequest('POST', self.path, skip_accept_encoding=True)
            for key, value in self.headers.items():
                if key.lower() not in HOP:
                    upstream.putheader(key, value)
            upstream.putheader('Transfer-Encoding', 'chunked')
            upstream.putheader('Connection', 'close')
            upstream.endheaders()
            for piece in self.body(deadline):
                upstream.send(('%x\r\n' % len(piece)).encode() + piece + b'\r\n')
            upstream.send(b'0\r\n\r\n')
            response = upstream.getresponse()
            if 300 <= response.status < 400:
                raise ValueError('fixture redirects forbidden')
            self.send_response_only(response.status)
            for key, value in response.getheaders():
                if key.lower() not in HOP:
                    self.send_header(key, value)
            self.send_header('Transfer-Encoding', 'chunked')
            self.send_header('Connection', 'close')
            self.end_headers()
            total = 0
            while True:
                if time.monotonic() > deadline:
                    raise ValueError('response deadline')
                piece = response.read1(BUFFER)
                if not piece:
                    break
                total += len(piece)
                if total > BODY_LIMIT:
                    raise ValueError('synthetic response bound')
                self.wfile.write(('%x\r\n' % len(piece)).encode() + piece + b'\r\n')
            self.wfile.write(b'0\r\n\r\n')
        except (OSError, ValueError, http.client.HTTPException):
            with state.condition:
                state.counts['errors'] += 1
        finally:
            if upstream:
                upstream.close()
            with state.condition:
                state.counts['active'] -= 1
                state.condition.notify_all()

    def body(self, deadline):
        sizes = self.headers.get_all('Content-Length', [])
        encodings = self.headers.get_all('Transfer-Encoding', [])
        if encodings == ['chunked'] and not sizes:
            remaining = None
        elif not encodings and len(sizes) == 1 and sizes[0].isascii() and sizes[0].isdigit():
            remaining = int(sizes[0])
        else:
            raise ValueError('ambiguous framing')
        total = 0
        while remaining is None or remaining > 0:
            chunked = remaining is None
            if chunked:
                line = self.rfile.readline(64)
                if not line.endswith(b'\r\n') or len(line) > 18:
                    raise ValueError('chunk size bound')
                if not line[:-2] or any(c not in b'0123456789abcdefABCDEF' for c in line[:-2]):
                    raise ValueError('chunk size syntax')
                size = int(line[:-2], 16)
                if size == 0:
                    if self.rfile.read(2) != b'\r\n':
                        raise ValueError('trailers forbidden')
                    return
            else:
                size = remaining
            total += size
            if total > BODY_LIMIT:
                raise ValueError('synthetic request bound')
            left = size
            while left:
                if time.monotonic() > deadline:
                    raise ValueError('request deadline')
                piece = self.rfile.read(min(left, BUFFER))
                if not piece:
                    raise ValueError('short body')
                left -= len(piece)
                yield piece
            if chunked:
                if self.rfile.read(2) != b'\r\n':
                    raise ValueError('chunk ending')
            else:
                remaining = 0

    def control(self):
        state = self.server
        tokens = self.headers.get_all('X-RF4-Control', [])
        if len(tokens) != 1 or not hmac.compare_digest(tokens[0], state.token):
            self.reply(403, {})
            return
        if self.headers.get_all('Content-Length') != ['0'] or self.headers.get_all('Transfer-Encoding'):
            self.reply(400, {})
            return
        if self.path == '/__rf4/native-state':
            status, value = state.native_state(self.connection)
            self.reply(status, value)
            return
        action = self.path[len('/__rf4/'):]
        with state.condition:
            if action == 'abort' and state.gate == 'holding':
                state.gate = 'aborted'
                state.condition.notify_all()
            elif action == 'release' and state.gate == 'aborted' and state.counts['active'] == 0:
                state.gate = 'released'
            elif action.startswith('mark/') and action[5:] in CHECKPOINTS:
                name = action[5:]
                if name in state.checkpoints:
                    self.reply(409, {})
                    return
                state.checkpoints[name] = state.snapshot()
            elif action not in ('snapshot', 'proof'):
                self.reply(409, {})
                return
            value = state.snapshot()
            if action == 'proof':
                value['checkpoints'] = dict(state.checkpoints)
        self.reply(200, value)


def main():
    import sys
    command, context = None, None
    if len(sys.argv) == 3:
        with open(sys.argv[2], 'rb') as stream:
            data = stream.read(16385)
        if len(data) > 16384:
            raise ValueError('reader configuration bound')
        config = reducer.strict_json(data)
        reducer.keys(config, ('command', 'context'))
        command, context = config['command'], reducer.digest(config['context'])
        if type(command) is not list or not 2 <= len(command) <= 16 or any(type(arg) is not str or not arg or len(arg) > 8192 or '\x00' in arg for arg in command):
            raise ValueError('reader configuration')
    elif len(sys.argv) != 2:
        raise ValueError('observer arguments')
    server = RigServer(int(sys.argv[1]), command, context)
    def stop(*_args):
        threading.Thread(target=server.stop, daemon=True).start()
    signal.signal(signal.SIGTERM, stop)
    signal.signal(signal.SIGHUP, stop)
    # EOF binds child lifetime to the MG controller even when its shell dies.
    def watch_input():
        os.read(0, 1)
        stop()
    threading.Thread(target=watch_input, daemon=True).start()
    watchdog = threading.Timer(LIFETIME_SECONDS, stop)
    watchdog.daemon = True
    watchdog.start()
    ready = dict(port=server.server_port, token=server.token)
    if context is not None:
        ready['nativeContext'] = context
    print(json.dumps(ready), flush=True)
    try:
        server.serve_forever(poll_interval=0.1)
    finally:
        watchdog.cancel()
        server.stop_reader()
        server.server_close()


if __name__ == '__main__':
    try:
        main()
    except Exception:
        # No tracebacks: exception text can hold opaque HTTP inputs.
        os.write(2, b'manual-transfer-wire: controlled fixture refused\n')
        raise SystemExit(1)
