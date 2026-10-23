#!/usr/bin/env python3
"""Focused RF4 fixture tests: all counters below come from real local sockets."""
import concurrent.futures
import hashlib
import http.client
import http.server
import importlib.util
import json
import select
import signal
import socket
import subprocess
import sys
import threading
import time
import unittest
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parents[1] / 'scripts'


def load(name, filename):
    spec = importlib.util.spec_from_file_location(name, SCRIPTS / filename)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


wire = load('rf4_wire', 'manual-transfer-wire.py')
config = load('rf4_config', 'manual-transfer-config.py')


class Upstream(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_args):
        pass

    def do_POST(self):
        digest = hashlib.sha256()
        if self.headers.get('Transfer-Encoding') == 'chunked':
            while True:
                size = int(self.rfile.readline(64), 16)
                if not size:
                    self.rfile.read(2)
                    break
                digest.update(self.rfile.read(size))
                assert self.rfile.read(2) == b'\r\n'
        else:
            digest.update(self.rfile.read(int(self.headers.get('Content-Length', 0))))
        with self.server.lock:
            self.server.observed += 1
        body = digest.hexdigest().encode()
        self.send_response_only(200)
        self.send_header('Content-Length', str(len(body)))
        self.send_header('anki-original-size', str(len(body)))
        self.end_headers()
        self.wfile.write(body)


def request(port, path, token=None, method='POST', body=b'', host=None, chunked=False):
    connection = http.client.HTTPConnection('127.0.0.1', port, timeout=3)
    headers = {'Host': host or '127.0.0.1:%d' % port}
    if token:
        headers['X-RF4-Control'] = token
    if chunked:
        headers['Transfer-Encoding'] = 'chunked'
    try:
        connection.request(method, path, body=[body] if chunked else body,
                           headers=headers, encode_chunked=chunked)
        response = connection.getresponse()
        return response.status, response.read()
    finally:
        connection.close()


class WireTests(unittest.TestCase):
    def setUp(self):
        self.upstream = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Upstream)
        self.upstream.lock = threading.Lock()
        self.upstream.observed = 0
        self.server = wire.RigServer(self.upstream.server_port)
        self.threads = []
        for server in (self.upstream, self.server):
            thread = threading.Thread(target=server.serve_forever, kwargs={'poll_interval': 0.05})
            thread.start()
            self.threads.append(thread)
        self.pool = concurrent.futures.ThreadPoolExecutor(max_workers=1)

    def tearDown(self):
        self.server.stop()
        self.server.server_close()
        self.upstream.shutdown()
        self.upstream.server_close()
        for thread in self.threads:
            thread.join(timeout=2)
            self.assertFalse(thread.is_alive())
        self.pool.shutdown(wait=True)

    def control(self, action):
        status, body = request(self.server.server_port, '/__rf4/' + action, self.server.token)
        self.assertEqual(status, 200)
        return json.loads(body)

    def held_request(self):
        future = self.pool.submit(request, self.server.server_port, '/msync/downloadFiles')
        with self.server.condition:
            self.assertTrue(self.server.condition.wait_for(lambda: self.server.gate == 'holding', timeout=2))
        return future

    def protocol(self, path, **kwargs):
        result = request(self.server.server_port, path, **kwargs)
        with self.server.condition:
            self.assertTrue(self.server.condition.wait_for(lambda: self.server.counts['active'] == 0, timeout=2))
        return result

    def await_aborted(self, future):
        with self.assertRaises(http.client.RemoteDisconnected):
            future.result(timeout=2)
        with self.server.condition:
            self.assertTrue(self.server.condition.wait_for(lambda: self.server.counts['active'] == 0, timeout=2))

    def test_counts_before_consent_are_observed_not_inferred(self):
        self.assertEqual(self.control('mark/startup')['media'], 0)
        self.protocol('/sync/download')
        before = self.control('mark/before-consent')
        self.assertEqual((before['media'], before['collection_transfers']), (0, 1))
        # Any actual media request before consent changes the count, regardless
        # of app state or whether any media file arrived.
        self.protocol('/msync/begin')
        after = self.control('snapshot')
        self.assertEqual(after['media'], 1)
        self.assertEqual(self.upstream.observed, 2)
        self.assertEqual(self.server.server_address[0], '127.0.0.1')

    def test_gate_cancel_passive_restart_decision_resume_and_proof(self):
        self.control('mark/startup')
        self.protocol('/sync/download')
        self.control('mark/before-consent')
        self.protocol('/msync/begin')
        future = self.held_request()
        self.assertEqual(self.upstream.observed, 2, 'held file request must not reach upstream')
        self.assertEqual(self.control('snapshot')['media'], 2)
        self.control('abort')
        self.await_aborted(future)
        for name in ('cancelled', 'passive-history', 'resume-decision'):
            self.control('mark/' + name)
        self.control('release')
        self.protocol('/msync/downloadFiles')
        self.control('mark/resumed')
        self.protocol('/sync/download')
        self.protocol('/sync/upload')
        self.control('mark/export-before-consent')
        self.protocol('/msync/uploadChanges')
        self.control('mark/exported')
        proof = self.control('proof')
        config.verify_wire(proof)
        # An otherwise complete proof must fail each required M8 wire condition.
        for checkpoint, key in (('before-consent', 'media'), ('passive-history', 'media'),
                                ('resume-decision', 'media'), ('resumed', 'collection_transfers')):
            broken = json.loads(json.dumps(proof))
            broken['checkpoints'][checkpoint][key] += 1
            with self.assertRaises(ValueError):
                config.verify_wire(broken)
        self.assertNotIn(self.server.token, json.dumps(proof))

    def test_streams_fixed_and_chunked_bodies_without_protocol_change(self):
        body = b'synthetic-only' * 8192
        for chunked in (False, True):
            status, actual = self.protocol('/sync/upload', body=body, chunked=chunked)
            self.assertEqual(status, 200)
            self.assertEqual(actual, hashlib.sha256(body).hexdigest().encode())

    def test_forbidden_forward_and_controls_never_touch_upstream(self):
        for path, method, host in (
                ('http://127.0.0.1:1/sync/meta', 'POST', None),
                ('http://external.invalid/sync/meta', 'POST', None),
                ('127.0.0.1:1', 'CONNECT', None),
                ('/sync/download?target=elsewhere', 'POST', None),
                ('/msync/unknown', 'POST', None), ('/sync/%64ownload', 'POST', None),
                ('/sync/download', 'POST', 'external.invalid')):
            self.assertEqual(request(self.server.server_port, path, method=method, host=host)[0], 403)
        self.assertEqual(request(self.server.server_port, '/__rf4/release', token='wrong')[0], 403)
        self.assertEqual(request(self.server.server_port, '/__rf4/release', self.server.token)[0], 409)
        self.assertEqual(self.upstream.observed, 0)
        self.assertEqual(self.control('snapshot')['media'], 0)
        with self.assertRaises(ValueError):
            wire.RigServer(0)

    def test_gate_expiry_is_bounded_and_cannot_produce_success(self):
        old = wire.GATE_SECONDS
        wire.GATE_SECONDS = 0.15
        try:
            future = self.held_request()
            self.await_aborted(future)
            snapshot = self.control('snapshot')
            self.assertEqual((snapshot['errors'], snapshot['aborted']), (1, 1))
            self.assertEqual(self.upstream.observed, 0)
        finally:
            wire.GATE_SECONDS = old

    def test_child_eof_and_signal_teardown_are_bounded_while_holding(self):
        for ending in ('eof', 'term'):
            child = subprocess.Popen([sys.executable, str(SCRIPTS / 'manual-transfer-wire.py'),
                                      str(self.upstream.server_port)], stdin=subprocess.PIPE,
                                     stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            try:
                self.assertTrue(select.select([child.stdout], [], [], 3)[0])
                private = json.loads(child.stdout.readline())
                port = private['port']
                future = self.pool.submit(request, port, '/msync/downloadFiles')
                end = time.monotonic() + 3
                while time.monotonic() < end:
                    status, data = request(port, '/__rf4/snapshot', private['token'])
                    self.assertEqual(status, 200)
                    if json.loads(data)['gate'] == 'holding':
                        break
                else:
                    self.fail('child never observed file request')
                if ending == 'term':
                    child.send_signal(signal.SIGTERM)
                _, stderr = child.communicate(timeout=3)
                self.assertEqual(child.returncode, 0)
                self.assertEqual(stderr, b'')
                with self.assertRaises((http.client.RemoteDisconnected, ConnectionResetError)):
                    future.result(timeout=2)
                with socket.socket() as probe:
                    probe.settimeout(1)
                    self.assertNotEqual(probe.connect_ex(('127.0.0.1', port)), 0)
                self.assertEqual(self.upstream.observed, 0)
            finally:
                if child.poll() is None:
                    child.kill()
                child.communicate(timeout=3)


if __name__ == '__main__':
    unittest.main()
