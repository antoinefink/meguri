#!/usr/bin/env python3
"""Synthetic generated build inputs and fake PUBLIC harness outputs only."""
import argparse
import copy
import ctypes
import io
import hashlib
import importlib.util
import json
import os
import random
from pathlib import Path
import shutil
import sqlite3
import subprocess
import sys
import struct
import zipfile
import tempfile
import unittest
import unicodedata
from unittest import mock

REPO = Path(__file__).absolute().parents[2]
HELPER = REPO / 'tools/scripts/package-interop-check.py'
WRAPPER = REPO / 'tools/mg.d/package-interop-check.sh'
spec = importlib.util.spec_from_file_location('package_interop', HELPER)
interop = importlib.util.module_from_spec(spec)
spec.loader.exec_module(interop)
PIN = {'anki_version': '26.9.3', 'lock_sha256': 'a' * 64}
NOW = '2030-01-15T10:00:00Z'
NATIVE_NOW = '2030-01-15T10:00:00+01:00'
NATIVE_TZ = 'Europe/Paris'


def dump_db(path):
    conn = sqlite3.connect(path)
    collation_invoked = False
    def refuse_comparison(left, right):
        nonlocal collation_invoked
        collation_invoked = True
        raise AssertionError('synthetic dump requested collation')
    try:
        conn.create_collation('unicase', refuse_comparison)
        result = {}
        for table in interop.COLUMNS:
            cursor = conn.execute('SELECT * FROM ' + table + ' NOT INDEXED')
            names = [x[0] for x in cursor.description]
            rows = {}
            for values in cursor:
                row = {k: {'sha256': hashlib.sha256(v).hexdigest()} if isinstance(v, bytes)
                       else v for k, v in zip(names, values)}
                parts = {'graves': ('oid', 'type'), 'fields': ('ntid', 'ord'),
                         'templates': ('ntid', 'ord'), 'tags': ('tag',),
                         'config': ('KEY',)}.get(table, ('id',))
                rows[','.join(str(row[p]) for p in parts)] = row
            result[table] = rows
        return result
    finally:
        conn.close()
        if collation_invoked:
            raise AssertionError('synthetic dump requested collation')


def outputs(run):
    work = run / 'interop'
    manifest = json.loads((work / 'manifest.json').read_text())
    sc = json.loads((work / 'scenarios/rf4/package-interop.json').read_text())
    before = dump_db(work / 'fixtures/collection.anki2')
    # Synthetic fixtures' independently known clocks. Fake client evidence
    # must also exist when decode refuses: never borrow its expected dump/proof.
    if manifest['tz'] == NATIVE_TZ:
        timing = {'days_elapsed': 0, 'now': 1894698000, 'utc_offset_secs': 3600,
                  'next_day_at': 1894762800}
    else:
        timing = {'days_elapsed': 0, 'now': 1894701600, 'utc_offset_secs': 0,
                  'next_day_at': 1894766400}
    after = copy.deepcopy(before)
    for stamp in ('mod', 'scm'):
        after['col']['1'][stamp] = manifest['now'] * 1000
    meta = dict(PIN, driver_sha256='d' * 64)
    golden = {'scenario_id': interop.ID,
              'scenario_sha256': hashlib.sha256(interop.canonical(sc).encode()).hexdigest(),
              'oracle': meta, 'clock': {'start': manifest['start'], 'mode': 'frozen'},
              'tz': manifest['tz'], 'observations': {'steps': [{}, {'db_dump': before, 'timing': timing},
                  {}, {'db_dump': after, 'timing': copy.deepcopy(timing)},
                  {'media_dir': manifest['media']}]}}
    report = {'command': 'record', 'oracle': meta, 'scenario_count': 1,
              'totals': {'recorded': 1}, 'jobs': 1, 'wall_s': 0.1,
              'scenarios': [{'scenario': interop.ID, 'status': 'recorded'}]}
    return report, golden


def save_outputs(run, report, golden):
    work = run / 'interop'
    area = work / 'goldens/rf4'
    area.mkdir(exist_ok=True)
    (area / 'package-interop.json').write_text(json.dumps(golden))
    (work / 'out/report.json').write_text(json.dumps(report))


def raw_zstd(data):
    # RFC 8878 single segment, 4-byte size, raw blocks; no external maker.
    frame = bytearray(b'\x28\xb5\x2f\xfd\xa0' + struct.pack('<I', len(data)))
    for pos in range(0, max(1, len(data)), 128 * 1024):
        block = data[pos:pos + 128 * 1024]
        last = pos + len(block) == len(data)
        frame.extend(((len(block) << 3) | int(last)).to_bytes(3, 'little'))
        frame.extend(block)
    return bytes(frame)


def compressed_zstd(data):
    # Public libzstd compressor used solely to synthesize compressed test input.
    lib = ctypes.CDLL('libzstd.so.1')
    lib.ZSTD_compressBound.argtypes = [ctypes.c_size_t]
    lib.ZSTD_compressBound.restype = ctypes.c_size_t
    lib.ZSTD_compress.argtypes = [ctypes.c_void_p, ctypes.c_size_t, ctypes.c_void_p,
                                  ctypes.c_size_t, ctypes.c_int]
    lib.ZSTD_compress.restype = ctypes.c_size_t
    target = ctypes.create_string_buffer(lib.ZSTD_compressBound(len(data)))
    source = ctypes.create_string_buffer(data)
    length = lib.ZSTD_compress(target, len(target), source, len(data), 3)
    return target.raw[:length]


def native_frame(source, destination, length):
    """Synthetic P15 frame via public streaming API, fixed buffers even for >100 MiB."""
    lib = ctypes.CDLL('libzstd.so.1')
    class Buffer(ctypes.Structure):
        _fields_ = [('data', ctypes.c_void_p), ('size', ctypes.c_size_t), ('pos', ctypes.c_size_t)]
    lib.ZSTD_createCCtx.restype = ctypes.c_void_p
    lib.ZSTD_freeCCtx.argtypes = [ctypes.c_void_p]
    lib.ZSTD_CCtx_setParameter.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_int]
    lib.ZSTD_CCtx_setParameter.restype = ctypes.c_size_t
    lib.ZSTD_CCtx_setPledgedSrcSize.argtypes = [ctypes.c_void_p, ctypes.c_ulonglong]
    lib.ZSTD_CCtx_setPledgedSrcSize.restype = ctypes.c_size_t
    lib.ZSTD_compressStream2.argtypes = [ctypes.c_void_p, ctypes.POINTER(Buffer),
                                       ctypes.POINTER(Buffer), ctypes.c_int]
    lib.ZSTD_compressStream2.restype = ctypes.c_size_t
    lib.ZSTD_isError.argtypes = [ctypes.c_size_t]
    lib.ZSTD_isError.restype = ctypes.c_uint
    def checked(result):
        if lib.ZSTD_isError(result):
            raise AssertionError('synthetic frame maker failed')
        return result
    ctx = lib.ZSTD_createCCtx()
    assert ctx
    try:
        checked(lib.ZSTD_CCtx_setParameter(ctx, 100, 3))  # compression level
        checked(lib.ZSTD_CCtx_setParameter(ctx, 201, 1))  # checksum
        checked(lib.ZSTD_CCtx_setPledgedSrcSize(ctx, length))
        output = ctypes.create_string_buffer(64 * 1024)
        consumed = 0
        while True:
            chunk = source.read(min(64 * 1024, length - consumed))
            consumed += len(chunk)
            assert chunk or consumed == length
            data = ctypes.create_string_buffer(chunk)
            incoming = Buffer(ctypes.cast(data, ctypes.c_void_p), len(chunk), 0)
            ending = consumed == length
            while True:
                outgoing = Buffer(ctypes.cast(output, ctypes.c_void_p), len(output), 0)
                remaining = checked(lib.ZSTD_compressStream2(ctx, ctypes.byref(outgoing),
                                                            ctypes.byref(incoming), 2 if ending else 0))
                destination.write(output.raw[:outgoing.pos])
                if incoming.pos == incoming.size and (not ending or remaining == 0):
                    break
            if ending:
                break
    finally:
        lib.ZSTD_freeCCtx(ctx)


def native_zstd(data):
    target = io.BytesIO()
    native_frame(io.BytesIO(data), target, len(data))
    return target.getvalue()


def varint(value):
    result = bytearray()
    while value >= 128:
        result.append((value & 127) | 128)
        value >>= 7
    result.append(value)
    return bytes(result)


def message_field(number, data):
    return varint((number << 3) | 2) + varint(len(data)) + data


def map_record(name, length, digest):
    return message_field(1, name.encode('utf-8')) + b'\x10' + varint(length) + message_field(3, digest)


def media_map(records):
    return b''.join(message_field(1, map_record(name, length, digest))
                    for name, length, digest in records)


class Fixture(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='package-interop-')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.expected = self.root / 'build/generated/frozen'
        self.media = self.expected / 'collection.media'
        self.media.mkdir(parents=True)
        self.package = self.root / 'build/generated/native.colpkg'

        self.db = self.expected / 'collection.anki2'
        conn = sqlite3.connect(self.db)
        for table, columns in interop.COLUMNS.items():
            names = columns.split()
            conn.execute('CREATE TABLE ' + table + ' (' + ','.join('"' + n + '"' for n in names) + ')')
            row = {n: 0 for n in names}
            for n in ('id', 'oid', 'ntid'):
                if n in row:
                    row[n] = 1
            if table == 'col':
                row.update(crt=1894665600, ver=18, mod=123, scm=456,
                           conf='', models='', decks='', dconf='', tags='')
            if table == 'notes':
                row.update(guid='synthetic-fixed-guid', flds='synthetic text', sfld=1.0,
                           tags='', data='{}')
            if table == 'cards':
                row.update(due=50, reps=12, queue=2, data='{"synthetic":true}')
            if table in ('fields', 'templates', 'notetypes', 'deck_config', 'tags'):
                row['config'] = b'\x48\x01\x00synthetic-blob'
            if table == 'decks':
                row.update(common=b'common', kind=b'kind')
            if table == 'tags':
                row['tag'] = 'synthetic'
            if 'name' in row:
                row['name'] = 'Synthetic'
            if table != 'config':
                conn.execute('INSERT INTO ' + table + ' VALUES (' + ','.join('?' for _ in names) + ')',
                             [row[n] for n in names])
        for key, value in {'creationOffset': 0, 'rollover': 4, 'schedVer': 2}.items():
            conn.execute('INSERT INTO config VALUES (?, 0, 0, ?)', (key, json.dumps(value).encode()))
        conn.commit()
        conn.close()
        (self.media / 'referenced.bin').write_bytes(b'synthetic payload')
        self.pack()
        self.run = self.root / 'build/mg/package-interop-check/unit'
        self.run.mkdir(parents=True)
        (self.root / 'conformance').mkdir()
        (self.root / 'conformance/oracle.json').write_text(json.dumps(PIN))
        self.args = argparse.Namespace(command='stage', root=str(self.root), run=str(self.run),
            package=str(self.package), expected=str(self.expected), now=NOW, tz='UTC', validate_only=False)

    def pack(self, data=None, frame=None):
        records = []
        files = sorted(self.media.iterdir(),
                       key=lambda p: unicodedata.normalize('NFC', p.name).encode('utf-8'))
        with zipfile.ZipFile(self.package, 'w', compression=zipfile.ZIP_STORED) as archive:
            archive.writestr('meta', b'\x08\x03')
            archive.writestr('collection.anki21b', frame if frame is not None else
                             native_zstd(self.db.read_bytes() if data is None else data))
            archive.writestr('collection.anki2', b'SQLite format 3\0' + bytes(100))
            for index, path in enumerate(files):
                digest = hashlib.sha1()
                with path.open('rb') as source:
                    while True:
                        chunk = source.read(64 * 1024)
                        if not chunk:
                            break
                        digest.update(chunk)
                length = path.stat().st_size
                records.append((unicodedata.normalize('NFC', path.name), length, digest.digest()))
                with path.open('rb') as source, archive.open(str(index), 'w') as target:
                    native_frame(source, target, length)
            archive.writestr('media', native_zstd(media_map(records)))

    def decode(self):
        work = self.run / 'interop'
        interop.decode(argparse.Namespace(package=str(work / 'fixtures/package.colpkg'),
            expected=str(work / 'fixtures/collection.anki2'), output=str(work / 'decoded'),
            media_inventory=str(work / 'media-inventory.json'), now=self.args.now, tz=self.args.tz))

    def native_clock(self):
        # Configure only this test's temporary synthetic inputs, before capture.
        # Never access or adapt the existing native artifact for a test.
        conn = sqlite3.connect(self.db)
        conn.execute('UPDATE col SET crt=1894676400')  # D3.1: Jan 15 04:00 +01.
        conn.execute('UPDATE config SET val=? WHERE KEY=?', (b'-60', 'creationOffset'))
        conn.commit()
        conn.close()
        self.pack()
        self.args.now, self.args.tz = NATIVE_NOW, NATIVE_TZ

    def stage(self):
        interop.stage(self.args)
        if not self.args.validate_only:
            self.decode()
        return self.run / 'interop'

    def verify(self, command='check'):
        interop.verify(argparse.Namespace(command=command, root=str(self.root), run=str(self.run)))

    def valid_outputs(self):
        self.stage()
        report, golden = outputs(self.run)
        save_outputs(self.run, report, golden)
        return report, golden

    def rejects(self, call, code=None):
        with self.assertRaises((interop.Refusal, OSError, ValueError, KeyError, TypeError, sqlite3.Error)) as error:
            call()
        if code:
            self.assertIn(code, str(error.exception))

    def assert_bad_outputs(self, mutate):
        report, golden = self.valid_outputs()
        mutate(report, golden)
        save_outputs(self.run, report, golden)
        self.rejects(self.verify)


class HostCompatibility(Fixture):
    def test_host_cli_stage_ready_check_without_sqlite_311_apis(self):
        # This host stub lacks Connection.setlimit and SQLITE_LIMIT_*.
        # Any host SQLite connection must fail this check immediately.
        class Python39SQLite:
            Error = sqlite3.Error
            class Connection:
                pass
            @staticmethod
            def connect(*args, **kwargs):
                raise AssertionError('host attempted SQLite access')
        self.assertFalse(hasattr(Python39SQLite.Connection, 'setlimit'))
        self.assertFalse(hasattr(Python39SQLite, 'SQLITE_LIMIT_LENGTH'))

        def host(command, extra=()):
            argv = [str(HELPER), command, '--root', str(self.root), '--run', str(self.run), *extra]
            with mock.patch.object(interop, 'sqlite3', Python39SQLite), \
                 mock.patch.object(sys, 'argv', argv), mock.patch.object(sys, 'stderr', io.StringIO()):
                return interop.main()

        extra = ['--package', str(self.package), '--expected', str(self.expected),
                 '--now', NOW, '--tz', 'UTC']
        self.assertEqual(host('stage', extra + ['--validate-only']), 0)
        self.assertFalse((self.run / 'interop').exists())
        self.assertEqual(host('stage', extra), 0)
        self.assertEqual(host('ready'), 1)  # No independent proof yet.
        self.decode()  # Actual independent decoder; real bounded SQLite API.
        self.assertEqual(host('ready'), 0)
        report, golden = outputs(self.run)  # Separate fake client, no proof reuse.
        save_outputs(self.run, report, golden)
        self.assertEqual(host('ready'), 1)  # Client outputs are no longer fresh.
        bad = copy.deepcopy(golden)
        bad['observations']['steps'][3]['db_dump']['notes']['1']['sfld'] = 1
        save_outputs(self.run, report, bad)
        self.assertEqual(host('check'), 1)  # Integer differs from expected real.
        save_outputs(self.run, report, golden)
        self.assertEqual(host('check'), 0)
        package = self.run / 'interop/fixtures/package.colpkg'
        package.chmod(0o600)
        package.write_bytes(b'replaced')
        self.assertEqual(host('check'), 1)

    def test_host_ready_binds_decoder_inputs_dump_and_frozen_clock(self):
        work = self.stage()
        decoded = work / 'decoded'
        neutral = json.loads((decoded / 'expected.json').read_text())
        self.assertTrue(interop.exact(neutral, dump_db(self.db)))
        proof_path = decoded / 'proof.json'
        proof = json.loads(proof_path.read_text())
        self.assertEqual(proof['expected_sha256'], hashlib.sha256(self.db.read_bytes()).hexdigest())
        self.assertEqual(proof['pre_client_stamp_edits'], 0)
        for field, value in [('package_sha256', 'b' * 64), ('expected_sha256', 'b' * 64),
                             ('decoded_sha256', 'b' * 64), ('raw_sha256', 'b' * 64),
                             ('start', '2030-01-14T10:00:00'), ('now', 1894701601),
                             ('tz', NATIVE_TZ), ('utc_offset_secs', 3600),
                             ('pre_client_stamp_edits', 1),
                             ('pre_client_database_equal', False),
                             ('pre_client_media_equal', False),
                             ('expected_media_inventory_sha256', 'b' * 64),
                             ('decoded_media_map_sha256', 'b' * 64)]:
            with self.subTest(field=field):
                proof_path.chmod(0o600)
                proof_path.write_text(json.dumps(dict(proof, **{field: value})))
                self.rejects(lambda: self.verify('ready'), 'decoder-proof-changed')
        proof_path.write_text(json.dumps(proof))
        raw_path = decoded / 'expected.json'
        raw_path.chmod(0o600)
        neutral['notes']['1']['guid'] = 'changed-neutral'
        raw_path.write_text(json.dumps(neutral))
        self.rejects(lambda: self.verify('ready'), 'decoder-proof-changed')


class NativeClock(Fixture):
    def setUp(self):
        super().setUp()
        self.native_clock()

    def prepare_media_baseline(self):
        output = self.run / 'native-media-baseline'
        output.mkdir()
        interop.prepare_native_media(argparse.Namespace(expected=str(self.expected), output=str(output)))
        return output

    def test_native_media_preparation_exact_one_blob_row_stamps_and_parent_media_unchanged(self):
        (self.expected / 'manifest.json').write_text('{"dump_sha256":"parent-only"}')
        before = {p.relative_to(self.expected).as_posix(): p.read_bytes()
                  for p in self.expected.rglob('*') if p.is_file()}
        parent_rows = list(interop.raw_rows(self.db))
        output = self.prepare_media_baseline()
        prepared = output / 'fixture'
        record = json.loads((output / 'preparation.json').read_text())
        inserted = ('config', 'localOffset', [('text', 'localOffset'), ('integer', -1),
                    ('integer', 1894676400), ('blob', b'-60')])
        rows = list(interop.raw_rows(prepared / 'collection.anki2'))
        self.assertEqual([r for r in rows if r[:2] == ('config', 'localOffset')], [inserted])
        self.assertEqual([r for r in rows if r[:2] != ('config', 'localOffset')], parent_rows)
        self.assertEqual(before, {p.relative_to(self.expected).as_posix(): p.read_bytes()
                                  for p in self.expected.rglob('*') if p.is_file()})
        for name, data in before.items():
            self.assertEqual(record['parent_files'][name],
                             {'sha256': hashlib.sha256(data).hexdigest(), 'bytes': len(data)})
            if name != 'collection.anki2':
                self.assertEqual((prepared / name).read_bytes(), data)
                self.assertEqual(record['parent_files'][name], record['prepared_files'][name])
        self.assertEqual(record['clock'], {'now': NATIVE_NOW, 'tz': NATIVE_TZ})
        self.assertEqual(record['inserted_row'], {'KEY': 'localOffset', 'usn': -1,
            'mtime_secs': 1894676400, 'val_hex': '2d3630', 'storage_class': 'blob'})
        self.assertFalse(any(p.name.endswith(('-wal', '-shm', '-journal')) for p in prepared.rglob('*')))
        self.assertFalse((prepared / 'preparation.json').exists())

    def test_native_media_prepared_expected_keeps_strict_independent_package_comparison(self):
        (self.expected / 'manifest.json').write_text('{}')
        prepared = self.prepare_media_baseline() / 'fixture'
        # The independent expected input is exactly the prepared copy. Synthetic
        # package construction happens before proof; no expected data comes back
        # from the decoded package or a client.
        self.expected = prepared
        self.media = prepared / 'collection.media'
        self.db = prepared / 'collection.anki2'
        self.args.expected = str(prepared)
        self.pack()
        self.run = self.run.with_name('unit-proof')
        self.run.mkdir(parents=True)
        self.args.run = str(self.run)
        work = self.stage()
        interop.compare_databases(self.db, work / 'decoded/collection.anki2')
        parent = self.root / 'build/generated/frozen/collection.anki2'
        self.rejects(lambda: interop.compare_databases(self.db, parent), 'pre-client-database-difference')
        for sql in ("UPDATE config SET val='-60' WHERE KEY='localOffset'",
                    "UPDATE config SET usn=0 WHERE KEY='localOffset'",
                    "UPDATE config SET mtime_secs=mtime_secs+1 WHERE KEY='localOffset'"):
            with self.subTest(sql=sql):
                decoded = work / 'decoded/collection.anki2'
                decoded.chmod(0o600)
                shutil.copyfile(self.db, decoded)
                conn = sqlite3.connect(decoded)
                conn.execute(sql)
                conn.commit()
                conn.close()
                self.rejects(lambda: interop.compare_databases(self.db, decoded),
                             'pre-client-database-difference')

    def test_native_media_preparation_refuses_present_key_and_every_sidecar(self):
        (self.expected / 'manifest.json').write_text('{}')
        for suffix in ('-wal', '-shm', '-journal'):
            with self.subTest(sidecar=suffix):
                sidecar = Path(str(self.db) + suffix)
                sidecar.write_bytes(b'pending')
                self.rejects(self.prepare_media_baseline, 'sqlite-sidecar')
                self.assertFalse((self.run / 'native-media-baseline/preparation.json').exists())
                shutil.rmtree(self.run / 'native-media-baseline')
                sidecar.unlink()
        for value in (b'-60', '-60', b'0'):
            with self.subTest(present_value=value):
                conn = sqlite3.connect(self.db)
                conn.execute('INSERT INTO config VALUES (?, -1, 1894676400, ?)', ('localOffset', value))
                conn.commit()
                conn.close()
                parent = self.db.read_bytes()
                self.rejects(self.prepare_media_baseline, 'native-local-offset-present')
                self.assertEqual(self.db.read_bytes(), parent)
                self.assertFalse((self.run / 'native-media-baseline/preparation.json').exists())
                shutil.rmtree(self.run / 'native-media-baseline')
                conn = sqlite3.connect(self.db)
                conn.execute('DELETE FROM config WHERE KEY=?', ('localOffset',))
                conn.commit()
                conn.close()

    def test_native_media_preparation_refuses_other_cell_class_stamp_or_media_tamper(self):
        (self.expected / 'manifest.json').write_text('{}')
        original = interop.raw_rows
        for change in ('cell', 'class', 'stamp', 'inserted', 'media'):
            with self.subTest(change=change):
                calls = 0
                def tamper(path):
                    nonlocal calls
                    if path == self.run / 'native-media-baseline/fixture/collection.anki2':
                        calls += 1
                        if calls == 2:
                            if change == 'media':
                                (path.parent / 'collection.media/referenced.bin').write_bytes(b'tampered')
                            else:
                                conn = sqlite3.connect(path)
                                sql = {'cell': 'UPDATE cards SET reps=reps+1',
                                       'class': 'UPDATE notes SET sfld=CAST(sfld AS TEXT)',
                                       'stamp': 'UPDATE col SET mod=mod+1',
                                       'inserted': "UPDATE config SET val='-60' WHERE KEY='localOffset'"}[change]
                                conn.execute(sql)
                                conn.commit()
                                conn.close()
                    return original(path)
                with mock.patch.object(interop, 'raw_rows', side_effect=tamper):
                    self.rejects(self.prepare_media_baseline,
                        'native-preparation-other-file-changed' if change == 'media' else
                        'native-preparation-row' if change == 'inserted' else 'native-preparation-other-cell-changed')
                self.assertFalse((self.run / 'native-media-baseline/preparation.json').exists())
                shutil.rmtree(self.run / 'native-media-baseline')

    def test_exact_native_full_proof_and_source_immutability(self):
        source = interop.open_dir(self.expected)
        try:
            before = interop.scan_tree(source, digest=True)
            package_hash = hashlib.sha256(self.package.read_bytes()).hexdigest()
            package_stat = interop.signature(self.package.stat())
            work = self.stage()
            # Stage and host checks consume neutral evidence without SQLite.
            with mock.patch.object(interop, 'sqlite_reader', side_effect=AssertionError('host SQLite')):
                self.verify('ready')
                report, golden = outputs(self.run)
                save_outputs(self.run, report, golden)
                self.verify()
            for name in ('manifest.json', 'decoded/proof.json', 'evidence.json'):
                value = json.loads((work / name).read_text())
                self.assertEqual(value['tz'], 'Europe/Paris')
                self.assertEqual(value['utc_offset_secs'], 3600)
                self.assertEqual(value['start'], NATIVE_NOW)
                self.assertEqual(value['now'], 1894698000)
            scenario = json.loads((work / 'scenarios/rf4/package-interop.json').read_text())
            self.assertEqual(scenario['clock'],
                {'tz': 'Europe/Paris', 'start': NATIVE_NOW, 'mode': 'frozen'})
            self.assertEqual(set(scenario), {'id', 'clock', 'steps'})  # No data/config rewrites.
            for i in (1, 3):
                self.assertEqual(scenario['steps'][i],
                    {'observe': [{'db_dump': {'raw': True}}, 'timing']})
            evidence = json.loads((work / 'evidence.json').read_text())
            self.assertEqual(evidence['tables'], 12)
            self.assertEqual(evidence['pre_client_stamp_edits'], 0)
            self.assertEqual(evidence['oracle_open_edits'], 0)
            self.assertEqual(evidence['import_col_mod_scm_ms'], 1894698000000)
            self.assertEqual(before, interop.scan_tree(source, digest=True))
            self.assertEqual(package_hash, hashlib.sha256(self.package.read_bytes()).hexdigest())
            self.assertEqual(package_stat, interop.signature(self.package.stat()))
        finally:
            os.close(source)

    def test_exact_admission_and_retained_utc_spellings(self):
        self.assertEqual(interop.clock(NATIVE_NOW, NATIVE_TZ), (NATIVE_NOW, 1894698000))
        self.assertEqual(interop.clock_offset(1894698000, NATIVE_TZ), 3600)
        for spelling in (NOW, '2030-01-15T10:00:00+00:00', '2030-01-15T10:00:00'):
            self.assertEqual(interop.clock(spelling, 'UTC'), ('2030-01-15T10:00:00', 1894701600))
        for spelling in ('2000-01-01T00:00:00Z', '2037-12-31T23:59:59Z'):
            self.assertEqual(interop.clock_offset(interop.clock(spelling, 'UTC')[1], 'UTC'), 0)
        for now, tz in [(NATIVE_NOW, 'UTC'), (NOW, NATIVE_TZ),
                        ('2030-01-15T09:00:00Z', NATIVE_TZ),  # Same instant, wrong local offset.
                        ('2030-01-15T10:00:00+02:00', NATIVE_TZ),
                        ('2030-01-15T10:00:00-01:00', NATIVE_TZ),
                        ('2030-01-15T10:00:01+01:00', NATIVE_TZ),
                        ('2030-01-16T10:00:00+01:00', NATIVE_TZ),
                        ('2030-07-15T10:00:00+01:00', NATIVE_TZ),
                        ('2030-01-15T10:00:00.0+01:00', NATIVE_TZ),
                        ('2030-01-15T10:00:00', NATIVE_TZ),
                        (NATIVE_NOW, 'Europe/Rome'), (NATIVE_NOW, 'Etc/GMT-1')]:
            with self.subTest(now=now, tz=tz):
                self.rejects(lambda: interop.clock(now, tz), 'clock-scope')
        self.rejects(lambda: interop.clock_offset(1894698001, NATIVE_TZ), 'clock-scope')
        # UTC still refuses a nonzero collection creation offset.
        self.rejects(lambda: interop.timing_for(self.db, 1894701600), 'collection-clock-scope')

    def test_decoder_independently_refuses_wrong_clock_scope_and_requires_tz(self):
        interop.stage(self.args)
        for now, tz in [(NOW, NATIVE_TZ), (NATIVE_NOW, 'UTC'),
                        ('2030-01-16T10:00:00+01:00', NATIVE_TZ), (NATIVE_NOW, 'Europe/Rome')]:
            self.args.now, self.args.tz = now, tz
            self.rejects(self.decode, 'clock-scope')
            self.assertFalse((self.run / 'interop/decoded/proof.json').exists())
        with mock.patch.object(sys, 'argv', [str(HELPER), 'decode', '--package', 'unused',
                '--expected', 'unused', '--output', 'unused', '--media-inventory', 'unused',
                '--now', NATIVE_NOW]), mock.patch.object(sys, 'stderr', io.StringIO()):
            with self.assertRaises(SystemExit) as error:
                interop.main()
        self.assertEqual(error.exception.code, 2)

    def test_d3_local_creation_date_rollover_boundary_and_clamp(self):
        # Expected epoch seconds below are independent fixed D3 calendar examples.
        # Includes the local creation midnight (23:00 UTC), exact R==now, and h=0/23.
        cases = [(1894676400, 4, 0, 1894762800),  # D3.1 creation date.
                 (1894590000, 4, 1, 1894762800),  # Created Jan 14 at 04:00 Paris.
                 (1894661999, 4, 1, 1894762800),  # Jan 14 23:59:59 Paris.
                 (1894662000, 4, 0, 1894762800),  # Jan 15 00:00 Paris.
                 (1894663800, 4, 0, 1894762800),  # Jan 15 00:30 Paris (Jan 14 UTC).
                 (1894590000, 0, 1, 1894748400),
                 (1894590000, 9, 1, 1894780800),
                 (1894590000, 10, 1, 1894784400),  # At rollover: next day, never now.
                 (1894590000, 11, 0, 1894701600),
                 (1894590000, 23, 0, 1894744800),
                 (1894676400, 11, 0, 1894701600),  # Negative day clamps to zero.
                 (1894698000, 4, 0, 1894762800)]  # Creation exactly now stays admitted.
        for crt, hour, day, boundary in cases:
            with self.subTest(crt=crt, hour=hour):
                conn = sqlite3.connect(self.db)
                conn.execute('UPDATE col SET crt=?', (crt,))
                conn.execute('UPDATE config SET val=? WHERE KEY=?',
                             (str(hour).encode(), 'rollover'))
                conn.commit()
                conn.close()
                self.assertEqual(interop.timing_for(self.db, 1894698000, NATIVE_TZ),
                    {'days_elapsed': day, 'now': 1894698000, 'utc_offset_secs': 3600,
                     'next_day_at': boundary})

    def test_native_config_schema_and_creation_bounds_still_refuse_before_open(self):
        original = self.db.read_bytes()
        cases = [('UPDATE config SET val=? WHERE KEY=?', (value, 'creationOffset'))
                 for value in (b'0', b'60', b'-120', b'-60.0', b'true', b'"-60"', b'null', '-60')]
        cases += [('UPDATE config SET val=? WHERE KEY=?', (value, 'rollover'))
                  for value in (b'-1', b'24', b'4.0', b'true')]
        cases += [('UPDATE config SET val=? WHERE KEY=?', (b'1', 'schedVer')),
                  ('DELETE FROM config WHERE KEY=?', ('creationOffset',)),
                  ('DELETE FROM config WHERE KEY=?', ('rollover',)),
                  ('DELETE FROM config WHERE KEY=?', ('schedVer',)),
                  ('UPDATE col SET ver=?', (17,)), ('UPDATE col SET ver=?', (19,)),
                  ('UPDATE col SET crt=?', (1894698001,)), ('UPDATE col SET crt=?', (-1,))]
        for sql, params in cases:
            with self.subTest(sql=sql, params=params):
                self.db.write_bytes(original)
                conn = sqlite3.connect(self.db)
                conn.execute(sql, params)
                conn.commit()
                conn.close()
                self.pack()
                self.rejects(self.stage)
                self.assertFalse((self.run / 'interop/decoded/proof.json').exists())
                shutil.rmtree(self.run / 'interop')

    def test_native_manifest_decoder_and_scenario_mismatches_refuse(self):
        work = self.stage()
        for filename in ('manifest.json', 'decoded/proof.json'):
            path = work / filename
            original = json.loads(path.read_text())
            path.chmod(0o600)
            for key, value in [('tz', 'UTC'), ('tz', 'Europe/Rome'), ('utc_offset_secs', 0),
                               ('utc_offset_secs', 3600.0), ('start', NOW),
                               ('start', '2030-01-16T10:00:00+01:00'), ('now', 1894701600)]:
                with self.subTest(filename=filename, key=key, value=value):
                    path.write_text(json.dumps(dict(original, **{key: value})))
                    self.rejects(lambda: self.verify('ready'))
            path.write_text(json.dumps(original))
        path = work / 'scenarios/rf4/package-interop.json'
        original = json.loads(path.read_text())
        path.chmod(0o600)
        for key, value in [('tz', 'UTC'), ('start', NOW), ('start', '2030-01-15T09:00:00Z')]:
            bad = copy.deepcopy(original)
            bad['clock'][key] = value
            path.write_text(json.dumps(bad))
            self.rejects(lambda: self.verify('ready'), 'scenario-changed')
        path.write_text(json.dumps(original))
        self.verify('ready')

    def test_native_proof_timing_and_raw_clock_value_binding(self):
        work = self.stage()
        path = work / 'decoded/proof.json'
        original = json.loads(path.read_text())
        path.chmod(0o600)
        for field in ('days_elapsed', 'now', 'utc_offset_secs', 'next_day_at'):
            bad = copy.deepcopy(original)
            bad['timing'][field] += 1  # Even an otherwise in-range day/boundary must fail.
            path.write_text(json.dumps(bad))
            self.rejects(lambda: self.verify('ready'), 'collection-clock-scope')
        for key, raw in [('creationOffset', b'0'), ('rollover', b'5'), ('schedVer', b'1')]:
            bad = copy.deepcopy(original)
            bad['clock_values'][key] = raw.hex()
            path.write_text(json.dumps(bad))
            self.rejects(lambda: self.verify('ready'), 'decoder-proof-changed')
        path.write_text(json.dumps(original))
        self.verify('ready')

    def test_native_actual_timings_and_import_stamps_use_selected_epoch(self):
        report, golden = self.valid_outputs()
        for field in ('days_elapsed', 'now', 'utc_offset_secs', 'next_day_at'):
            for indices in ((1,), (3,), (1, 3)):
                bad = copy.deepcopy(golden)
                for index in indices:
                    bad['observations']['steps'][index]['timing'][field] += 1
                save_outputs(self.run, report, bad)
                self.rejects(self.verify, 'timing-difference')
        for index in (1, 3):
            for stamp in ('mod', 'scm'):
                bad = copy.deepcopy(golden)
                bad['observations']['steps'][index]['db_dump']['col']['1'][stamp] = 1894701600000
                save_outputs(self.run, report, bad)
                self.rejects(self.verify, 'database-difference')
        for key, value in [('tz', 'UTC'), ('tz', 'Europe/Rome'),
                           ('clock', {'start': '2030-01-15T09:00:00Z', 'mode': 'frozen'})]:
            save_outputs(self.run, report, dict(golden, **{key: value}))
            self.rejects(self.verify, 'golden-identity')
        save_outputs(self.run, report, golden)
        self.verify()

    def test_native_full_raw_reviews_and_media_still_required(self):
        report, golden = self.valid_outputs()
        for index in (1, 3):
            bad = copy.deepcopy(golden)
            bad['observations']['steps'][index]['db_dump']['revlog'].clear()
            save_outputs(self.run, report, bad)
            self.rejects(self.verify, 'database-difference')
        bad = copy.deepcopy(golden)
        bad['observations']['steps'][4]['media_dir'] = {}
        save_outputs(self.run, report, bad)
        self.rejects(self.verify, 'media-difference')
        # A native clock never waives pre-client raw storage class/value equality.
        shutil.rmtree(self.run / 'interop')
        variant = self.root / 'build/generated/variant.anki2'
        shutil.copyfile(self.db, variant)
        conn = sqlite3.connect(variant)
        conn.execute('UPDATE revlog SET time=999')
        conn.commit()
        conn.close()
        self.pack(data=variant.read_bytes())
        self.rejects(self.stage, 'pre-client-database-difference')
        self.assertFalse((self.run / 'interop/decoded/proof.json').exists())


class Evidence(Fixture):
    def test_exact_and_only_declared_stamps(self):
        self.valid_outputs()
        self.verify()
        evidence = json.loads((self.run / 'interop/evidence.json').read_text())
        self.assertEqual(evidence['tables'], 12)
        self.assertTrue(evidence['database_equal'])
        self.assertTrue(evidence['media_equal'])
        self.assertNotIn('synthetic', json.dumps(evidence))
        self.assertEqual(evidence['media_files'], 1)
        self.assertTrue(evidence['pre_client_media_equal'])
        raw_map = (self.run / 'interop/decoded/media-map.bin').read_bytes()
        self.assertEqual(evidence['decoded_media_map_sha256'], hashlib.sha256(raw_map).hexdigest())
        imported = outputs(self.run)[1]['observations']['steps'][4]['media_dir']
        self.assertEqual(evidence['imported_media_inventory_sha256'],
                         hashlib.sha256(interop.canonical(imported).encode('utf-8')).hexdigest())
        self.assertNotEqual(evidence['decoded_media_map_sha256'],
                            evidence['imported_media_inventory_sha256'])
        self.assertNotIn('media_map_sha256', evidence)

    def test_wrong_import_stamp(self):
        self.assert_bad_outputs(lambda r, g: g['observations']['steps'][3]['db_dump']['col']['1']
                                .update(mod=123))

    def test_raw_guid_blob_scheduling_revlog_and_type_differences(self):
        report, golden = self.valid_outputs()
        mutations = [('notes', 'guid', '#random'), ('fields', 'config', {'sha256': 'b' * 64}),
                     ('templates', 'config', {'sha256': 'c' * 64}), ('cards', 'due', 51),
                     ('cards', 'reps', 11), ('revlog', 'time', 999), ('notes', 'sfld', 1),
                     ('cards', 'queue', True), ('col', 'usn', 1), ('notes', 'sfld', -0.0)]
        for table, column, value in mutations:
            with self.subTest(table=table, column=column):
                bad = copy.deepcopy(golden)
                rows = bad['observations']['steps'][3]['db_dump'][table]
                self.assertEqual(len(rows), 1)
                key = next(iter(rows))
                rows[key][column] = value
                save_outputs(self.run, report, bad)
                with self.assertRaises(interop.Refusal):
                    self.verify()
        for table in ('notes', 'revlog', 'cards'):
            bad = copy.deepcopy(golden)
            bad['observations']['steps'][3]['db_dump'][table].clear()
            save_outputs(self.run, report, bad)
            self.rejects(self.verify)

    def test_report_requires_one_actual_success(self):
        report, golden = self.valid_outputs()
        variants = [None, {}, dict(report, command='compare'), dict(report, scenario_count=0),
                    dict(report, scenario_count=True), dict(report, jobs=2),
                    dict(report, totals={'recorded': 1, 'error': 1}),
                    dict(report, scenarios=[]), dict(report, scenarios=report['scenarios'] * 2),
                    dict(report, scenarios=[{'scenario': 'other/id', 'status': 'recorded'}]),
                    dict(report, scenarios=[{'scenario': interop.ID, 'status': 'error'}]),
                    dict(report, error={'kind': 'internal_error'}), dict(report, oracle={})]
        for bad in variants:
            with self.subTest(report=bad):
                save_outputs(self.run, bad, golden)
                self.rejects(self.verify)
        path = self.run / 'interop/out/report.json'
        for raw in ('{', '{"command":"record","command":"record"}', '{"x":NaN}'):
            path.write_text(raw)
            self.rejects(self.verify)
        path.unlink()
        self.rejects(self.verify)

    def test_import_error_missing_duplicate_steps_and_incomplete_dump(self):
        report, golden = self.valid_outputs()
        mutations = [lambda steps: steps[2].update({'import': {'error': 'corrupt'}}),
                     lambda steps: steps.pop(), lambda steps: steps.append(steps[1]),
                     lambda steps: steps[1]['db_dump'].pop('graves'),
                     lambda steps: steps[3]['db_dump']['notes']['1'].pop('guid'),
                     lambda steps: steps[3].update(unexpected=True)]
        for mutate in mutations:
            bad = copy.deepcopy(golden)
            mutate(bad['observations']['steps'])
            save_outputs(self.run, report, bad)
            self.rejects(self.verify)

    def test_clock_id_hash_pin_and_timing(self):
        report, golden = self.valid_outputs()
        variants = [dict(golden, scenario_id='wrong/id'), dict(golden, tz='Europe/Paris'),
                    dict(golden, clock={'start': '2030-01-15T10:00:01', 'mode': 'frozen'}),
                    dict(golden, scenario_sha256='0' * 64), dict(golden, oracle=dict(PIN, driver_sha256='z'))]
        for field in ('days_elapsed', 'now', 'next_day_at', 'utc_offset_secs'):
            bad = copy.deepcopy(golden)
            bad['observations']['steps'][3]['timing'][field] += 1
            variants.append(bad)
            # Both observations being equally wrong is still refused.
            bad = copy.deepcopy(bad)
            bad['observations']['steps'][1]['timing'] = copy.deepcopy(bad['observations']['steps'][3]['timing'])
            variants.append(bad)
        for bad in variants:
            save_outputs(self.run, report, bad)
            self.rejects(self.verify)

    def test_exact_media_not_counts(self):
        report, golden = self.valid_outputs()
        variants = [{}, {'extra.bin': {'sha1': '0' * 40, 'size': 1}},
                    {'Referenced.bin': next(iter(golden['observations']['steps'][4]['media_dir'].values()))},
                    {'referenced.bin': {'size': 17, 'sha1': 'f' * 40}},
                    {'referenced.bin': {'dir': True}}]
        for media in variants:
            bad = copy.deepcopy(golden)
            bad['observations']['steps'][4]['media_dir'] = media
            save_outputs(self.run, report, bad)
            self.rejects(self.verify)

    def test_empty_media_directory(self):
        (self.media / 'referenced.bin').unlink()
        self.pack()
        self.valid_outputs()
        self.verify()

    def test_empty_unused_underscore_unicode_case_and_large_streaming(self):
        for name, data in {'empty.bin': b'', '_unused.bin': b'underscore',
                           'Unused.bin': b'unused', '\u96ea.bin': b'unicode',
                           'case.bin': b'lower', 'CASE.bin': b'upper'}.items():
            (self.media / name).write_bytes(data)
        with (self.media / 'large.bin').open('wb') as stream:
            # Incompressible generated bytes keep the existing ratio bound meaningful.
            rng = random.Random(42)
            remaining = 100 * 1024 * 1024 + 1
            while remaining:
                chunk = rng.randbytes(min(128 * 1024, remaining))
                stream.write(chunk)
                remaining -= len(chunk)
        self.pack()
        reads, digest_writes, archive_reads = [], [], []
        original = os.read
        def tracked(fd, size):
            reads.append(size)
            return original(fd, size)
        digest_write = interop.PayloadDigest.write
        archive_read = zipfile.ZipExtFile.read
        def counted(sink, data):
            digest_writes.append(len(data))
            return digest_write(sink, data)
        def bounded_read(source, size=-1):
            if source.name != 'meta':
                self.assertGreaterEqual(size, 0)
                self.assertLessEqual(size, 64 * 1024)
                archive_reads.append(size)
            return archive_read(source, size)
        with mock.patch.object(interop.os, 'read', tracked), \
             mock.patch.object(interop.PayloadDigest, 'write', counted), \
             mock.patch.object(zipfile.ZipExtFile, 'read', bounded_read):
            self.valid_outputs()
            self.verify()
        self.assertTrue(reads)
        self.assertLessEqual(max(reads), 1024 * 1024)
        manifest = json.loads((self.run / 'interop/manifest.json').read_text())
        self.assertEqual(manifest['media']['large.bin']['size'], 100 * 1024 * 1024 + 1)
        self.assertEqual(manifest['media']['empty.bin']['sha1'], hashlib.sha1(b'').hexdigest())
        self.assertEqual(set(manifest['media']), set(os.listdir(self.media)))

        self.assertTrue(archive_reads)
        self.assertLessEqual(max(digest_writes), 64 * 1024)
        self.assertEqual(sum(digest_writes), sum(v['size'] for v in manifest['media'].values()))


class Boundaries(Fixture):
    def test_package_link(self):
        self.package.rename(self.package.with_suffix('.real'))
        self.package.symlink_to(self.package.with_suffix('.real'))
        self.rejects(self.stage)

    def test_build_link(self):
        (self.root / 'build').rename(self.root / 'actual-build')
        (self.root / 'build').symlink_to(self.root / 'actual-build', target_is_directory=True)
        self.rejects(self.stage)

    def test_ancestor_link(self):
        generated = self.root / 'build/generated'
        generated.rename(self.root / 'build/actual-generated')
        generated.symlink_to(self.root / 'build/actual-generated', target_is_directory=True)
        self.rejects(self.stage)

    def test_media_file_link(self):
        (self.media / 'alias.bin').symlink_to(self.package)
        self.rejects(self.stage)

    def test_media_root_link(self):
        self.media.rename(self.expected / 'actual-media')
        self.media.symlink_to(self.expected / 'actual-media', target_is_directory=True)
        self.rejects(self.stage)

    def test_missing_media_root(self):
        shutil.rmtree(self.media)
        self.rejects(self.stage)

    def test_media_subdirectory(self):
        (self.media / 'child').mkdir()
        self.rejects(self.stage)

    def test_hardlink_package_and_media(self):
        os.link(self.package, self.package.with_suffix('.alias'))
        self.rejects(self.stage)
        self.package.with_suffix('.alias').unlink()
        os.link(self.media / 'referenced.bin', self.media / 'alias.bin')
        self.rejects(self.stage)

    def test_hardlink_database(self):
        os.link(self.db, self.expected / 'database-alias')
        self.rejects(self.stage)

    def test_device_fifo_and_ownership(self):
        os.mkfifo(self.media / 'fifo')
        self.rejects(self.stage)
        (self.media / 'fifo').unlink()
        s = os.stat(self.package)
        fake = mock.Mock(st_uid=s.st_uid + 1, st_mode=s.st_mode, st_nlink=1)
        self.rejects(lambda: interop.owned(fake))
        self.rejects(lambda: interop.owned(os.stat('/dev/null')))

    def test_outside_private_forbidden_and_parent_components(self):
        for path in (self.root / 'outside.colpkg', self.root / 'build/local-data/private.colpkg',
                     self.root / 'build/refs/file.colpkg', self.root / 'build/oracle/file.colpkg',
                     self.root / 'build/generated/../native.colpkg'):
            self.args.package = str(path)
            self.rejects(self.stage)

    def test_staging_and_run_output_links(self):
        (self.run / 'interop').symlink_to(self.expected, target_is_directory=True)
        self.rejects(self.stage)
        (self.run / 'interop').unlink()
        self.stage()
        (self.run / 'interop/out').rmdir()
        (self.run / 'interop/out').symlink_to(self.expected, target_is_directory=True)
        self.rejects(lambda: self.verify('ready'))

    def test_run_ancestor_link(self):
        self.run.rename(self.run.with_name('other'))
        self.run.symlink_to(self.run.with_name('other'), target_is_directory=True)
        self.rejects(self.stage)

    def test_wal_shm_and_journal_refused_even_empty(self):
        for suffix in ('-wal', '-shm', '-journal'):
            path = self.expected / ('collection.anki2' + suffix)
            path.write_bytes(b'')
            self.rejects(self.stage)
            path.unlink()

    def test_source_unchanged_and_media_isolated(self):
        initial = {p: (p.stat().st_mtime_ns, p.read_bytes()) for p in
                   (self.package, self.db, self.media / 'referenced.bin')}
        work = self.stage()
        self.verify('ready')
        self.assertEqual({p: (p.stat().st_mtime_ns, p.read_bytes()) for p in initial}, initial)
        self.assertEqual(set(os.listdir(work / 'fixtures')), {'collection.anki2', 'package.colpkg'})
        self.assertFalse(list(work.rglob('collection.media')))
        self.assertEqual(list((work / 'goldens').iterdir()), [])
        self.assertEqual(list((work / 'out').iterdir()), [])
        for path in (work / 'fixtures').iterdir():
            self.assertEqual(path.stat().st_mode & 0o222, 0)
        sc = json.loads((work / 'scenarios/rf4/package-interop.json').read_text())
        self.assertNotIn('setup', sc)
        self.assertNotIn('check_db', json.dumps(sc))
        self.assertEqual(sc['steps'][1], {'observe': [{'db_dump': {'raw': True}}, 'timing']})
        self.assertEqual(sc['steps'][3], sc['steps'][1])

    def test_in_place_same_length_mutation_during_stream(self):
        data = b'A' * (2 * 1024 * 1024)
        self.package.write_bytes(data)
        original = os.read
        fired = []
        def changing(fd, size):
            out = original(fd, size)
            if out and not fired:
                fired.append(True)
                with self.package.open('r+b') as stream:
                    stream.write(b'B')
            return out
        with mock.patch.object(interop.os, 'read', changing):
            self.rejects(self.stage, 'source-changed')
        self.assertTrue(fired)

    def test_media_same_length_mutation_during_stream(self):
        media_file = self.media / 'referenced.bin'
        media_file.write_bytes(b'A' * (2 * 1024 * 1024))
        inode = media_file.stat().st_ino
        original = os.read
        fired = []
        def changing(fd, size):
            out = original(fd, size)
            if out and os.fstat(fd).st_ino == inode and not fired:
                fired.append(True)
                with media_file.open('r+b') as stream:
                    stream.write(b'B')
            return out
        with mock.patch.object(interop.os, 'read', changing):
            self.rejects(self.stage, 'source-changed')
        self.assertTrue(fired)

    def test_source_replacement_during_stream(self):
        original = os.read
        fired = []
        def changing(fd, size):
            out = original(fd, size)
            if out and not fired:
                fired.append(True)
                replacement = self.package.with_suffix('.new')
                replacement.write_bytes(b'replaced')
                replacement.replace(self.package)
            return out
        with mock.patch.object(interop.os, 'read', changing):
            self.rejects(self.stage, 'source-changed')

    def test_post_capture_source_mutations_refused(self):
        self.valid_outputs()
        (self.media / 'extra.bin').write_bytes(b'extra')
        self.rejects(self.verify, 'source-changed')
        (self.media / 'extra.bin').unlink()
        # Parent directory metadata also changes, and a fresh run is required.
        self.rejects(self.verify, 'source-changed')

    def test_clock_safe_scope(self):
        for now, tz in [('2030-01-15T10:00:00+01:00', 'UTC'), (NOW, 'Europe/Paris'),
                        ('2038-01-01T00:00:00Z', 'UTC'), ('2030-01-15T10:00:00.1Z', 'UTC'),
                        ('1999-01-01T00:00:00Z', 'UTC')]:
            self.args.now, self.args.tz = now, tz
            self.rejects(self.stage)

    def test_unsupported_collection_clock(self):
        conn = sqlite3.connect(self.db)
        conn.execute('UPDATE config SET val=? WHERE KEY=?', (b'60', 'creationOffset'))
        conn.commit()
        conn.close()
        self.pack()
        self.rejects(self.stage, 'collection-clock-scope')

    def test_dry_validation_leaves_no_stage(self):
        self.args.validate_only = True
        self.stage()
        self.assertFalse((self.run / 'interop').exists())
        (self.media / 'alias.bin').symlink_to(self.package)
        self.rejects(self.stage)


class PreClient(Fixture):
    def pack_db_variant(self, sql, params=()):
        variant_db = self.root / 'build/generated/variant.anki2'
        shutil.copyfile(self.db, variant_db)
        conn = sqlite3.connect(variant_db)
        conn.execute(sql, params)
        conn.commit()
        conn.close()
        self.pack(data=variant_db.read_bytes())

    def test_neutral_raw_difference_coordinates_and_secret_redaction(self):
        secret_key = 'sk-synthetic-secret-key-\u00e9'
        secret_value = b'sk-synthetic-secret-value'
        changed_key = 'sk-synthetic-secret-key-e\u0301'
        changed_value = b'sk-synthetic-secret-value-changed'
        conn = sqlite3.connect(self.db)
        conn.execute('INSERT INTO config VALUES (?,0,0,?)', (secret_key, secret_value))
        conn.commit()
        conn.close()
        self.pack()
        work = self.stage()
        self.assertTrue(json.loads((work / 'decoded/proof.json').read_text())['pre_client_database_equal'])
        shutil.rmtree(work)

        cases = [
            ('UPDATE config SET val=? WHERE KEY=?', (changed_value, secret_key),
             'table 11 row 3 column 3 value'),
            ('UPDATE config SET val=? WHERE KEY=?', (secret_value.decode(), secret_key),
             'table 11 row 3 column 3 storage-kind'),
            ('UPDATE config SET KEY=? WHERE KEY=?', (changed_key, secret_key),
             'table 11 row 3 key expected-key-sha256 ' +
             hashlib.sha256(secret_key.encode('utf-8')).hexdigest() +
             ' decoded-key-sha256 ' + hashlib.sha256(changed_key.encode('utf-8')).hexdigest()),
            ('DELETE FROM config WHERE KEY=?', (secret_key,), 'table 11 row 3 shape'),
            ('DELETE FROM tags', (), 'table 10 row 0 shape'),
            ('UPDATE col SET mod=124', (), 'table 0 row 0 column 2 value'),
        ]
        for sql, params, coordinates in cases:
            with self.subTest(coordinates=coordinates):
                self.pack_db_variant(sql, params)
                with self.assertRaises(interop.Refusal) as error:
                    self.stage()
                diagnostic = str(error.exception)
                self.assertEqual(diagnostic, 'pre-client-database-difference ' + coordinates)
                for secret in (secret_key, changed_key, secret_value.decode(), changed_value.decode()):
                    self.assertNotIn(secret, diagnostic)
                self.assertFalse((work / 'decoded/proof.json').exists())
                shutil.rmtree(work)

    def test_timing_repair_cannot_hide_package_changes(self):
        for sql, params in [
            ('UPDATE cards SET queue=-2', ()),
            ('UPDATE config SET val=? WHERE KEY=?', (b'99', 'rollover')),
            ('INSERT INTO config VALUES (?,0,0,?)', ('unknown-generated-key', b'changed')),
            ('UPDATE notes SET flds=?', ('changed-note',)),
            ('UPDATE revlog SET time=98', ()),
            ('UPDATE cards SET due=1234', ()),
            ('UPDATE fields SET config=?', (b'changed-blob',)),
        ]:
            with self.subTest(sql=sql):
                self.pack_db_variant(sql, params)
                self.rejects(self.stage, 'pre-client-database-difference')
                # A matching client report after open cannot satisfy the separate
                # raw DB proof required before any client call.
                report, golden = outputs(self.run)
                save_outputs(self.run, report, golden)
                self.rejects(self.verify, 'decoder-proof-required')
                shutil.rmtree(self.run / 'interop')

    def test_missing_creation_offset_is_rejected_before_open(self):
        self.pack_db_variant('DELETE FROM config WHERE KEY=?', ('creationOffset',))
        self.rejects(self.stage, 'pre-client-database-difference')
        report, golden = outputs(self.run)
        save_outputs(self.run, report, golden)
        self.rejects(self.verify, 'decoder-proof-required')

    def test_direct_equality_has_no_stamp_waiver(self):
        for col in ('mod', 'scm'):
            self.pack_db_variant('UPDATE col SET ' + col + '=1894701600000')
            self.rejects(self.stage, 'pre-client-database-difference')
            shutil.rmtree(self.run / 'interop')

    def test_before_and_after_cannot_share_same_corruption(self):
        report, golden = self.valid_outputs()
        for index in (1, 3):
            golden['observations']['steps'][index]['db_dump']['cards']['1']['queue'] = -2
        save_outputs(self.run, report, golden)
        self.rejects(self.verify, 'database-difference')

    def test_frozen_fixture_without_creation_offset_is_refused(self):
        conn = sqlite3.connect(self.db)
        conn.execute('DELETE FROM config WHERE KEY=?', ('creationOffset',))
        conn.commit()
        conn.close()
        self.pack()
        self.rejects(self.stage, 'collection-clock-scope')

    def test_both_combined_observations_order_dump_before_timing(self):
        value = interop.scenario('2030-01-15T10:00:00')
        self.assertEqual(len(value['steps']), 5)
        for index in (1, 3):
            self.assertEqual(value['steps'][index],
                {'observe': [{'db_dump': {'raw': True}}, 'timing']})
        report, golden = self.valid_outputs()
        for index in (1, 3):
            changed = copy.deepcopy(golden)
            changed['observations']['steps'][index]['timing']['days_elapsed'] += 1
            save_outputs(self.run, report, changed)
            self.rejects(self.verify, 'timing-difference')


class MediaNames(Fixture):
    def inventory(self, apple):
        fd = interop.open_dir(self.media)
        try:
            return interop.media_inventory(fd, apple)
        finally:
            os.close(fd)

    def test_apple_nfd_expected_nfc_exact_client_names_without_rename(self):
        original = 'e\u0301.bin'
        (self.media / original).write_bytes(b'apple generated source')
        before = set(os.listdir(self.media))
        self.pack()
        with mock.patch.object(interop.sys, 'platform', 'darwin'):
            _, golden = self.valid_outputs()
        self.assertEqual(set(os.listdir(self.media)), before)
        imported = golden['observations']['steps'][4]['media_dir']
        self.assertIn('\u00e9.bin', imported)
        self.assertNotIn(original, imported)
        self.assertEqual(imported['\u00e9.bin']['sha1'], hashlib.sha1(b'apple generated source').hexdigest())
        self.verify()

    def test_nfc_collision_refused_and_case_payloads_distinct(self):
        (self.media / 'e\u0301.bin').write_bytes(b'nfd')
        (self.media / '\u00e9.bin').write_bytes(b'nfc')
        self.rejects(lambda: self.inventory(True), 'media-nfc-collision')
        (self.media / '\u00e9.bin').unlink()
        (self.media / 'E\u0301.bin').write_bytes(b'upper')
        found = self.inventory(True)
        self.assertNotEqual(found['\u00e9.bin']['sha1'], found['\u00c9.bin']['sha1'])
        self.rejects(lambda: self.inventory(False), 'non-apple-non-nfc')

    def test_m8_exact_unicode_simple_case_skip_and_near_misses(self):
        ignored = ['Thumbs.db', 'THUMBS.DB', 'thumbs_db', 'thumbsXdb', 'thumbſ-db',
                   '.DS_Store', '.ds_store', 'xds_store', 'ſDS_STORE', ':thumbs\ndb']
        included = ['thumbs.db.png', 'thumbs.dbx', 'thumb.db', 'my-thumbs.db',
                    'ds_store', '..ds_store', 'my.ds_store', '_unused', 'empty',
                    'thumbs.Kb', '\u0860.bin', '\U0001f600.bin']
        for name in ignored + included:
            (self.media / name).write_bytes(b'')
        got = self.inventory(False)
        self.assertEqual(set(got), {'referenced.bin', *included})
        for name in ignored:
            self.assertNotIn(name, got)
        decomposable = self.media / 'thumbs.\u212ab'
        self.assertEqual(unicodedata.normalize('NFC', decomposable.name), 'thumbs.Kb')
        decomposable.write_bytes(b'')
        self.rejects(lambda: self.inventory(False), 'non-apple-non-nfc')
        decomposable.unlink()
        (self.media / '.DS_Store').unlink()
        (self.media / '.DS_Store').symlink_to(self.package)
        self.rejects(lambda: self.inventory(False), 'entry-kind')

    def test_unsafe_names_refuse_instead_of_rename_or_drop(self):
        for name in ['con.jpg', 'bad?.bin', 'trailing.', '\u0378.bin', '\U0001f970.bin']:
            with self.subTest(name=name):
                path = self.media / name
                path.write_bytes(b'synthetic')
                self.rejects(lambda: self.inventory(False), 'unsafe-media-name')
                path.unlink()


class AllocationBounds(Fixture):
    def test_json_size_depth_duplicates_malformed_and_token_budget(self):
        self.stage()
        work = interop.open_dir(self.run / 'interop/out')
        path = self.run / 'interop/out/report.json'
        try:
            with path.open('wb') as stream:
                stream.truncate(interop.JSON_LIMIT + 1)
            with mock.patch.object(interop.os, 'read', side_effect=AssertionError('must not read')):
                self.rejects(lambda: interop.load_json(work, path.name), 'json-size-scope')
            for content, diagnostic in [
                (b'{"x":1,"x":2}', 'duplicate-json-key'),
                (b'[' * (interop.JSON_DEPTH + 1), 'json-depth-scope'),
                (b'{', None), (b'{"x":NaN}', 'nonfinite-json')]:
                path.write_bytes(content)
                self.rejects(lambda: interop.load_json(work, path.name), diagnostic)
            path.write_text('[0,0,0,0,0]')
            with mock.patch.object(interop, 'JSON_TOKENS', 3):
                self.rejects(lambda: interop.load_json(work, path.name), 'json-token-scope')
        finally:
            os.close(work)

    def test_report_growth_reads_initial_size_plus_one_byte(self):
        path = self.root / 'build/generated/report.json'
        path.write_bytes(b'{}')
        fd = interop.open_dir(path.parent)
        original = os.read
        requested = []
        def growing(source, size):
            requested.append(size)
            with path.open('ab') as stream:
                stream.write(b' ' * 4096)
            self.assertLessEqual(sum(requested), 3)
            return original(source, size)
        try:
            with mock.patch.object(interop.os, 'read', growing):
                self.rejects(lambda: interop.load_json(fd, path.name), 'output-changed')
            self.assertEqual(requested, [2, 1])
        finally:
            os.close(fd)

    def test_source_growth_keeps_extra_bytes_out_of_hash_and_sink(self):
        path = self.media / 'referenced.bin'
        path.write_bytes(b'abc')
        fd = interop.open_dir(self.media)
        original = os.read
        requests, digested = [], []
        class Hash:
            def update(self, data):
                digested.append(data)
            def hexdigest(self):
                return 'unused'
        def growing(source, size):
            requests.append(size)
            with path.open('ab') as stream:
                stream.write(b'grow' * 1024)
            self.assertLessEqual(sum(requests), 4)
            return original(source, size)
        destination = io.BytesIO()
        try:
            with mock.patch.object(interop.os, 'read', growing), \
                 mock.patch.object(interop.hashlib, 'sha1', Hash), \
                 mock.patch.object(interop.hashlib, 'sha256', Hash):
                self.rejects(lambda: interop.read_file(fd, path.name, destination), 'source-changed')
            self.assertEqual(requests, [3, 1])
            self.assertEqual(destination.getvalue(), b'abc')
            self.assertEqual(digested, [b'abc', b'abc'])
        finally:
            os.close(fd)

    def test_huge_unrelated_config_is_never_selected_by_timing(self):
        conn = sqlite3.connect(self.db)
        conn.execute('INSERT INTO config VALUES (?,0,0,zeroblob(?))',
                     ('unrelated-huge', interop.CELL_LIMIT * 4))
        conn.commit()
        conn.close()
        queries = []
        original = interop.sqlite_reader
        def traced(path):
            conn = original(path)
            conn.set_trace_callback(queries.append)
            return conn
        with mock.patch.object(interop, 'sqlite_reader', traced):
            result = interop.timing_for(self.db, 1894701600)
        self.assertEqual(result['now'], 1894701600)
        fetches = [q for q in queries if q.startswith('SELECT val')]
        self.assertEqual(len(fetches), 3)
        self.assertFalse(any('unrelated-huge' in q for q in queries))
        self.assertTrue(all('FROM config NOT INDEXED WHERE "KEY" COLLATE BINARY=' in q
                            for q in fetches))

    def test_oversized_required_key_fails_before_value_select(self):
        conn = sqlite3.connect(self.db)
        conn.execute('UPDATE config SET val=zeroblob(?) WHERE KEY=?',
                     (interop.CLOCK_LIMIT + 1, 'creationOffset'))
        conn.commit()
        conn.close()
        queries = []
        original = interop.sqlite_reader
        def traced(path):
            conn = original(path)
            conn.set_trace_callback(queries.append)
            return conn
        with mock.patch.object(interop, 'sqlite_reader', traced):
            self.rejects(lambda: interop.timing_for(self.db, 1894701600), 'collection-clock-scope')
        self.assertFalse(any(q.startswith('SELECT val') for q in queries))

    def test_sqlite_cell_and_row_bounds_refuse_before_python_payload_fetch(self):
        conn = sqlite3.connect(self.db)
        conn.execute('UPDATE notes SET flds=zeroblob(?)', (interop.CELL_LIMIT + 1,))
        conn.commit()
        conn.close()
        self.rejects(lambda: list(interop.raw_rows(self.db)), 'sqlite-cell')


class RawSchema(Fixture):
    def setUp(self):
        super().setUp()
        # Public storage/schema-18 S2/S3 only. Rebuild this generated fixture
        # with real affinities, primary trees and all four unique name indexes.
        definitions = {
            'col': 'id integer PRIMARY KEY, crt integer NOT NULL, mod integer NOT NULL, '
                   'scm integer NOT NULL, ver integer NOT NULL, dty integer NOT NULL, '
                   'usn integer NOT NULL, ls integer NOT NULL, conf text NOT NULL, '
                   'models text NOT NULL, decks text NOT NULL, dconf text NOT NULL, tags text NOT NULL',
            'notes': 'id integer PRIMARY KEY, guid text NOT NULL, mid integer NOT NULL, '
                     'mod integer NOT NULL, usn integer NOT NULL, tags text NOT NULL, '
                     'flds text NOT NULL, sfld integer NOT NULL, csum integer NOT NULL, '
                     'flags integer NOT NULL, data text NOT NULL',
            'cards': 'id integer PRIMARY KEY, ' + ', '.join(n + ' integer NOT NULL' for n in
                     'nid did ord mod usn type queue due ivl factor reps lapses left odue odid flags'.split())
                     + ', data text NOT NULL',
            'revlog': 'id integer PRIMARY KEY, ' + ', '.join(n + ' integer NOT NULL' for n in
                      'cid usn ease ivl lastIvl factor time type'.split()),
            'graves': 'oid integer NOT NULL, type integer NOT NULL, usn integer NOT NULL, PRIMARY KEY (oid, type)',
            'decks': 'id integer PRIMARY KEY NOT NULL, name text NOT NULL COLLATE unicase, '
                     'mtime_secs integer NOT NULL, usn integer NOT NULL, common blob NOT NULL, kind blob NOT NULL',
            'deck_config': 'id integer PRIMARY KEY NOT NULL, name text NOT NULL COLLATE unicase, '
                           'mtime_secs integer NOT NULL, usn integer NOT NULL, config blob NOT NULL',
            'notetypes': 'id integer NOT NULL PRIMARY KEY, name text NOT NULL COLLATE unicase, '
                         'mtime_secs integer NOT NULL, usn integer NOT NULL, config blob NOT NULL',
            'fields': 'ntid integer NOT NULL, ord integer NOT NULL, name text NOT NULL COLLATE unicase, '
                      'config blob NOT NULL, PRIMARY KEY (ntid, ord)',
            'templates': 'ntid integer NOT NULL, ord integer NOT NULL, name text NOT NULL COLLATE unicase, '
                         'mtime_secs integer NOT NULL, usn integer NOT NULL, config blob NOT NULL, PRIMARY KEY (ntid, ord)',
            'tags': 'tag text NOT NULL PRIMARY KEY COLLATE unicase, usn integer NOT NULL, '
                    'collapsed boolean NOT NULL, config blob NULL',
            'config': 'KEY text NOT NULL PRIMARY KEY, usn integer NOT NULL, mtime_secs integer NOT NULL, val blob NOT NULL',
        }
        # Only these independently known fixed facts may construct the fixture:
        # Synthetic == synthetic, and synthetic < Z < ä under S4. No general
        # Unicode comparator, BINARY fallback, core or external client is used.
        ranks = {'Synthetic': 0, 'synthetic': 0, 'Z': 1, 'ä': 2}
        def fixed_comparison(left, right):
            if left not in ranks or right not in ranks:
                raise AssertionError('unknown fixture comparison')
            return (ranks[left] > ranks[right]) - (ranks[left] < ranks[right])
        conn = sqlite3.connect(self.db)
        try:
            conn.create_collation('unicase', fixed_comparison)
            for table, declaration in definitions.items():
                rows = conn.execute('SELECT * FROM ' + table).fetchall()
                conn.execute('DROP TABLE ' + table)
                suffix = ' WITHOUT ROWID' if table in interop.KEYS else ''
                conn.execute('CREATE TABLE ' + table + ' (' + declaration + ')' + suffix)
                insert = 'INSERT INTO ' + table + ' VALUES (' + ','.join('?' for _ in rows[0]) + ')'
                conn.executemany(insert, rows)
                if table == 'tags':
                    conn.executemany(insert, [('Z', -1, 1, b'\x00\xff'), ('ä', 7, 0, None)])
                if table in ('decks', 'deck_config', 'notetypes', 'fields', 'templates'):
                    columns = interop.COLUMNS[table].split()
                    for identity, name in ((2, 'Z'), (3, 'ä')):
                        row = list(rows[0])
                        row[columns.index('ord' if table in ('fields', 'templates') else 'id')] = identity
                        row[columns.index('name')] = name
                        conn.execute(insert, row)
            for table, index, keys in (
                    ('fields', 'idx_fields_name_ntid', 'name, ntid'),
                    ('templates', 'idx_templates_name_ntid', 'name, ntid'),
                    ('notetypes', 'idx_notetypes_name', 'name'),
                    ('decks', 'idx_decks_name', 'name')):
                conn.execute('CREATE UNIQUE INDEX ' + index + ' ON ' + table + ' (' + keys + ')')
            conn.commit()
        finally:
            conn.close()
        self.pack()

    def test_raw_binary_scan_never_invokes_unicase_and_preserves_source(self):
        before = (self.db.read_bytes(), interop.signature(self.db.stat()))
        with mock.patch.object(interop.RawConnection, 'refuse_collation', autospec=True,
                               side_effect=AssertionError('raw scan called unicase')) as callback:
            rows = list(interop.raw_rows(self.db))
            self.assertEqual([key for table, key, _ in rows if table == 'tags'],
                             ['Z', 'synthetic', 'ä'])  # Differs from the primary tree's S4 order.
            work = self.stage()
            self.verify('ready')
            self.assertEqual(json.loads((work / 'decoded/expected.json').read_text()), dump_db(self.db))
            callback.assert_not_called()
        self.assertEqual(before, (self.db.read_bytes(), interop.signature(self.db.stat())))
        proof = json.loads((work / 'decoded/proof.json').read_text())
        self.assertTrue(proof['pre_client_database_equal'])
        self.assertTrue(proof['pre_client_media_equal'])
        self.assertEqual(proof['pre_client_stamp_edits'], 0)

    def test_real_schema_native_clock_and_complete_fake_client_binding(self):
        self.native_clock()
        self.valid_outputs()
        self.verify()

    def test_semantic_query_refuses_and_caught_exception_cannot_publish_proof(self):
        interop.stage(self.args)
        original = interop.sqlite_reader
        handles = []
        def poisoned(path):
            conn = original(path)
            handles.append(conn)
            self.rejects(lambda: conn.execute('SELECT tag FROM tags NOT INDEXED '
                         'WHERE tag COLLATE unicase=?', ('Z',)).fetchall(), 'sqlite-collation-required')
            self.assertTrue(conn.collation_invoked)
            self.rejects(lambda: conn.execute('SELECT 1'), 'sqlite-collation-required')
            return conn  # Deliberately caught callback failure remains fatal.
        with mock.patch.object(interop, 'sqlite_reader', poisoned):
            self.rejects(self.decode, 'sqlite-collation-required')
        self.assertTrue(handles)
        self.assertFalse((self.run / 'interop/decoded/proof.json').exists())

    def test_adapter_retains_collation_failure_after_callback_error_is_caught(self):
        # An adapter may catch a callback error while executing or fetching.
        # The raw connection must retain that failure through later calls and close.
        for operation in ('execute', 'fetch'):
            with self.subTest(phase=operation):
                adapter = mock.Mock()
                raw_connection = interop.RawConnection(adapter)
                def capture_callback_error(*unused_arguments):
                    try:
                        raw_connection.refuse_collation('Z', 'synthetic')
                    except interop.Refusal:
                        pass
                    return mock.Mock() if operation == 'execute' else ('Z',)
                if operation == 'execute':
                    adapter.execute.side_effect = capture_callback_error
                    self.rejects(lambda: raw_connection.execute('SELECT 1'), 'sqlite-collation-required')
                else:
                    row_cursor = raw_connection.execute('SELECT 1')
                    adapter.execute.return_value.fetchone.side_effect = capture_callback_error
                    self.rejects(row_cursor.fetchone, 'sqlite-collation-required')
                self.assertTrue(raw_connection.collation_invoked)
                self.rejects(lambda: raw_connection.execute('SELECT 1'), 'sqlite-collation-required')
                self.rejects(raw_connection.close, 'sqlite-collation-required')
                adapter.close.assert_called_once()


class Decoder(Fixture):
    def test_decoder_keeps_sqlite_api_limits(self):
        conn = interop.sqlite_reader(self.db)
        try:
            self.assertEqual(conn.getlimit(sqlite3.SQLITE_LIMIT_LENGTH), interop.CELL_LIMIT)
            self.assertEqual(conn.getlimit(sqlite3.SQLITE_LIMIT_SQL_LENGTH), 16384)
        finally:
            conn.close()

    def test_neutral_dump_retains_byte_and_token_limits(self):
        interop.stage(self.args)
        for limit_name, limit, diagnostic in [('JSON_LIMIT', 256, 'json-size-scope'),
                                               ('JSON_TOKENS', 32, 'json-token-scope')]:
            with self.subTest(limit=limit_name), mock.patch.object(interop, limit_name, limit):
                self.rejects(self.decode, diagnostic)
                output = self.run / 'interop/decoded'
                self.assertFalse((output / 'proof.json').exists())
                if limit_name == 'JSON_LIMIT':
                    self.assertLessEqual((output / 'expected.json').stat().st_size, limit)
            shutil.rmtree(output)
            output.mkdir()

    def test_compressed_collection_and_concatenated_skippable_frames(self):
        data = self.db.read_bytes()
        compressed = compressed_zstd(data)
        self.assertLess(len(compressed), len(data))
        skippable = b'\x50\x2a\x4d\x18' + struct.pack('<I', 3) + b'abc'
        frame = compressed_zstd(data[:97]) + skippable + compressed_zstd(data[97:])
        sink = io.BytesIO()
        interop.decode_zstd(io.BytesIO(frame), sink, len(frame))
        self.assertEqual(sink.getvalue(), data)
        self.rejects(lambda: interop.decode_zstd(io.BytesIO(frame), io.BytesIO(), len(frame),
                                                native=True))
        self.pack(frame=native_zstd(data))
        self.stage()
        self.verify('ready')
        self.assertEqual((self.run / 'interop/decoded/collection.anki2').read_bytes(), data)

    def test_truncated_garbage_checksum_and_window_limits(self):
        data = self.db.read_bytes()
        # Raw block frame with checksum flag: zero is an intentionally wrong checksum.
        checksum = bytearray(raw_zstd(data))
        checksum[4] |= 4
        checksum.extend(b'\0' * 4)
        # Non-single-segment frame asks for a 256 MiB window (log 28).
        huge_window = b'\x28\xb5\x2f\xfd\x00\x90\x01\x00\x00'
        frame = compressed_zstd(data)
        for bad in (frame[:-1], frame + b'garbage', bytes(checksum), huge_window):
            with self.subTest(length=len(bad)):
                self.pack(frame=bad)
                self.rejects(self.stage)
                self.assertFalse((self.run / 'interop/decoded/proof.json').exists())
                shutil.rmtree(self.run / 'interop')

    def test_decoded_limit_stops_before_first_excess_write_and_ratio_limit(self):
        data = b'A' * (256 * 1024)
        frame = compressed_zstd(data)
        sink = io.BytesIO()
        with mock.patch.object(interop, 'DB_LIMIT', 100001):
            self.rejects(lambda: interop.decode_zstd(io.BytesIO(frame), sink, len(frame)),
                         'zstd-decoded-scope')
        self.assertLessEqual(len(sink.getvalue()), 100001)
        bomb = compressed_zstd(b'A' * (16 * 1024 * 1024 + 1))
        self.rejects(lambda: interop.decode_zstd(io.BytesIO(bomb), io.BytesIO(), len(bomb)),
                     'zstd-decoded-scope')

    def test_directory_limit_precedes_stdlib_zipfile_allocation(self):
        self.pack()
        data = bytearray(self.package.read_bytes())
        struct.pack_into('<HH', data, len(data) - 14, interop.ENTRY_LIMIT + 1, interop.ENTRY_LIMIT + 1)
        self.package.write_bytes(data)
        interop.stage(self.args)
        with mock.patch.object(interop.zipfile, 'ZipFile', side_effect=AssertionError('no allocation')):
            self.rejects(self.decode, 'archive-directory-scope')

    def test_ready_requires_independent_proof_even_with_perfect_fake_reports(self):
        interop.stage(self.args)
        report, golden = outputs(self.run)
        save_outputs(self.run, report, golden)
        self.rejects(self.verify, 'decoder-proof-required')

    def archive_entries(self):
        # Corruptions below use only this small generated fixture.
        with zipfile.ZipFile(self.package) as archive:
            return [(info.filename, archive.read(info)) for info in archive.infolist()]

    def refuse_archive(self, entries, diagnostic=None):
        with zipfile.ZipFile(self.package, 'w', compression=zipfile.ZIP_STORED) as archive:
            for name, data in entries:
                archive.writestr(name, data)
        self.rejects(self.stage, diagnostic)
        work = self.run / 'interop'
        self.assertFalse((work / 'decoded/proof.json').exists())
        # Matching client reports still require a separate archive media proof.
        report, golden = outputs(self.run)
        save_outputs(self.run, report, golden)
        self.rejects(self.verify, 'decoder-proof-required')
        shutil.rmtree(work)

    def test_missing_extra_nonnumeric_and_duplicate_archive_members(self):
        entries = self.archive_entries()
        for name, _ in entries:
            with self.subTest(missing=name):
                self.refuse_archive([entry for entry in entries if entry[0] != name],
                                    'archive-members')
        for name in ('1', '00', '-1', 'payload', '0/', '../x', 'META'):
            with self.subTest(extra=name):
                self.refuse_archive(entries[:-1] + [(name, native_zstd(b''))] + entries[-1:],
                                    'archive-members')
        for name in ('0', 'media', 'meta'):
            with self.subTest(duplicate=name):
                self.refuse_archive(entries + [next(entry for entry in entries if entry[0] == name)],
                                    'archive-duplicate')
        reversed_entries = entries[:3] + entries[4:] + entries[3:4]
        self.refuse_archive(reversed_entries, 'archive-members')

    def test_unlisted_local_members_and_mismatched_local_headers(self):
        # CD advertises the same set while a complete extra local record hides in a gap.
        entries = self.archive_entries()
        with zipfile.ZipFile(self.package, 'w') as archive:
            archive.writestr('hidden', b'')
            for name, data in entries:
                archive.writestr(name, data)
        data = bytearray(self.package.read_bytes())
        cd = struct.unpack_from('<I', data, len(data) - 6)[0]
        record_size = 46 + len('hidden')
        del data[cd:cd + record_size]
        struct.pack_into('<HHI', data, len(data) - 14, len(entries), len(entries),
                         len(data) - 22 - cd)
        self.package.write_bytes(data)
        self.rejects(self.stage, 'archive-local-layout')
        shutil.rmtree(self.run / 'interop')
        self.pack()
        data = bytearray(self.package.read_bytes())
        data[30] = ord('M')  # local meta name disagrees with the central directory
        self.package.write_bytes(data)
        self.rejects(self.stage, 'archive-local-layout')

    def test_malformed_map_records_wire_rules_and_unsafe_names(self):
        entries = self.archive_entries()
        digest = hashlib.sha1(b'synthetic payload').digest()
        record = map_record('referenced.bin', 17, digest)
        malformed = [b'\x00', b'\x0a\x80', b'\x08\x01', b'\x0a\x05x',
                     b'\x3b', b'\x3c', b'\x3b\x44', b'\x38' + b'\xff' * 9 + b'\x02',
                     varint(1 << 32), b'\x3e',
                     message_field(1, record + b'\x10\x80'),
                     message_field(1, record + b'\x10' + varint(1 << 32)),
                     message_field(1, record + b'\x12\x00'),
                     message_field(1, record + varint(255 << 3) + b'\x00'),
                     message_field(1, message_field(1, b'\xff') + message_field(3, digest)),
                     message_field(1, message_field(1, b'referenced.bin')),
                     message_field(1, map_record('referenced.bin', 17, digest[:-1]))]
        for raw in malformed:
            with self.subTest(kind='wire', size=len(raw)):
                self.refuse_archive([(n, native_zstd(raw) if n == 'media' else b)
                                     for n, b in entries])
        for name in ('', '.', '..', '../x', 'a/b', 'a\\b', 'con.jpg', 'nul',
                     'bad?.bin', 'tab\t.bin', 'space ', 'dot.', 'e\u0301.bin',
                     '\u0378.bin', 'x' * 121):
            raw = media_map([(name, 17, digest)])
            with self.subTest(kind='name', size=len(name)):
                self.refuse_archive([(n, native_zstd(raw) if n == 'media' else b)
                                     for n, b in entries], 'unsafe-media-name')

    def test_map_requires_order_unique_names_and_exact_inventory(self):
        entries = self.archive_entries()
        digest = hashlib.sha1(b'synthetic payload').digest()
        variants = [[('referenced.bin', 17, digest)] * 2,
                    [('z.bin', 17, digest), ('a.bin', 0, hashlib.sha1(b'').digest())],
                    [('Referenced.bin', 17, digest)], [('referenced.bin', 16, digest)],
                    [('referenced.bin', 17, bytes(20))], []]
        for records in variants:
            raw = media_map(records)
            with self.subTest(records=len(records)):
                self.refuse_archive([(n, native_zstd(raw) if n == 'media' else b)
                                     for n, b in entries])

    def test_payload_requires_exact_length_hash_and_entry_number(self):
        entries = self.archive_entries()
        for payload in (b'', b'synthetic payload!', b'Synthetic payload'):
            with self.subTest(size=len(payload)):
                self.refuse_archive([(n, native_zstd(payload) if n == '0' else b)
                                     for n, b in entries])
        self.refuse_archive([('1' if n == '0' else n, b) for n, b in entries], 'archive-members')

    def test_media_map_and_payload_frames_reject_trailing_truncated_checksum_and_settings(self):
        entries = self.archive_entries()
        for target in ('collection.anki21b', 'media', '0'):
            frame = dict(entries)[target]
            bad_checksum = frame[:-1] + bytes([frame[-1] ^ 1])
            bad_size = bytearray(frame)
            # Change the declared size in its first byte, keeping the payload/checksum.
            offset = 5 + (not bool(frame[4] & 32))
            bad_size[offset] ^= 1
            for bad in (frame[:-1], frame + b'garbage', frame + native_zstd(b''),
                        bad_checksum, bytes(bad_size), raw_zstd(b'synthetic payload'),
                        compressed_zstd(b'synthetic payload')):
                with self.subTest(target=target, size=len(bad)):
                    self.refuse_archive([(n, bad if n == target else b) for n, b in entries])
        frame = native_zstd(b'abc')
        no_size = frame[:4] + b'\x04\x00' + frame[6:]
        dictionary = frame[:4] + bytes([frame[4] | 1]) + frame[5:]
        reserved = frame[:4] + bytes([frame[4] | 8]) + frame[5:]
        huge_window = frame[:4] + b'\x84\x90' + struct.pack('<I', 3) + frame[6:]
        skippable = b'\x50\x2a\x4d\x18' + struct.pack('<I', 0)
        for bad in (no_size, dictionary, reserved, huge_window, skippable + frame):
            self.refuse_archive([(n, bad if n == '0' else b) for n, b in entries])

    def test_unknown_fields_are_validated_and_exact_decoded_map_bytes_are_hashed(self):
        entries = self.archive_entries()
        digest = hashlib.sha1(b'synthetic payload').digest()
        record = map_record('referenced.bin', 17, digest)
        # Unknown varint/fixed64/bytes/group/fixed32, and a repeated known scalar
        # with last-value semantics. Unknown bytes remain in the truthful map hash.
        unknown = b'\x38\x01\x41' + bytes(8) + b'\x4a\x01x\x53\x58\x01\x54\x65' + bytes(4)
        raw = unknown + message_field(1, record + b'\x10\x11' + unknown)
        with zipfile.ZipFile(self.package, 'w') as archive:
            for name, data in entries:
                archive.writestr(name, native_zstd(raw) if name == 'media' else data)
        work = self.stage()
        self.verify('ready')
        self.assertEqual((work / 'decoded/media-map.bin').read_bytes(), raw)
        proof = json.loads((work / 'decoded/proof.json').read_text())
        self.assertEqual(proof['decoded_media_map_sha256'], hashlib.sha256(raw).hexdigest())
        path = work / 'decoded/media-map.bin'
        path.chmod(0o600)
        path.write_bytes(raw + b'\x38\x00')
        self.rejects(lambda: self.verify('ready'), 'decoder-proof-changed')

    def test_map_and_aggregate_bounds_fail_without_independent_proof(self):
        with mock.patch.object(interop, 'MAP_LIMIT', 1):
            self.rejects(self.stage, 'zstd-decoded-scope')
        self.assertFalse((self.run / 'interop/decoded/proof.json').exists())
        shutil.rmtree(self.run / 'interop')
        with mock.patch.object(interop, 'TOTAL_LIMIT', 1):
            self.rejects(self.stage, 'archive-decoded-scope')
        self.assertFalse((self.run / 'interop/decoded/proof.json').exists())

    def test_empty_payload_default_length_and_bounded_unknown_group_depth(self):
        (self.media / 'referenced.bin').unlink()
        (self.media / 'empty.bin').write_bytes(b'')
        self.pack()
        entries = self.archive_entries()
        record = message_field(1, b'empty.bin') + message_field(3, hashlib.sha1(b'').digest())
        raw = message_field(1, record)  # proto3 omits the default uint32 zero
        with zipfile.ZipFile(self.package, 'w') as archive:
            for name, data in entries:
                archive.writestr(name, native_zstd(raw) if name == 'media' else data)
        work = self.stage()
        self.verify('ready')
        self.assertTrue(json.loads((work / 'decoded/proof.json').read_text())['pre_client_media_equal'])
        # 100 empty unknown groups at top level are legal; another field at depth 100 is not.
        valid = b'\x3b' * 100 + b'\x3c' * 100 + raw
        self.assertEqual(interop.media_records(valid),
                         [('empty.bin', {'size': 0, 'sha1': hashlib.sha1(b'').hexdigest()})])
        for invalid in (b'\x3b' * 101 + b'\x3c' * 101,
                        b'\x3b' * 100 + b'\x38\x00' + b'\x3c' * 100):
            self.rejects(lambda: interop.media_records(invalid), 'media-map-protobuf')

    def test_expected_inventory_tamper_never_becomes_media_archive_proof(self):
        work = self.stage()
        path = work / 'media-inventory.json'
        path.chmod(0o600)
        path.write_text('{}')
        self.rejects(lambda: self.verify('ready'), 'media-inventory-changed')


class Wrapper(Fixture):
    def invoke(self, *args, dry=False, mode='valid'):
        tools = self.root / 'tools/scripts'
        tools.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(HELPER, tools / HELPER.name)
        script = r'''
set -u
mg_job() { :; }
mg_message() { :; }
mg_failure() { printf 'failure %s\n' "$1" >>"$EVENTS"; }
mg_status() { printf 'status %s\n' "$1" >>"$EVENTS"; }
mg_detail() { printf 'detail %s\n' "$1" >>"$EVENTS"; }
mg_count() { printf 'count %s %s\n' "$1" "$2" >>"$EVENTS"; }
mg_artifact() { printf 'artifact %s\n' "$1" >>"$EVENTS"; }
mg_compose() {
  case " $* " in
    *" build "*) printf 'decoder-build\n' >>"$EVENTS" ;;
    *)
      printf 'decoder-run\n' >>"$EVENTS"
      python3 "$FAKE_SCRIPT" --decode "$MG_RUN_DIR" "$@"
      ;;
  esac
}
mg_oracle__tree() { printf 'tree\n' >>"$EVENTS"; MG_ORACLE_DIR=black-box; }
mg_oracle__build() { printf 'build\n' >>"$EVENTS"; MG_ORACLE_TAG=pinned; }
mg_oracle__run() {
  printf 'run\n' >>"$EVENTS"
  python3 "$FAKE_SCRIPT" --fake "$MG_RUN_DIR" "$FAKE_MODE" "$@"
}
. "$SOURCE_WRAPPER"
mg_package_interop_check "$@"
'''
        env = dict(os.environ, MG_ROOT=str(self.root), MG_RUN_DIR=str(self.run),
                   MG_DRY_RUN='1' if dry else '0', MG_RUN_ID='unit', SOURCE_WRAPPER=str(WRAPPER),
                   FAKE_SCRIPT=str(Path(__file__).absolute()), FAKE_MODE=mode,
                   EVENTS=str(self.root / 'events'), ORACLE_FIXTURES='/must-never-be-mounted')
        return subprocess.run(['bash', '-c', script, 'wrapper', *args], env=env,
                              capture_output=True, text=True)

    def cli(self):
        return ['--package', str(self.package), '--expected', str(self.expected),
                '--now', self.args.now, '--tz', self.args.tz]

    def events(self):
        return (self.root / 'events').read_text()

    def test_public_helper_composition_and_exact_readonly_mounts(self):
        result = self.invoke(*self.cli())
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('status pass', self.events())
        self.assertEqual([x for x in self.events().splitlines() if x in
                          ('decoder-build', 'decoder-run', 'tree', 'build', 'run')],
                         ['decoder-build', 'decoder-run', 'tree', 'build', 'run'])
        args = json.loads((self.run / 'interop/out/fake-args.json').read_text())
        self.assertEqual(args[:2], ['pinned', 'black-box'])
        stage = str(self.run / 'interop')
        for mount in (stage + '/scenarios:/in/scenarios:ro', stage + '/fixtures:/in/fixtures:ro',
                      stage + '/goldens:/in/goldens', stage + '/out:/out'):
            self.assertIn(mount, args)
        self.assertIn('ORACLE_FIXTURES=/in/fixtures', args)
        self.assertIn('TZ=UTC', args)
        self.assertNotIn('/must-never-be-mounted', ' '.join(args))
        self.assertEqual(args[-11:], ['oracle', 'harness', 'record', '--scenarios', '/in/scenarios',
            '--goldens', '/in/goldens', '--jobs', '1', '--report', '/out/report.json'])
        self.assertNotIn(str(self.media), ' '.join(args))
        self.assertNotIn('network', ' '.join(args))
        self.assertGreaterEqual(self.events().count('artifact report'), 3)
        self.assertIn('detail package_interop', self.events())
        decoder = json.loads((self.run / 'interop/decoder-args.json').read_text())
        self.assertIn('meguri.mg.run-id=unit', decoder)
        self.assertIn(stage + '/fixtures:/in:ro', decoder)
        self.assertIn(stage + '/decoded:/out', decoder)
        self.assertIn(str(self.root / 'tools/scripts/package-interop-check.py') + ':/decoder.py:ro', decoder)
        self.assertEqual(decoder[decoder.index('package-interop-decoder') + 1:],
            ['python3', '/decoder.py', 'decode', '--package', '/in/package.colpkg',
             '--expected', '/in/collection.anki2', '--output', '/out',
             '--media-inventory', '/expected-media.json', '--now', NOW, '--tz', 'UTC'])
        mounts = [decoder[i + 1] for i, arg in enumerate(decoder[:-1]) if arg == '-v']
        self.assertEqual(len(mounts), 4)
        self.assertIn(stage + '/media-inventory.json:/expected-media.json:ro', mounts)
        self.assertNotIn('/expected-media.json', ' '.join(args))
        self.assertNotIn(stage + '/media-inventory.json', ' '.join(args))
        self.assertFalse(any(':/work' in mount or str(self.expected) in mount for mount in mounts))
        service = (REPO / 'docker/compose.yml').read_text().split('  package-interop-decoder:', 1)[1]
        service = service.split('  fuzz:', 1)[0]
        self.assertIn('network_mode: none', service)
        self.assertIn('read_only: true', service)
        self.assertNotIn('volumes:', service)
        self.assertNotIn('ports:', service)

    def test_native_clock_is_forwarded_to_decoder_and_actual_scenario(self):
        self.native_clock()
        package_hash = hashlib.sha256(self.package.read_bytes()).hexdigest()
        db_hash = hashlib.sha256(self.db.read_bytes()).hexdigest()
        result = self.invoke(*self.cli())
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('status pass', self.events())
        work = self.run / 'interop'
        decoder = json.loads((work / 'decoder-args.json').read_text())
        self.assertEqual(decoder[-4:], ['--now', NATIVE_NOW, '--tz', 'Europe/Paris'])
        client = json.loads((work / 'out/fake-args.json').read_text())
        self.assertIn('TZ=Europe/Paris', client)
        scenario = json.loads((work / 'scenarios/rf4/package-interop.json').read_text())
        self.assertEqual(scenario['clock'],
            {'tz': 'Europe/Paris', 'start': NATIVE_NOW, 'mode': 'frozen'})
        self.assertEqual(package_hash, hashlib.sha256(self.package.read_bytes()).hexdigest())
        self.assertEqual(db_hash, hashlib.sha256(self.db.read_bytes()).hexdigest())

    def test_direct_proof_failure_never_reaches_oracle_build_or_open(self):
        variant_db = self.root / 'build/generated/variant.anki2'
        shutil.copyfile(self.db, variant_db)
        conn = sqlite3.connect(variant_db)
        conn.execute('UPDATE cards SET queue=-2')
        conn.commit()
        conn.close()
        self.pack(data=variant_db.read_bytes())
        self.invoke(*self.cli())
        self.assertIn('decoder-run', self.events())
        self.assertIn('failure pre-client-database', self.events())
        self.assertNotIn('tree', self.events())
        self.assertNotIn('status pass', self.events())

    def test_validation_before_oracle_jobs_and_private_diagnostics(self):
        self.package.unlink()
        result = self.invoke(*self.cli())
        self.assertNotIn('tree', self.events())
        self.assertIn('status error', self.events())
        self.assertNotIn(str(self.package), result.stderr)
        self.assertNotIn('referenced.bin', result.stderr)

    def test_oracle_nonzero_and_import_error_fail(self):
        result = self.invoke(*self.cli(), mode='nonzero')
        self.assertEqual(result.returncode, 0)
        self.assertIn('status fail', self.events())
        self.assertNotIn('status pass', self.events())

    def test_wrapper_import_rejection_fails(self):
        result = self.invoke(*self.cli(), mode='import-error')
        self.assertEqual(result.returncode, 0)
        self.assertIn('status fail', self.events())
        self.assertNotIn('status pass', self.events())

    def test_wrapper_dry_run_is_safe(self):
        self.invoke(*self.cli(), dry=True)
        self.assertIn('status pass', self.events())
        self.assertNotIn('tree', self.events())
        self.assertFalse((self.run / 'interop').exists())

    def test_utility_registration_and_entrypoint_dry_run(self):
        tiers = (REPO / 'tools/gate/tiers.yaml').read_text()
        self.assertNotIn('package-interop-check', tiers.split('utilities:')[0])
        self.assertIn('job: package-interop-check', tiers.split('utilities:')[1])
        self.invoke(dry=True)
        self.assertIn('status pass', self.events())
        self.assertNotIn('tree', self.events())


def fake(run, mode, args):
    run = Path(run)
    (run / 'interop/out/fake-args.json').write_text(json.dumps(args))
    report, golden = outputs(run)
    if mode == 'import-error':
        golden['observations']['steps'][2] = {'import': {'error': 'corrupt'}}
    save_outputs(run, report, golden)
    return 1 if mode == 'nonzero' else 0


if __name__ == '__main__':
    if len(sys.argv) > 1 and sys.argv[1] == '--decode':
        work = Path(sys.argv[2]) / 'interop'
        composition = sys.argv[3:]
        (work / 'decoder-args.json').write_text(json.dumps(composition))
        # Mirror the actual service CLI instead of inventing decoder context.
        service_args = composition[composition.index('package-interop-decoder') + 1:]
        assert service_args[:3] == ['python3', '/decoder.py', 'decode']
        options = dict(zip(service_args[3::2], service_args[4::2]))
        assert set(options) == {'--package', '--expected', '--output', '--media-inventory', '--now', '--tz'}
        assert options['--package'] == '/in/package.colpkg'
        assert options['--expected'] == '/in/collection.anki2'
        assert options['--output'] == '/out'
        assert options['--media-inventory'] == '/expected-media.json'
        interop.decode(argparse.Namespace(package=str(work / 'fixtures/package.colpkg'),
            expected=str(work / 'fixtures/collection.anki2'), output=str(work / 'decoded'),
            media_inventory=str(work / 'media-inventory.json'), now=options['--now'], tz=options['--tz']))
        sys.exit(0)
    if len(sys.argv) > 1 and sys.argv[1] == '--fake':
        sys.exit(fake(sys.argv[2], sys.argv[3], sys.argv[4:]))
    unittest.main()
