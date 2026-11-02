#!/usr/bin/env python3
"""Synthetic only; authoring leaves these tests unrun."""
import importlib.util
import io
import json
import os
from pathlib import Path
import select
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location('crash_info', Path(__file__).parents[1] / 'scripts/vm-crash-info.py')
m = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(m)


class CrashInfoTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.run = '20260101T010000Z-321'
        self.suite = 'drive-generated-sheet'
        self.base = self.root / 'build/mg/mac-ui' / self.run
        (self.base / self.suite).mkdir(parents=True)
        self.receipt = dict(schema_version=1, job='mac-ui', run_id=self.run, root=str(self.root),
                            dry_run=False, placement='host', status='error', args=[],
                            started_at='2026-01-01T01:00:00Z', finished_at='2026-01-01T01:01:00Z',
                            duration_ms=60000,
                            details=dict(suites=['drive:tools/drive-scripts/examples/generated-sheet.json'],
                                         mac_vm=dict(name='generated-vm')),
                            artifacts=[dict(kind='log', path=str(self.base / 'log.txt'))])
        self.write_receipt()
        (self.base / 'log.txt').write_text('drive: running tools/drive-scripts/examples/generated-sheet.json on the Mac (vm) (Debug)\n')
        (self.base / self.suite / 'drive.log').write_text('2026-01-01 01:00:30.100 +0000 xcodebuild[456:789] generated\n')
        self.reports = self.root / 'reports'
        self.reports.mkdir()
        self.admission = dict(pid=456, start=m.timestamp('2026-01-01T01:00:00Z'), end=m.timestamp('2026-01-01T01:01:00Z'))

    def write_receipt(self):
        (self.base / 'result.json').write_text(json.dumps(self.receipt))

    def ips(self, **changes):
        body = dict(procName='xcodebuild', pid=456, captureTime='2026-01-01 01:00:30.100 +0000',
                    exception=dict(signal='SIGABRT'), faultingThread=1,
                    threads=[dict(frames=[dict(imageIndex=1)]), dict(frames=[dict(imageIndex=0)])],
                    usedImages=[dict(name='libsystem_kernel.dylib', path='/usr/lib/system/libsystem_kernel.dylib'),
                                dict(name='AppKit', path='/System/Library/Frameworks/AppKit.framework/AppKit')])
        body.update(changes)
        return (json.dumps(dict(app_name='xcodebuild', private='generated secret')) + '\n' + json.dumps(body)).encode()

    def report(self, data):
        path = self.reports / 'xcodebuild-generated.ips'
        path.write_bytes(data)
        return path

    def test_admission_derives_process_time_and_pins_all_originals(self):
        value = m.admit(str(self.root), self.run, self.suite)
        self.assertEqual(value['pid'], 456)
        self.assertAlmostEqual(value['start'], self.admission['start'] - 29.9, places=5)
        before = value['hashes']
        with (self.base / 'log.txt').open('a') as out:
            out.write('changed\n')
        self.assertNotEqual(before, m.admit(str(self.root), self.run, self.suite)['hashes'])

    def test_wrong_job_run_suite_root_paths_and_dry_run_refused(self):
        for key, value in [('job', 'drive'), ('run_id', 'different'), ('root', '/generated-wrong'), ('dry_run', True)]:
            with self.subTest(key=key):
                original = self.receipt[key]
                self.receipt[key] = value
                self.write_receipt()
                with self.assertRaises(m.Refused):
                    m.admit(str(self.root), self.run, self.suite)
                self.receipt[key] = original
        self.receipt['artifacts'][0]['path'] = '/generated-private/log.txt'
        self.write_receipt()
        with self.assertRaises(m.Refused):
            m.admit(str(self.root), self.run, self.suite)
        for run, suite in [('../other', self.suite), (self.run, '../suite'), (self.run, 'drive-unknown')]:
            with self.assertRaises((m.Refused, OSError)):
                m.admit(str(self.root), run, suite)

    def test_missing_ambiguous_or_outside_original_evidence_refused(self):
        path = self.base / self.suite / 'drive.log'
        for text in ('',
                     '2026-01-01 01:00:30Z xcodebuild[456:789] one\n2026-01-01 01:00:31Z xcodebuild[457:789] two\n',
                     '2026-01-02 01:00:30Z xcodebuild[456:789] wrong date\n'):
            path.write_text(text)
            with self.assertRaises(m.Refused):
                m.admit(str(self.root), self.run, self.suite)

    def unzoned(self):
        # Generated real shape: the local clock is deliberately unrelated to
        # UTC. It must supply only a PID, never an inferred UTC crash point.
        self.receipt.update(started_at='2026-11-02T11:07:48Z',
                            finished_at='2026-11-02T11:11:30Z', duration_ms=222437)
        self.write_receipt()
        (self.base / self.suite / 'drive.log').write_text(
            '2026-11-02 12:11:15.123 xcodebuild[20674:987654] [MT] generated diagnostic\n')
        return m.admit(str(self.root), self.run, self.suite)

    def test_unzoned_uses_only_original_receipt_and_requires_one_report(self):
        admission = self.unzoned()
        self.assertEqual(admission['pid'], 20674)
        self.assertTrue(admission['exact_one'])
        self.assertEqual(admission['start'], m.timestamp(self.receipt['started_at']))
        self.assertEqual(admission['end'], m.timestamp(self.receipt['finished_at']))
        for when in ('2026-11-02T11:07:48Z', '2026-11-02T11:11:30Z'):
            self.report(self.ips(pid=20674, captureTime=when))
            value = m.validate_public(m.scan(self.reports, admission), True)
            self.assertEqual(value['outcome'], 'observed')
            self.assertEqual(value['counts']['matched'], 1)
        self.report(self.ips(pid=20674, captureTime='2026-11-02T11:11:31Z'))
        self.assertEqual(m.validate_public(m.scan(self.reports, admission), True)['outcome'], 'refused')

    def test_unzoned_multiple_matching_reports_refuse_without_exporting_facts(self):
        admission = self.unzoned()
        data = self.ips(pid=20674, captureTime='2026-11-02T11:10:00Z')
        self.report(data)
        (self.reports / 'xcodebuild-second.ips').write_bytes(data)
        value = m.validate_public(m.scan(self.reports, admission), True)
        self.assertEqual((value['outcome'], value['category']), ('refused', 'reports'))
        self.assertEqual(value['counts']['matched'], 2)
        self.assertEqual(value['reports'], [])
        for path in self.reports.iterdir():
            path.unlink()
        self.assertEqual(m.validate_public(m.scan(self.reports, admission), True)['outcome'], 'refused')
        self.assertEqual(m.scan(self.reports / 'missing', admission)['outcome'], 'refused')

    def test_unzoned_ambiguous_pid_or_bad_receipt_window_refuses(self):
        self.unzoned()
        original = dict(self.receipt)
        for changes in (dict(duration_ms=0), dict(duration_ms=-1), dict(duration_ms=420001),
                        dict(duration_ms=True), dict(duration_ms=None),
                        dict(finished_at='2026-11-02T11:07:48Z'),
                        dict(finished_at='2026-11-02T11:07:47Z'),
                        dict(finished_at='2026-11-02T11:14:49Z'),
                        dict(started_at='2026-11-02T12:07:48+01:00')):
            with self.subTest(changes=changes):
                self.receipt = dict(original, **changes)
                self.write_receipt()
                with self.assertRaises(m.Refused):
                    m.admit(str(self.root), self.run, self.suite)
        self.receipt = original
        self.write_receipt()
        with (self.base / self.suite / 'drive.log').open('a') as output:
            output.write('2026-11-02 12:11:16.123 xcodebuild[20675:987655] generated\n')
        with self.assertRaises(m.Refused):
            m.admit(str(self.root), self.run, self.suite)

    def test_zoned_process_span_stays_strict_even_with_short_receipt(self):
        self.unzoned()
        path = self.base / self.suite / 'drive.log'
        path.write_text('2026-11-02 11:07:47Z xcodebuild[20674:123] generated\n')
        with self.assertRaises(m.Refused):
            m.admit(str(self.root), self.run, self.suite)
        self.receipt['finished_at'] = '2026-11-02T11:14:00Z'
        self.write_receipt()
        path.write_text('2026-11-02 11:07:48Z xcodebuild[20674:123] generated\n'
                        '2026-11-02 11:12:49Z xcodebuild[20674:123] generated\n')
        with self.assertRaises(m.Refused):
            m.admit(str(self.root), self.run, self.suite)

    def test_two_objects_and_actual_faulting_thread_first_frame(self):
        self.report(self.ips())
        value = m.validate_public(m.scan(self.reports, self.admission))
        self.assertEqual(value['outcome'], 'observed')
        self.assertEqual(value['reports'], [dict(process='xcodebuild', signal='SIGABRT', library='libsystem_kernel.dylib')])
        self.assertNotIn('secret', json.dumps(value))

    def test_pid_and_time_mismatch_are_absence_not_pass(self):
        for changes, count in [(dict(pid=457), 'pid_mismatch'),
                               (dict(captureTime='2026-01-01T01:10:00Z'), 'time_mismatch')]:
            self.report(self.ips(**changes))
            value = m.validate_public(m.scan(self.reports, self.admission))
            self.assertEqual(value['counts'][count], 1)
            self.assertEqual(value['outcome'], 'no-evidence')
            self.assertEqual(value['reports'], [])

    def test_links_oversize_corrupt_and_changed_refused(self):
        path = self.report(b'generated corrupt private text')
        self.assertEqual(m.scan(self.reports, self.admission)['counts']['corrupt'], 1)
        path.unlink()
        path.symlink_to(self.base / 'result.json')
        self.assertEqual(m.scan(self.reports, self.admission)['counts']['link'], 1)
        path.unlink()
        path.write_bytes(b'x' * (m.MAX_FILE + 1))
        self.assertEqual(m.scan(self.reports, self.admission)['counts']['oversize'], 1)
        self.report(self.ips())
        with patch.object(m, 'regular', side_effect=m.Refused('changed')):
            self.assertEqual(m.scan(self.reports, self.admission)['counts']['changed'], 1)
        path.unlink()
        path.hardlink_to(self.base / 'result.json')
        self.assertEqual(m.scan(self.reports, self.admission)['counts']['link'], 1)

    def test_unknown_values_and_untrusted_image_paths_stay_other(self):
        private = 'generated-account-error-description'
        self.report(self.ips(exception=dict(signal=private), usedImages=[dict(name=private, path='/Users/' + private)]))
        value = m.validate_public(m.scan(self.reports, self.admission))
        self.assertEqual(value['reports'], [dict(process='xcodebuild', signal='other', library='other')])
        self.assertNotIn(private, json.dumps(value))
        self.assertEqual(m.library('AppKit', '/Users/generated/AppKit'), 'other')

    def test_honest_absence_and_malformed_faulting_image(self):
        value = m.validate_public(m.scan(self.reports, self.admission))
        self.assertEqual(value['outcome'], 'no-evidence')
        self.assertEqual(value['counts']['candidates'], 0)
        for changes in (dict(faultingThread=4), dict(threads=[None, None]),
                        dict(usedImages=[None]), dict(exception=[])):
            self.report(self.ips(**changes))
            self.assertEqual(m.scan(self.reports, self.admission)['outcome'], 'refused')

    def test_raw_crash_uses_faulting_address_and_binary_image(self):
        data = b'''Process: xcodebuild [456]
Date/Time: 2026-01-01 01:00:30 +0000
Crashed Thread: 2
Exception Type: EXC_CRASH (SIGABRT)
Thread 2 Crashed:
0 libsystem_kernel.dylib 0x1234 generated_private_symbol
Binary Images:
0x1000 - 0x1fff libsystem_kernel.dylib arm64 <generated> /usr/lib/system/libsystem_kernel.dylib
'''
        self.assertEqual(m.crash(data)[2:], ('SIGABRT', 'libsystem_kernel.dylib'))

    def test_changed_original_pin_refuses_before_transport(self):
        import io
        import sys
        admission = m.admit(str(self.root), self.run, self.suite)
        with (self.base / 'log.txt').open('a') as out:
            out.write('changed\n')
        context = json.dumps(dict(admission=admission, selection=dict(ssh='/generated/never-run')))
        with patch.object(sys, 'argv', ['helper', '--collect', str(self.root), self.run, self.suite]), \
                patch.object(m.os, 'fdopen', return_value=io.StringIO(context)), \
                patch.object(m, 'collect') as transport, \
                patch('sys.stdout', new_callable=io.StringIO) as output:
            self.assertEqual(m.main(), 1)
            transport.assert_not_called()
            self.assertEqual(json.loads(output.getvalue())['category'], 'changed')

    def test_only_original_completed_guest_response_allows_release(self):
        admission = m.admit(str(self.root), self.run, self.suite)
        context = dict(selection=dict(ssh='/generated-wrapper'), private=str(self.root))
        owned = m.owned_cleanup()
        value = m.scan(self.reports, admission)
        safe = self.root / 'lease-safe.json'
        for failure in (owned.Cancelled(), owned.Refused(), owned.Uncertain(),
                        subprocess.TimeoutExpired('generated', 60)):
            with patch.object(m, 'owned_cleanup', return_value=owned), \
                    patch.object(owned, 'command', side_effect=failure):
                with self.assertRaises((m.Refused, subprocess.TimeoutExpired)):
                    m.collect(context, admission)
            self.assertFalse(safe.exists())
        # A prior, unrelated join and successful local transport cleanup are
        # insufficient when this invocation's returned guest facts are invalid.
        (self.root / 'receiver-joined.json').write_text('{"joined":true}')
        with patch.object(m, 'owned_cleanup', return_value=owned), \
                patch.object(owned, 'command', return_value=(0, b'{"private":"generated"}')):
            with self.assertRaises(m.Refused):
                m.collect(context, admission)
        self.assertFalse(safe.exists())
        with patch.object(m, 'owned_cleanup', return_value=owned), \
                patch.object(owned, 'command', return_value=(0, json.dumps(value).encode())):
            self.assertEqual(m.collect(context, admission), value)
        self.assertEqual(json.loads(safe.read_text()), {'safe': True})

    def test_generated_transport_descendants_cancel_and_timeout_without_release(self):
        # Local generated transport only. An inherited pipe proves the original
        # TERM-ignoring descendant is gone, even if the immediate child exited.
        for mode in ('cancel', 'timeout'):
            with self.subTest(mode=mode):
                private = self.root / mode
                private.mkdir()
                ready, driver = private / 'ready', private / 'driver.py'
                fixture = private / 'transport.py'
                fixture.write_text('''import os, signal, sys
from pathlib import Path
sys.stdin.buffer.read()
if os.fork() == 0:
    signal.signal(signal.SIGTERM, signal.SIG_IGN)
    os.write(int(sys.argv[2]), b'live')
    Path(sys.argv[1]).touch()
    while True: signal.pause()
os.wait()
''')
                reader, writer = os.pipe()
                try:
                    driver.write_text(
                        'import importlib.util,sys\n'
                        's=importlib.util.spec_from_file_location("crash",' + repr(SPEC.origin) + ')\n'
                        'm=importlib.util.module_from_spec(s);s.loader.exec_module(m)\n'
                        'owned=m.owned_cleanup();owned.SHUTDOWN=0.1\n'
                        'original=owned.command\n'
                        'def command(argv,**kw):\n'
                        ' kw["timeout"]=' + ('0.5' if mode == 'timeout' else '10') + '\n'
                        ' return original([sys.executable,' + repr(str(fixture)) + ',' + repr(str(ready)) + ',sys.argv[1]],pass_fds=(int(sys.argv[1]),),**kw)\n'
                        'owned.command=command;m.owned_cleanup=lambda:owned\n'
                        'try: m.collect({"selection":{"ssh":"generated"},"private":' + repr(str(private)) + '},{"pid":456,"start":1,"end":2,"exact_one":False})\n'
                        'except m.Refused: sys.exit(23)\n')
                    child = subprocess.Popen([sys.executable, '-B', str(driver), str(writer)], pass_fds=(writer,))
                    os.close(writer)
                    writer = None
                    try:
                        deadline = time.monotonic() + 5
                        while not ready.exists():
                            self.assertLess(time.monotonic(), deadline)
                            time.sleep(0.01)
                        self.assertEqual(os.read(reader, 4), b'live')
                        if mode == 'cancel':
                            child.send_signal(signal.SIGHUP)
                        self.assertEqual(child.wait(timeout=5), 23)
                        self.assertTrue(select.select([reader], [], [], 5)[0])
                        self.assertEqual(os.read(reader, 1), b'')
                        self.assertFalse((private / 'lease-safe.json').exists())
                    finally:
                        if child.poll() is None:
                            child.kill()
                        child.wait(timeout=5)
                finally:
                    os.close(reader)
                    if writer is not None:
                        os.close(writer)

    def test_transport_cannot_add_private_fields_or_false_absence(self):
        value = m.scan(self.reports, self.admission)
        for changed in (dict(value, path='/Users/generated'), dict(value, outcome='observed')):
            with self.assertRaises(m.Refused):
                m.validate_public(changed)


if __name__ == '__main__':
    unittest.main()
