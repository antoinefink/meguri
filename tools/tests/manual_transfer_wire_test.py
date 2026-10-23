#!/usr/bin/env python3
"""Focused RF4 fixture tests: all counters below come from real local sockets."""
import concurrent.futures
import hashlib
import copy
import contextlib
import io
import os
import tempfile
from unittest import mock
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


def synthetic_native(root):
    support = root / 'Library/Containers/com.antoinefink.meguri.dev/Data/Library/Application Support'
    (support / 'ManualTransfers').mkdir(parents=True)
    media = support / 'Collection/collection.media'
    media.mkdir(parents=True)
    (media / 'rf4-device.bin').write_bytes(b'synthetic-device')
    (media / 'rf4-zero.bin').write_bytes(b'')
    identity = '0000000000000001-0000000000000002'
    record = dict.fromkeys(('id account configured_endpoint target_endpoint direction scope phase acceptance boundary guard recovery_guard recovery_local local remote candidate activation_identity original safety_backups server_copy prepared_at actual_generation collection_complete resulting_publication media receipt revision').split())
    record.update(id=identity, account='a' * 64, configured_endpoint='http://reserved.invalid/',
                  target_endpoint='http://reserved.invalid/', direction='Import',
                  scope='CollectionAndMedia', phase='CollectionComplete', acceptance='ExactOk',
                  safety_backups=[], prepared_at=10, actual_generation=1,
                  collection_complete=True, resulting_publication=[123, 456], revision=5,
                  media=dict(stage='AwaitingConsent', consent=None,
                             counters=dict(uploaded_files=0, uploaded_deletions=0,
                                           downloaded_files=0, downloaded_deletions=0,
                                           checked=0, bytes_downloaded=0, bytes_uploaded=0,
                                           awaiting=0, resynced=False, empty=[], oversized=[]),
                             remote_outcome_unknown=False, exclusions_verified=False, failure_kind=None))
    history = dict(version=1, serial=1, active=identity, operations={identity: record}, cooldowns={})
    path = support / 'ManualTransfers/manual-transfers.json'
    path.write_text(json.dumps(dict(manualTransfers=history)))
    return support, path, history


class NativeStateTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.support, self.path, self.history = synthetic_native(Path(self.temp.name).resolve())
        self.context = config.native_context(self.support)

    def save(self):
        self.path.write_text(json.dumps(dict(manualTransfers=self.history)))

    def test_reduction_preserves_identity_cancel_resume_receipt_and_actual_bytes(self):
        original = config.native_state(self.context)
        record = next(iter(self.history['operations'].values()))
        self.assertIsNone(original['operations']['Export'])
        self.assertFalse(original['files']['rf4-server.bin']['exists'])
        self.assertIsNone(original['files']['rf4-server.bin']['bytes'])
        self.assertEqual(original['files']['rf4-device.bin']['sha256'], hashlib.sha256(b'synthetic-device').hexdigest())
        self.assertEqual(original['files']['rf4-zero.bin']['bytes'], 0)
        record['phase'] = 'MediaIncomplete'
        record['media']['stage'] = 'Cancelled'
        record['media']['remote_outcome_unknown'] = True
        self.save()
        partial = config.native_state(self.context)['operations']['Import']
        for key in ('id', 'authority', 'account', 'configured_endpoint', 'resulting_publication'):
            self.assertEqual(partial[key], original['operations']['Import'][key])
        self.assertTrue(partial['collection_complete'])
        self.assertIsNone(partial['receipt']); self.assertIsNone(partial['success_cooldown'])
        self.assertTrue(partial['media']['remote_outcome_unknown'])
        self.assertEqual(partial['media']['counters']['downloaded_files'], 0)
        self.assertEqual(partial['media']['counters']['uploaded_files'], 0)
        self.assertEqual(config.native_state(self.context)['operations']['Import'], partial)
        record['media']['stage'] = 'AwaitingConsent'; self.save()
        self.assertIsNone(config.native_state(self.context)['operations']['Import']['media']['consent'])
        record['phase'] = 'Complete'; record['media']['stage'] = 'Complete'
        record['media']['exclusions_verified'] = True
        record['media']['consent'] = dict(revision=5, generation=1, categories=1,
                                         account=record['account'], configured_endpoint=record['configured_endpoint'])
        record['receipt'] = dict(completed_at=20, next_eligible_at=3600020)
        self.history['cooldowns'][record['account']] = record['receipt']
        self.save()
        finished = config.native_state(self.context)
        self.assertTrue(finished['operations']['Import']['media']['consent_bound'])
        self.assertEqual(finished['operations']['Import']['receipt'], finished['operations']['Import']['success_cooldown'])
        output = json.dumps(finished)
        self.assertNotIn(record['account'], output)
        self.assertNotIn(record['configured_endpoint'], output)
        self.assertNotIn(str(self.support), output)
        self.assertNotIn('synthetic-device', output)

    def test_missing_is_distinct_from_denial_symlinks_and_changing_sidecar(self):
        self.assertFalse(config.native_state(self.context)['files']['rf4-server.bin']['exists'])
        real_open = os.open
        def denied(path, *args, **kwargs):
            if path == 'rf4-server.bin':
                raise PermissionError('private error never exported')
            return real_open(path, *args, **kwargs)
        with mock.patch.object(config.os, 'open', side_effect=denied):
            with self.assertRaises(PermissionError):
                config.native_state(self.context)
        server = self.support / 'Collection/collection.media/rf4-server.bin'
        server.symlink_to(self.path)
        with self.assertRaises((OSError, ValueError)):
            config.native_state(self.context)
        server.unlink()
        original = config.bounded_read
        def changing(parent, name, *args, **kwargs):
            result = original(parent, name, *args, **kwargs)
            if name == 'rf4-device.bin':
                self.path.write_text('{}')
            return result
        with mock.patch.object(config, 'bounded_read', side_effect=changing):
            with self.assertRaises(ValueError):
                config.native_state(self.context)

    def test_context_identity_parent_symlink_schema_and_selection_fail_closed(self):
        wrong = dict(self.context, inode=self.context['inode'] + 1)
        with self.assertRaises(ValueError):
            config.native_state(wrong)
        with self.assertRaises(ValueError):
            config.native_context(Path(self.temp.name) / 'wrong/Library/Application Support')
        transfer = self.support / 'ManualTransfers'
        transfer.rename(self.support / 'moved')
        transfer.symlink_to(self.support / 'moved', target_is_directory=True)
        with self.assertRaises(OSError):
            config.native_state(self.context)
        transfer.unlink(); (self.support / 'moved').rename(transfer)
        record = next(iter(self.history['operations'].values()))
        record['collection_complete'] = 1; self.save()
        with self.assertRaises(ValueError):
            config.native_state(self.context)
        record['collection_complete'] = True
        extra = copy.deepcopy(record); extra['id'] = '0000000000000001-0000000000000003'
        self.history['operations'][extra['id']] = extra; self.save()
        with self.assertRaises(ValueError):
            config.native_state(self.context)
        del self.history['operations'][extra['id']]
        newer = copy.deepcopy(record); newer['id'] = '0000000000000002-0000000000000001'
        newer['prepared_at'] = 1
        self.history['serial'] = 2; self.history['operations'][newer['id']] = newer; self.save()
        self.assertEqual(config.native_state(self.context)['operations']['Import']['id'], newer['id'])
        self.path.write_text('{"manualTransfers":{},"manualTransfers":{}}')
        with self.assertRaises(ValueError):
            config.native_state(self.context)

    def test_reader_cli_never_emits_malformed_sidecar_or_exception_text(self):
        context_path = Path(self.temp.name) / 'context.json'
        context_path.write_text(json.dumps(self.context))
        self.path.write_text('{"private":"never-export-private-input"}')
        child = subprocess.run([sys.executable, str(SCRIPTS / 'manual-transfer-config.py'),
                                'native-state', str(context_path)], capture_output=True, timeout=3)
        self.assertNotEqual(child.returncode, 0)
        self.assertEqual(child.stdout, b'')
        self.assertEqual(child.stderr, b'manual-transfer-data: synthetic configuration refused\n')

    def test_reduced_schema_rejects_extra_fields_aliases_hashes_names_and_absence(self):
        value = config.native_state(self.context)
        config.validate_native_state(value, config.fingerprint(self.context))
        variants = []
        bad = copy.deepcopy(value); bad['extra'] = 'private'; variants.append(bad)
        bad = copy.deepcopy(value); bad['operations']['Import']['media']['remote_outcome_unknown'] = 1; variants.append(bad)
        bad = copy.deepcopy(value); bad['operations']['Import']['media']['counters']['checked'] = True; variants.append(bad)
        bad = copy.deepcopy(value); bad['operations']['Import']['media']['counters']['empty'] = ['other']; variants.append(bad)
        bad = copy.deepcopy(value); bad['files']['rf4-device.bin']['sha256'] = 'f'; variants.append(bad)
        bad = copy.deepcopy(value); bad['operations']['Import'] = None; variants.append(bad)
        bad = copy.deepcopy(value); bad['operations']['Import']['phase'] = 'unrecognized'; variants.append(bad)
        bad = copy.deepcopy(value); bad['files']['rf4-server.bin']['bytes'] = 0; variants.append(bad)
        for bad in variants:
            with self.assertRaises(ValueError):
                config.validate_native_state(bad, config.fingerprint(self.context))


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

    def reader(self, code):
        self.server.reader_context = 'a' * 64
        self.server.reader_command = [sys.executable, '-c', code]

    def test_native_state_exact_control_auth_schema_and_private_output(self):
        with tempfile.TemporaryDirectory() as temporary:
            support, _, _ = synthetic_native(Path(temporary).resolve())
            value = config.native_state(config.native_context(support))
        self.reader('import sys; sys.stderr.write("private-reader-error"); print(' + repr(json.dumps(value)) + ')')
        self.server.reader_context = value['context']
        self.assertEqual(self.control('native-state'), value)
        self.reader('raise SystemExit("private-reader-error")')
        with contextlib.redirect_stderr(io.StringIO()) as logs, mock.patch.object(wire.subprocess, 'Popen') as launch:
            for path, token, body, expected in (
                ('/__rf4/native-state', None, b'', 403),
                ('/__rf4/native-state', 'wrong', b'', 403),
                ('/__rf4/native-state?path=/tmp/private', self.server.token, b'', 409),
                ('/__rf4/native-state/action', self.server.token, b'', 409),
                ('/__rf4/native-state', self.server.token, b'{"path":"private"}', 400)):
                status, data = request(self.server.server_port, path, token, body=body)
                self.assertEqual((status, data), (expected, b'{}'))
            launch.assert_not_called()
        self.assertEqual(logs.getvalue(), '')
        self.assertEqual(request(self.server.server_port, '/__rf4/native-state', self.server.token), (503, b'{}'))
        for output in ('{}', 'not-json', '{"private":"never-export"}', 'x' * (wire.READER_LIMIT + 1)):
            self.reader('print(' + repr(output) + ')')
            self.assertEqual(request(self.server.server_port, '/__rf4/native-state', self.server.token), (503, b'{}'))
        # Even a valid reduced response is refused when selected context changed.
        self.reader('print(' + repr(json.dumps(value)) + ')')
        self.assertEqual(request(self.server.server_port, '/__rf4/native-state', self.server.token), (503, b'{}'))

    def await_reader(self):
        end = time.monotonic() + 2
        while time.monotonic() < end:
            with self.server.reader_guard:
                if self.server.reader_process is not None:
                    return self.server.reader_process
            time.sleep(0.01)
        self.fail('synthetic reader did not start')

    def test_native_timeout_kills_drains_and_reaps_process_group(self):
        previous = wire.READER_SECONDS
        wire.READER_SECONDS = 0.2
        try:
            # A descendant retaining stdout must also die for drain to finish.
            self.reader('import subprocess,sys,time; subprocess.Popen([sys.executable,"-c","import time; time.sleep(60)"]); time.sleep(60)')
            future = self.pool.submit(request, self.server.server_port, '/__rf4/native-state', self.server.token)
            child = self.await_reader()
            self.assertEqual(future.result(timeout=3), (503, b'{}'))
            self.assertIsNotNone(child.returncode)
            self.assertTrue(child.stdout.closed)
            self.assertTrue(child.stdin.closed)
            self.assertIsNone(self.server.reader_process)
        finally:
            wire.READER_SECONDS = previous

    def test_native_busy_refused_and_shutdown_drains_active_reader(self):
        self.reader('import time; time.sleep(60)')
        future = self.pool.submit(request, self.server.server_port, '/__rf4/native-state', self.server.token)
        child = self.await_reader()
        self.assertEqual(request(self.server.server_port, '/__rf4/native-state', self.server.token), (409, b'{}'))
        # Protocol snapshots remain responsive: subprocess work holds no condition.
        self.control('snapshot')
        self.server.stop_reader()
        self.assertEqual(future.result(timeout=2), (503, b'{}'))
        self.assertIsNotNone(child.returncode)
        self.assertTrue(child.stdout.closed)
        self.assertIsNone(self.server.reader_process)

    def test_native_client_cancellation_kills_and_drains_reader(self):
        self.reader('import time; time.sleep(60)')
        client = socket.create_connection(('127.0.0.1', self.server.server_port), timeout=2)
        try:
            headers = ('POST /__rf4/native-state HTTP/1.1\r\nHost: 127.0.0.1:%d\r\n'
                       'X-RF4-Control: %s\r\nContent-Length: 0\r\n\r\n') % (self.server.server_port, self.server.token)
            client.sendall(headers.encode('ascii'))
            child = self.await_reader()
        finally:
            client.close()
        self.assertTrue(self.server.reader_slot.acquire(timeout=2))
        self.server.reader_slot.release()
        self.assertIsNotNone(child.returncode)
        self.assertTrue(child.stdout.closed)

    def test_observer_eof_term_hup_drain_active_reader_and_close_socket(self):
        for ending in ('eof', 'term', 'hup'):
            with tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                marker = root / 'reader.pid'
                code = 'import os,time; from pathlib import Path; Path(' + repr(str(marker)) + ').write_text(str(os.getpid())); time.sleep(60)'
                command = root / 'reader-command.json'
                command.write_text(json.dumps(dict(context='a' * 64, command=[sys.executable, '-c', code])))
                child = subprocess.Popen([sys.executable, str(SCRIPTS / 'manual-transfer-wire.py'),
                                          str(self.upstream.server_port), str(command)],
                                         stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
                try:
                    self.assertTrue(select.select([child.stdout], [], [], 3)[0])
                    ready = json.loads(child.stdout.readline())
                    future = self.pool.submit(request, ready['port'], '/__rf4/native-state', ready['token'])
                    end = time.monotonic() + 2
                    while not marker.exists() and time.monotonic() < end:
                        time.sleep(0.01)
                    self.assertTrue(marker.exists())
                    reader_pid = int(marker.read_text())
                    if ending != 'eof':
                        child.send_signal(signal.SIGTERM if ending == 'term' else signal.SIGHUP)
                    _, stderr = child.communicate(timeout=3)
                    self.assertEqual((child.returncode, stderr), (0, b''))
                    try:
                        self.assertEqual(future.result(timeout=2), (503, b'{}'))
                    except (http.client.RemoteDisconnected, ConnectionResetError):
                        pass  # shutdown may close before the static refusal
                    with self.assertRaises(ProcessLookupError):
                        os.kill(reader_pid, 0)
                    with socket.socket() as probe:
                        self.assertNotEqual(probe.connect_ex(('127.0.0.1', ready['port'])), 0)
                finally:
                    if child.poll() is None:
                        child.kill()
                    child.communicate(timeout=3)


if __name__ == '__main__':
    unittest.main()
