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


def synthetic_cycle(root):
    support, path, history = synthetic_native(root)
    template = next(iter(history['operations'].values()))
    history['operations'] = {}
    for serial, label, files, downloaded, uploaded in (
            (1, 'Import', 1, 4096, 6144), (2, 'AExport', 0, 0, 0), (3, 'Export', 1, 8192, 6144)):
        record = copy.deepcopy(template)
        identity = '%016x-0000000000000002' % serial
        record.update(id=identity, direction='Import' if serial == 1 else 'Export',
                      prepared_at=serial * 20 - 10, phase='Complete',
                      account=('b' if serial == 3 else 'a') * 64,
                      receipt=dict(completed_at=serial * 20, next_eligible_at=serial * 20),
                      safety_backups=[] if serial == 1 else [{}])
        record['media'].update(stage='Complete', exclusions_verified=True,
                               remote_outcome_unknown=label != 'AExport',
                               consent=dict(revision=5, generation=1, categories=1,
                                            account=record['account'], configured_endpoint=record['configured_endpoint']))
        record['media']['counters'].update(downloaded_files=files, uploaded_files=files,
                                           bytes_downloaded=downloaded, bytes_uploaded=uploaded,
                                           empty=['rf4-zero.bin'])
        history['operations'][identity] = record
    history.update(serial=3, active=None)
    # Historical future map intentionally differs from all new own receipts.
    history['cooldowns']['a' * 64] = dict(completed_at=1, next_eligible_at=3600001)
    path.write_text(json.dumps(dict(manualTransfers=history)))
    (support / 'Collection/collection.media/rf4-server.bin').write_bytes(b's' * 8192)
    (support / 'Collection/collection.media/rf4-device.bin').write_bytes(b'd' * 6144)
    return support, path, history


class MobileBindingTests(unittest.TestCase):
    udid = '11111111-2222-3333-4444-555555555555'
    appid = 'AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE'
    run_id = '20261024T010101Z-17'

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.home = Path(self.temp.name).resolve()
        support, _, _ = synthetic_native(self.home)
        self.device = self.home / 'Library/Developer/CoreSimulator/Devices' / self.udid
        self.root = self.device / 'data/Containers/Data/Application' / self.appid
        self.root.parent.mkdir(parents=True)
        support.parent.parent.rename(self.root)
        self.support = self.root / 'Library/Application Support'
        self.media = self.support / 'Collection/collection.media'
        for name in ('rf4-device.bin', 'rf4-zero.bin'):
            (self.media / name).unlink()
        self.inputs = self.home / 'mg-owned'
        self.inputs.mkdir(mode=0o700)
        self.path = self.inputs / 'native-reader-input.json'
        for name in ('reader_home', 'dedicated_fixture_identity'):
            guard = mock.patch.object(config, name, return_value=self.home)
            guard.start(); self.addCleanup(guard.stop)
        guard = mock.patch.object(config, 'dedicated_fixture_vm', side_effect=ValueError('app running'))
        guard.start(); self.addCleanup(guard.stop)
        self.calls = []
        self.booted = True
        self.installed = False
        def command(argv, stage):
            self.calls.append(argv)
            if argv[2] == 'list':
                return json.dumps(dict(devices={'runtime': [dict(udid=self.udid,
                    state='Booted' if self.booted else 'Shutdown', isAvailable=True)]})).encode()
            self.assertTrue(self.installed, 'container resolution must follow initial app launch')
            self.assertEqual(argv, ['/usr/bin/xcrun', 'simctl', 'get_app_container', self.udid,
                                    'com.antoinefink.meguri.dev', 'data'])
            return (str(self.root) + '\n').encode()
        guard = mock.patch.object(config, 'vm_child', side_effect=command)
        guard.start(); self.addCleanup(guard.stop)
        self.selection = config.mobile_reader_config(self.udid, self.run_id)
        self.path.write_text(json.dumps(self.selection)); self.path.chmod(0o600)
        self.selector = config.fingerprint(self.selection)
        self.bound = None

    def bootstrap(self):
        self.installed = True
        value = config.mobile_native_state(self.path, dict(action='bootstrap', selector=self.selector, bound=None))
        self.bound = value['bootstrap']
        return value

    def read(self):
        return config.mobile_native_state(self.path, dict(action='read', selector=self.selector, bound=self.bound))

    def resolvers(self):
        return [argv for argv in self.calls if argv[2] == 'get_app_container']

    def marker(self):
        context = config.native_context(self.support)
        return self.home / 'MeguriTestArchives/ManualTransferFixtureMedia' / config.fingerprint(context) / 'ownership' / (self.run_id + '.json')

    def test_setup_defers_then_exact_device_bundle_freezes_without_rediscovery(self):
        self.assertEqual(self.resolvers(), [])
        self.assertEqual(list(self.media.iterdir()), [])
        self.assertFalse((self.home / 'MeguriTestArchives').exists())
        first = self.bootstrap()
        self.assertEqual(set(first), {'selector', 'bootstrap'})
        self.assertEqual(first['selector'], self.selector)
        self.assertEqual(first['bootstrap']['context'], config.fingerprint(config.native_context(self.support)))
        self.assertEqual(first['bootstrap']['authority'], config.BOOTSTRAP_AUTHORITY)
        self.assertEqual(self.read()['bootstrap'], first['bootstrap'])
        with mock.patch.object(config, 'stage_media', side_effect=AssertionError('READ must not stage')), mock.patch.object(config, 'selected_mobile_context', side_effect=AssertionError('READ must not resolve')):
            self.assertEqual(self.read()['bootstrap'], first['bootstrap'])
        self.assertEqual(len(self.resolvers()), 1)
        self.root.rename(self.root.with_name('retained'))
        self.root.mkdir()
        with self.assertRaises(config.EvidenceFailure): self.read()
        self.assertEqual(len(self.resolvers()), 1)

    def test_bootstrap_only_media_and_marker_preserves_database_sidecar_policy(self):
        retained = {}
        for relative in ('Collection/collection.anki2', 'Collection/collection.anki2-wal',
                         'Collection/collection.anki2-shm', 'manual-transfer-policy.json',
                         'clock.json', 'ManualTransfers/manual-transfers.json'):
            path = self.support / relative
            if not path.exists(): path.write_bytes(b'preexisting-' + relative.encode())
            retained[path] = (path.read_bytes(), config.signature(path.stat()))
        before = {p.relative_to(self.root) for p in self.root.rglob('*')}
        self.bootstrap()
        after = {p.relative_to(self.root) for p in self.root.rglob('*')}
        self.assertEqual(after - before, {Path('Library/Application Support/Collection/collection.media') / name
            for name in ('rf4-device.bin', 'rf4-zero.bin')})
        self.assertEqual({p.name for p in self.media.iterdir()}, {'rf4-device.bin', 'rf4-zero.bin'})
        self.assertEqual((self.media / 'rf4-device.bin').read_bytes(), config.recipe_bytes(self.run_id, 'device', 6144))
        self.assertEqual((self.media / 'rf4-zero.bin').read_bytes(), b'')
        self.assertEqual(self.marker().stat().st_mode & 0o777, 0o400)
        for path, original in retained.items(): self.assertEqual((path.read_bytes(), config.signature(path.stat())), original)

    def test_initial_history_absence_is_not_synthesized_or_required(self):
        sidecar = self.support / 'ManualTransfers/manual-transfers.json'
        sidecar.unlink(); sidecar.parent.rmdir()
        with mock.patch.object(config, 'native_state', side_effect=AssertionError('no history before Import')):
            self.bootstrap()
        self.assertFalse(sidecar.exists())
        self.assertFalse(sidecar.parent.exists())
        with self.assertRaises((ValueError, OSError)): self.read()
        self.assertFalse(sidecar.exists())

    def test_bootstrap_replay_and_failed_attempt_never_resolve_or_stage_again(self):
        self.bootstrap()
        with mock.patch.object(config, 'stage_media', side_effect=AssertionError('no restage')):
            with self.assertRaises(config.EvidenceFailure): self.bootstrap()
        self.assertEqual(len(self.resolvers()), 1)

    def test_lost_bound_record_or_attempt_never_resolves_again(self):
        self.bootstrap()
        for name in ('native-reader-bound.json', 'native-reader-attempt'):
            path = self.inputs / name
            saved = path.with_name(name + '-saved')
            path.rename(saved)
            with mock.patch.object(config, 'stage_media', side_effect=AssertionError('no restage')):
                with self.assertRaises((ValueError, OSError)): self.read()
            saved.rename(path)
            self.assertEqual(len(self.resolvers()), 1)

    def test_replaced_binding_with_identical_bytes_refuses(self):
        self.bootstrap()
        path = self.inputs / 'native-reader-bound.json'
        saved = path.with_name('saved-binding')
        path.rename(saved); path.write_bytes(saved.read_bytes()); path.chmod(0o400)
        with self.assertRaises(config.EvidenceFailure): self.read()
        self.assertEqual(len(self.resolvers()), 1)

    def test_initial_lost_marker_or_media_never_restages_or_retries_bind(self):
        self.bootstrap()
        for index, target in enumerate((self.media / 'rf4-device.bin', self.media / 'rf4-zero.bin', self.marker())):
            saved = target.with_name(target.name + '-retained')
            target.rename(saved)
            with mock.patch.object(config, 'stage_media', side_effect=AssertionError('must not restage')):
                with self.assertRaises(config.EvidenceFailure) as error: self.read()
            self.assertEqual(error.exception.receipt['failure']['stage'], 'marker' if index == 2 else 'media')
            self.assertFalse(target.exists())
            saved.rename(target)
        self.assertEqual(len(self.resolvers()), 1)

    def test_initial_changed_marker_provenance_refuses_without_adoption(self):
        self.bootstrap()
        marker = self.marker()
        marker.chmod(0o600)
        marker.write_text(json.dumps(config.fixture_marker(config.native_context(self.support), '20261024T020202Z-18')))
        marker.chmod(0o400)
        with mock.patch.object(config, 'stage_media', side_effect=AssertionError('must not restage')):
            with self.assertRaises(config.EvidenceFailure) as error: self.read()
        self.assertEqual(error.exception.receipt['failure']['stage'], 'marker')
        self.assertEqual(len(self.resolvers()), 1)

    def test_initial_changed_media_refuses_recipe_adoption(self):
        media = self.media / 'rf4-device.bin'
        media.write_bytes(b'private-unexpected-media')
        with self.assertRaises(config.EvidenceFailure) as error: self.bootstrap()
        self.assertEqual(error.exception.receipt['failure']['stage'], 'media')
        self.assertEqual(media.read_bytes(), b'private-unexpected-media')
        self.assertFalse(self.marker().exists())
        with self.assertRaises(config.EvidenceFailure): self.bootstrap()
        self.assertEqual(len(self.resolvers()), 1)

    def test_old_context_marker_and_matching_recipe_bytes_cannot_be_adopted(self):
        old = config.native_context(self.support)
        owners = self.home / 'MeguriTestArchives/ManualTransferFixtureMedia' / config.fingerprint(old) / 'ownership'
        owners.mkdir(parents=True)
        (owners / (self.run_id + '.json')).write_text(json.dumps(config.fixture_marker(old, self.run_id)))
        replacement = self.root.with_name('BBBBBBBB-BBBB-CCCC-DDDD-EEEEEEEEEEEE')
        self.root.rename(replacement); self.root.mkdir()
        # Keep old root readable; the resolver still selects the new empty root.
        (self.root / 'Library/Application Support/Collection/collection.media').mkdir(parents=True)
        self.support = self.root / 'Library/Application Support'
        self.media = self.support / 'Collection/collection.media'
        (self.media / 'rf4-device.bin').write_bytes(config.recipe_bytes(self.run_id, 'device', 6144))
        with self.assertRaises(config.EvidenceFailure): self.bootstrap()
        self.assertEqual(json.loads((owners / (self.run_id + '.json')).read_bytes()), config.fixture_marker(old, self.run_id))
        self.assertTrue(replacement.exists())

    def test_bound_marker_change_refuses(self):
        self.bootstrap()
        marker = self.marker(); marker.chmod(0o600); marker.write_bytes(b'private-changed-marker')
        with self.assertRaises(config.EvidenceFailure) as error: self.read()
        self.assertEqual(error.exception.receipt['failure']['stage'], 'marker')
        self.assertEqual(len(self.resolvers()), 1)

    def test_marker_replacement_with_identical_recipe_provenance_refuses(self):
        self.bootstrap()
        marker = self.marker()
        saved = marker.with_name('retained-marker')
        marker.rename(saved); marker.write_bytes(saved.read_bytes()); marker.chmod(0o400)
        with mock.patch.object(config, 'stage_media', side_effect=AssertionError('no restage')):
            with self.assertRaises(config.EvidenceFailure) as error: self.read()
        self.assertEqual(error.exception.receipt['failure']['stage'], 'marker')
        self.assertEqual(len(self.resolvers()), 1)

    def test_generic_live_staging_cannot_use_read_capability(self):
        self.bootstrap()
        context = config.native_context(self.support)
        authority = config.MobileBootstrap(self.selection, self.home, self.device, context,
                                           config.mobile_applications(self.device))
        with self.assertRaises(config.EvidenceFailure):
            config.stage_media(self.support, self.run_id, _bootstrap=authority)
        with mock.patch.object(config, 'dedicated_fixture_vm', return_value=self.home):
            with self.assertRaises(ValueError): config.stage_media(self.support, self.run_id)

    def test_bound_media_loss_or_change_refuses_without_restaging(self):
        self.bootstrap()
        media = self.media / 'rf4-device.bin'
        before = media.read_bytes(); media.unlink()
        with mock.patch.object(config, 'stage_media', side_effect=AssertionError('must not restage')):
            with self.assertRaises(config.EvidenceFailure) as error: self.read()
            self.assertEqual(error.exception.receipt['failure']['stage'], 'media')
            self.assertFalse(media.exists())
            media.write_bytes(b'private-changed-media')
            with self.assertRaises(config.EvidenceFailure): self.read()
        self.assertEqual(len(self.resolvers()), 1)
        self.assertNotEqual(media.read_bytes(), before)

    def test_config_device_selector_and_bound_fingerprint_changes_refuse(self):
        self.bootstrap()
        self.booted = False
        with self.assertRaises(config.EvidenceFailure): self.read()
        self.booted = True
        original_bound = self.bound; self.bound = dict(self.bound, context='f' * 64)
        with self.assertRaises(config.EvidenceFailure): self.read()
        self.bound = original_bound
        with self.assertRaises(config.EvidenceFailure):
            config.mobile_native_state(self.path, dict(action='read', selector='f' * 64, bound=self.bound))
        before = self.path.read_bytes(); self.path.unlink()
        with self.assertRaises((ValueError, OSError)): self.read()
        self.path.write_bytes(before); self.path.chmod(0o600)
        self.path.write_bytes(b'private-changed-config')
        with self.assertRaises(config.EvidenceFailure): self.read()
        self.assertEqual(len(self.resolvers()), 1)

    def test_application_membership_or_device_inode_change_refuses_no_resolver(self):
        self.bootstrap()
        self.root.with_name('retained-sibling').mkdir()
        with self.assertRaises(config.EvidenceFailure): self.read()
        self.device.rename(self.device.with_name('retained-device')); self.device.mkdir()
        with self.assertRaises(config.EvidenceFailure): self.read()
        self.assertEqual(len(self.resolvers()), 1)

    def test_membership_change_during_first_resolution_refuses_readable_old_root(self):
        self.installed = True
        command = config.vm_child.side_effect
        context = config.native_context(self.support)
        original_state = config.native_state(context)
        def changing(argv, stage):
            result = command(argv, stage)
            if argv[2] == 'get_app_container': self.root.with_name('new-container').mkdir()
            return result
        with mock.patch.object(config, 'vm_child', side_effect=changing), mock.patch.object(config, 'stage_media', side_effect=AssertionError('race before any write')):
            with self.assertRaises(config.EvidenceFailure) as error: self.bootstrap()
        self.assertEqual(error.exception.receipt['failure']['stage'], 'context')
        self.assertEqual(config.native_state(context), original_state)
        self.assertFalse(self.marker().exists())
        with self.assertRaises(config.EvidenceFailure): self.bootstrap()
        self.assertEqual(len(self.resolvers()), 1)

    def test_namespace_change_during_bootstrap_refuses_old_readable_root(self):
        original = config.stage_media
        context = config.native_context(self.support)
        before = config.native_state(context)
        def changing(*args, **kwargs):
            value = original(*args, **kwargs)
            self.root.with_name('new-container').mkdir()
            return value
        with mock.patch.object(config, 'stage_media', side_effect=changing):
            with self.assertRaises(config.EvidenceFailure) as error: self.bootstrap()
        self.assertEqual(error.exception.receipt['failure']['stage'], 'context')
        self.assertFalse((self.inputs / 'native-reader-bound.json').exists())
        self.assertEqual(config.native_state(context)['operations'], before['operations'])
        with self.assertRaises(config.EvidenceFailure): self.bootstrap()
        self.assertEqual(len(self.resolvers()), 1)

    def assert_read_mutation_refused(self):
        self.bootstrap()
        context = config.native_context(self.support)
        read = config.native_state
        original_state = read(context)
        def changing(actual):
            value = read(actual)
            self.assertEqual(value, original_state)
            self.root.with_name('new-container').mkdir()
            return value
        with mock.patch.object(config, 'native_state', side_effect=changing) as reader:
            with self.assertRaises(config.EvidenceFailure) as error: self.read()
            reader.assert_called_once_with(context)
        self.assertEqual(error.exception.receipt['failure']['stage'], 'context')
        self.assertEqual(read(context), original_state)
        self.assertEqual(len(self.resolvers()), 1)
        with self.assertRaises(config.EvidenceFailure): self.read()
        self.assertEqual(len(self.resolvers()), 1)

    def test_membership_change_during_initial_read_refuses_readable_old_root(self):
        self.assert_read_mutation_refused()

    def test_membership_change_during_bound_read_refuses_readable_old_root(self):
        self.bootstrap(); self.read()
        context = config.native_context(self.support)
        original = config.native_state
        def changing(actual):
            value = original(actual)
            self.root.with_name('new-container').mkdir()
            return value
        with mock.patch.object(config, 'native_state', side_effect=changing):
            with self.assertRaises(config.EvidenceFailure): self.read()
        self.assertEqual(len(self.resolvers()), 1)
        self.assertTrue(Path(context['root']).is_dir())

    def test_invalid_selector_payload_and_failed_resolver_are_one_attempt(self):
        self.installed = True
        with self.assertRaises(ValueError):
            config.mobile_native_state(self.path, dict(action='bootstrap', selector=self.selector, bound=None, path='/private'))
        self.assertEqual(self.resolvers(), [])
        with mock.patch.object(config, 'selected_mobile_context', side_effect=config.EvidenceFailure('resolver', child_rc=70)) as resolve:
            with self.assertRaises(config.EvidenceFailure) as error: self.bootstrap()
            self.assertEqual(error.exception.receipt, config.failure('resolver', child_rc=70))
            with self.assertRaises(config.EvidenceFailure): self.bootstrap()
            self.assertEqual(resolve.call_count, 1)

    def test_read_without_bootstrap_cannot_resolve_or_stage(self):
        with mock.patch.object(config, 'stage_media', side_effect=AssertionError('READ cannot stage')), mock.patch.object(config, 'selected_mobile_context', side_effect=AssertionError('READ cannot resolve')):
            with self.assertRaises(config.EvidenceFailure): self.read()
        self.assertEqual(self.resolvers(), [])
        self.assertFalse((self.inputs / 'native-reader-attempt').exists())


class SafeDiagnosticTests(unittest.TestCase):
    def test_fixed_codes_exact_shape_and_numeric_bounds_match_swift_parser(self):
        for stage in config.FAILURE_STAGES:
            value = config.failure(stage, child_rc=-9, http_status=503, timeout_ms=8000, script_line=100)
            self.assertEqual(config.validate_failure(value), value)
        for change in (dict(stage='private-path-or-nonce'), dict(child_rc=True), dict(child_rc=256),
                       dict(http_status=600), dict(timeout_ms=120001), dict(script_line=-1),
                       dict(stderr='private-error'), dict(nonce='never-output')):
            value = config.failure('read'); value['failure'].update(change)
            with self.assertRaises(ValueError): config.validate_failure(value)
        for raw in ({}, dict(failure={}), dict(failure='private'), dict(failure={}, extra=True)):
            with self.assertRaises(ValueError): config.validate_failure(raw)

    def test_os_read_schema_and_timeout_reduction_never_includes_raw_error(self):
        for stage, exception, expected in (('schema', OSError('private-path'), 'read'),
                                           ('schema', ValueError('private-record'), 'schema'),
                                           ('marker', ValueError('private-nonce'), 'marker')):
            with self.assertRaises(config.EvidenceFailure) as error:
                config.controlled(stage, mock.Mock(side_effect=exception))
            self.assertEqual(error.exception.receipt, config.failure(expected))
        with self.assertRaises(config.EvidenceFailure) as error:
            config.vm_child([sys.executable, '-c', 'import time; time.sleep(60)'], 'resolver')
        self.assertEqual(error.exception.receipt, config.failure('timeout', timeout_ms=2000))
        with self.assertRaises(config.EvidenceFailure) as error:
            config.vm_child([sys.executable, '-c', 'raise SystemExit(70)'], 'resolver')
        self.assertEqual(error.exception.receipt, config.failure('resolver', child_rc=70))


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
        diagnostic = config.validate_failure(json.loads(child.stdout))
        self.assertEqual(diagnostic['failure']['stage'], 'schema')
        self.assertNotIn(b'never-export-private-input', child.stdout)
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


class SameLoginCycleTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.support, self.path, self.history = synthetic_cycle(Path(self.temp.name).resolve())

    def save(self):
        self.path.write_text(json.dumps(dict(manualTransfers=self.history)))

    def test_equal_time_own_receipts_and_old_future_map_are_preserved(self):
        proof = config.native_proof(self.support)
        self.assertEqual(set(proof['stages']), {'Import', 'AExport', 'Export'})
        self.assertTrue(all(proof['cycle'].values()))
        state = config.native_state(config.native_context(self.support))
        a = state['operations']['AExport']
        self.assertNotEqual(a['id'], state['operations']['Export']['id'])
        self.assertEqual(a['account'], state['operations']['Import']['account'])
        self.assertEqual(a['receipt'], dict(completed_at=40, next_eligible_at=40))
        self.assertEqual(a['success_cooldown'], dict(completed_at=1, next_eligible_at=3600001))
        self.assertNotIn('reserved.invalid', json.dumps(proof))
        self.assertNotIn('a' * 64, json.dumps(proof))

    def test_partial_stage_keeps_nonnull_legacy_map_without_own_receipt(self):
        # B has not started. A's partial Export retains pre-operation history.
        records = self.history['operations']
        del records['0000000000000003-0000000000000002']
        a = records['0000000000000002-0000000000000002']
        a.update(phase='MediaIncomplete', receipt=None)
        a['media'].update(stage='Cancelled', consent=None, remote_outcome_unknown=True)
        self.save()
        before = copy.deepcopy(self.history['cooldowns'])
        state = config.native_state(config.native_context(self.support))
        self.assertIsNone(state['operations']['AExport']['receipt'])
        self.assertEqual(state['operations']['AExport']['success_cooldown'], before['a' * 64])
        self.assertEqual(json.loads(self.path.read_text())['manualTransfers']['cooldowns'], before)
        with self.assertRaises(ValueError):
            config.native_proof(self.support)

    def test_missing_wrong_account_intervening_unfinished_or_unordered_a_rejected(self):
        original = copy.deepcopy(self.history)
        for mutation in ('missing', 'account', 'endpoint', 'intervening', 'unfinished', 'order', 'hour', 'future', 'backup'):
            with self.subTest(mutation=mutation):
                self.history = copy.deepcopy(original)
                records = self.history['operations']
                a = records['0000000000000002-0000000000000002']
                if mutation == 'missing':
                    del records[a['id']]
                elif mutation == 'account':
                    a['account'] = 'c' * 64
                elif mutation == 'endpoint':
                    a['configured_endpoint'] = 'http://other.invalid/'
                elif mutation == 'intervening':
                    extra = copy.deepcopy(a)
                    extra['id'] = '0000000000000004-0000000000000002'
                    records[extra['id']] = extra
                    self.history['serial'] = 4
                elif mutation == 'unfinished':
                    a.update(phase='MediaIncomplete', receipt=None)
                elif mutation == 'order':
                    a['prepared_at'] = 19
                elif mutation == 'hour':
                    a['receipt'] = dict(completed_at=3600020, next_eligible_at=3600020)
                    b = records['0000000000000003-0000000000000002']
                    b['prepared_at'] = 3600030
                    b['receipt'] = dict(completed_at=3600040, next_eligible_at=3600040)
                elif mutation == 'future':
                    a['receipt']['next_eligible_at'] += 3600000
                else:
                    a['safety_backups'] = []
                self.save()
                with self.assertRaises(ValueError):
                    config.native_proof(self.support)

    def test_exact_a_zero_and_b_positive_counts_bytes_and_history_rejected_on_change(self):
        original = copy.deepcopy(self.history)
        for serial in (1, 2, 3):
            for field in ('downloaded_files', 'uploaded_files', 'bytes_downloaded', 'bytes_uploaded',
                          'downloaded_deletions', 'uploaded_deletions', 'awaiting'):
                with self.subTest(serial=serial, field=field):
                    self.history = copy.deepcopy(original)
                    self.history['operations']['%016x-0000000000000002' % serial]['media']['counters'][field] += 1
                    self.save()
                    with self.assertRaises(ValueError):
                        config.native_proof(self.support)
        self.history = copy.deepcopy(original)
        self.history['cooldowns']['a' * 64] = dict(completed_at=10, next_eligible_at=9)
        self.save()
        with self.assertRaises(ValueError):
            config.native_proof(self.support)
        self.history = original; self.save()
        (self.support / 'Collection/collection.media/rf4-device.bin').write_bytes(b'wrong-size')
        with self.assertRaises(ValueError):
            config.native_proof(self.support)


class MediaStagingTests(unittest.TestCase):
    """Only made-up directories; no VM, application process, clocks or policy edits."""
    old_run = '20261023T010101Z-11'
    new_run = '20261024T010101Z-17'

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.home = Path(self.temp.name).resolve()
        self.support = self.home / 'Library/Containers/com.antoinefink.meguri.dev/Data/Library/Application Support'
        self.media = self.support / 'Collection/collection.media'
        self.media.mkdir(parents=True)
        (self.support / 'ManualTransfers').mkdir()
        self.sentinels = []
        for name in ('Collection/collection.anki2', 'Collection/collection.anki2-wal',
                     'Collection/collection.anki2-shm', 'Collection/collection.anki2-journal',
                     'Collection/meguri-sidecar.json', 'ManualTransfers/manual-transfers.json',
                     'ManualTransfers/retained-receipt', 'Collection/collection.media/unrelated.bin'):
            path = self.support / name
            path.write_bytes(('made-up-untouched:' + name).encode())
            self.sentinels.append(path)
        guard = mock.patch.object(config, 'dedicated_fixture_vm', return_value=self.home)
        guard.start()
        self.addCleanup(guard.stop)

    def facts(self):
        return {str(path.relative_to(self.support)): (path.read_bytes(), path.stat().st_ino)
                for path in self.sentinels}

    def storage(self):
        return self.home / 'MeguriTestArchives/ManualTransferFixtureMedia' / config.fingerprint(config.native_context(self.support))

    def test_stage_and_repeat_preserve_db_wal_sidecar_policy_receipts_and_unrelated_media(self):
        before = self.facts()
        config.stage_media(self.support, self.old_run)
        # Independent reference recipe, rather than adopting observed bytes.
        expected = b''.join(hashlib.sha256(('RF4:%s:device:%d' % (self.old_run, block)).encode()).digest()
                            for block in range(192))
        self.assertEqual((self.media / 'rf4-device.bin').read_bytes(), expected)
        self.assertEqual((self.media / 'rf4-zero.bin').read_bytes(), b'')
        self.assertFalse((self.media / 'rf4-server.bin').exists())
        inode = (self.media / 'rf4-device.bin').stat().st_ino
        config.stage_media(self.support, self.old_run)
        self.assertEqual((self.media / 'rf4-device.bin').stat().st_ino, inode)
        self.assertEqual(self.facts(), before)
        self.assertNotIn('MeguriTestArchives', str(self.support))

    def test_recipe_matching_bytes_without_prior_marker_are_not_adopted(self):
        path = self.media / 'rf4-device.bin'
        path.write_bytes(config.recipe_bytes(self.old_run, 'device', 6144))
        before = path.read_bytes(), path.stat().st_ino, self.facts()
        with self.assertRaises(ValueError):
            config.stage_media(self.support, self.old_run)
        self.assertEqual((path.read_bytes(), path.stat().st_ino, self.facts()), before)
        self.assertFalse((self.home / 'MeguriTestArchives').exists())

    def test_known_prior_device_and_server_are_archived_with_bytes_inode_and_marker_preserved(self):
        config.stage_media(self.support, self.old_run)
        (self.media / 'rf4-server.bin').write_bytes(config.recipe_bytes(self.old_run, 'server-c', 4096))
        originals = {name: ((self.media / name).read_bytes(), (self.media / name).stat().st_ino)
                     for name in ('rf4-server.bin', 'rf4-device.bin')}
        marker = self.storage() / 'ownership' / (self.old_run + '.json')
        marker_before = marker.read_bytes(), marker.stat().st_ino
        before = self.facts()
        config.stage_media(self.support, self.new_run)
        for name, (data, inode) in originals.items():
            archive = self.storage() / 'originals' / self.new_run / (name + '-' + hashlib.sha256(data).hexdigest())
            self.assertEqual((archive.read_bytes(), archive.stat().st_ino), (data, inode))
        self.assertEqual((marker.read_bytes(), marker.stat().st_ino), marker_before)
        self.assertFalse((self.media / 'rf4-server.bin').exists())
        self.assertEqual(self.facts(), before)

    def test_unknown_collision_symlink_and_inconsistent_marker_refuse_without_replacement(self):
        config.stage_media(self.support, self.old_run)
        device = self.media / 'rf4-device.bin'
        device.write_bytes(b'unknown-do-not-print')
        before = device.read_bytes(), device.stat().st_ino, self.facts()
        with self.assertRaises(ValueError):
            config.stage_media(self.support, self.new_run)
        self.assertEqual((device.read_bytes(), device.stat().st_ino, self.facts()), before)
        device.unlink()  # Synthetic test setup only.
        device.symlink_to(self.sentinels[0])
        with self.assertRaises((OSError, ValueError)):
            config.stage_media(self.support, self.new_run)
        device.unlink()
        marker = self.storage() / 'ownership' / (self.old_run + '.json')
        marker.chmod(0o600)
        marker.write_text('{"purpose":"invented"}')
        with self.assertRaises(ValueError):
            config.stage_media(self.support, self.new_run)
        self.assertEqual(self.facts(), before[2])

    def test_interruption_after_archiving_retains_original_and_retry_uses_recipe(self):
        config.stage_media(self.support, self.old_run)
        original = (self.media / 'rf4-device.bin').read_bytes()
        rename = config.rename_new

        def interrupt(source_fd, source, destination_fd, destination):
            if destination == 'rf4-device.bin':
                raise OSError('synthetic interruption')
            return rename(source_fd, source, destination_fd, destination)

        with mock.patch.object(config, 'rename_new', side_effect=interrupt):
            with self.assertRaises(OSError):
                config.stage_media(self.support, self.new_run)
        retained = self.storage() / 'originals' / self.new_run / ('rf4-device.bin-' + hashlib.sha256(original).hexdigest())
        self.assertEqual(retained.read_bytes(), original)
        config.stage_media(self.support, self.new_run)
        self.assertEqual(retained.read_bytes(), original)
        self.assertEqual((self.media / 'rf4-device.bin').read_bytes(), config.recipe_bytes(self.new_run, 'device', 6144))

    def test_existing_archive_target_and_unsupported_atomic_rename_never_overwrite_original(self):
        config.stage_media(self.support, self.old_run)
        original = (self.media / 'rf4-device.bin').read_bytes()
        saved = self.storage() / 'originals' / self.new_run
        saved.mkdir()
        target = saved / ('rf4-device.bin-' + hashlib.sha256(original).hexdigest())
        target.write_bytes(b'preexisting-archive')
        with self.assertRaises(OSError):
            config.stage_media(self.support, self.new_run)
        self.assertEqual(target.read_bytes(), b'preexisting-archive')
        self.assertEqual((self.media / 'rf4-device.bin').read_bytes(), original)
        with mock.patch.object(config, 'rename_new', side_effect=ValueError('unavailable')):
            with self.assertRaises(ValueError):
                config.stage_media(self.support, self.new_run)
        self.assertEqual((self.media / 'rf4-device.bin').read_bytes(), original)


class RepairArchiveTests(unittest.TestCase):
    """Synthetic VM metadata only; archive reads/writes stay in temporary data."""
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.run = self.root / 'build/mg/manual-transfer-check' / config.FAILED_FIXTURE_RUN
        self.run.mkdir(parents=True)
        self.result = dict(root=str(self.root), job='manual-transfer-check', run_id=config.FAILED_FIXTURE_RUN,
                           args=['--platform', 'mac'], dry_run=False, status='fail',
                           failures=[dict(id='manual-transfer/execution')],
                           details=dict(native_transfer_complete=False), message='never-print-this')
        self.seed = dict(notes=1, cards=1, reviews=0, deckID=1, noteID=123, cardID=456,
                         server_time_ms=1000, distinct_accounts=True, empty_nondefault_b=True,
                         media_seeded=True, media_names=list(config.MEDIA_NAMES))
        self.save()

    def save(self):
        (self.run / 'result.json').write_text(json.dumps(self.result))
        (self.run / 'seed.json').write_text(json.dumps(self.seed))

    def fixture(self):
        self.home = self.root / 'home'
        self.support, self.policy, self.history = synthetic_native(self.home)
        self.media = self.support / 'Collection/collection.media'
        (self.media / 'rf4-device.bin').write_bytes(config.recipe_bytes(config.FAILED_FIXTURE_RUN, 'device', 6144))
        active = self.history['operations'][self.history['active']]
        active.update(phase='MediaIncomplete', acceptance='NotSent', account=config.repair_account(),
                      configured_endpoint=config.REPAIR_ENDPOINT, target_endpoint=config.REPAIR_ENDPOINT)
        old = copy.deepcopy(active)
        old.update(id='0000000000000002-0000000000000003', direction='Export', phase='Complete',
                   acceptance='ExactOk', account='b' * 64, receipt=dict(completed_at=1, next_eligible_at=2))
        old['media']['stage'] = 'Complete'
        self.history['operations'][old['id']] = old
        self.history['serial'] = 2
        self.history['cooldowns']['b' * 64] = old['receipt']
        self.write_policy()
        self.target = self.home / config.REPAIR_ARCHIVE
        self.patcher = mock.patch.object(config, 'dedicated_fixture_vm', return_value=self.home)
        self.patcher.start()
        self.addCleanup(self.patcher.stop)

    def write_policy(self):
        self.policy.write_text(json.dumps(dict(manualTransfers=self.history)))

    def archive(self):
        return config.archive_stale_fixture_state(config.repair_source(self.root, config.FAILED_FIXTURE_RUN))

    def assert_blocked(self):
        before = (self.support.stat().st_ino, self.policy.read_bytes())
        with self.assertRaises((ValueError, OSError)):
            self.archive()
        self.assertEqual((self.support.stat().st_ino, self.policy.read_bytes()), before)

    def test_import_notsent_lossless_rename_retains_unknown_files_links_receipts_and_inode(self):
        self.fixture()
        self.assertEqual(self.history['operations'][self.history['active']]['acceptance'], 'NotSent')
        outside = self.root / 'outside'
        outside.write_bytes(b'never-follow-external-link')
        link = self.support / 'unknown-link'
        link.symlink_to(outside)
        (self.support / 'unknown-directory-link').symlink_to(self.root, target_is_directory=True)
        (self.support / '.DS_Store').write_bytes(b'unknown-OS-metadata')
        (self.support / 'ManualTransfers/old-artifact').write_bytes(b'old-artifact-unchanged')
        before = {}
        # Test-only snapshot; never follow a link, including directory links.
        for parent, directories, files in os.walk(self.support, followlinks=False):
            for name in directories + files:
                path = Path(parent) / name
                info = path.lstat()
                before[str(path.relative_to(self.support))] = (
                    info.st_ino, info.st_mode, info.st_mtime_ns,
                    os.readlink(path) if path.is_symlink() else None if path.is_dir() else path.read_bytes())
        inode, policy = self.support.stat().st_ino, self.policy.read_bytes()
        real_open = config.os.open
        def guarded_open(path, *args, **kwargs):
            self.assertNotIn(str(path), (str(outside), 'unknown-link', 'unknown-directory-link', '.DS_Store', 'old-artifact'))
            return real_open(path, *args, **kwargs)
        with mock.patch.object(config.os, 'open', side_effect=guarded_open), mock.patch.object(
                config, 'rename_new', wraps=config.rename_new) as rename:
            proof = self.archive()
        self.assertEqual(rename.call_count, 1)
        self.assertTrue(proof['archived'])
        self.assertEqual(proof['purpose'], 'environment-repair-not-M8')
        self.assertEqual(proof['status'], 'archived')
        self.assertEqual(self.target.stat().st_ino, inode)
        self.assertFalse(self.support.exists())
        self.assertEqual((self.target / 'ManualTransfers/manual-transfers.json').read_bytes(), policy)
        for name, fact in before.items():
            path = self.target / name
            info = path.lstat()
            self.assertEqual((info.st_ino, info.st_mode, info.st_mtime_ns,
                              os.readlink(path) if path.is_symlink() else None if path.is_dir() else path.read_bytes()), fact)
        self.assertEqual(proof['policy_sha256_before'], proof['policy_sha256_after'])
        self.assertEqual(proof['support_inode'], proof['archived_inode'])
        self.assertTrue(proof['state_guards_proven'] and proof['cooldown_guards_proven'])
        self.assertEqual(proof['archive_location'], '~/' + config.REPAIR_ARCHIVE)
        self.assertEqual(outside.read_bytes(), b'never-follow-external-link')
        for forbidden in (str(self.root), 'meguri-c', config.REPAIR_ENDPOINT,
                          'native_transfer_complete', 'peer_proof', 'Resume'):
            self.assertNotIn(forbidden, json.dumps(proof))
        # No cleanup hook has the persistent archive; source absence is retained.
        self.assertTrue(self.target.is_dir())

    def test_unexpired_malformed_or_unknown_cooldown_and_old_receipt_refuse(self):
        self.fixture()
        expired = copy.deepcopy(self.history)
        bad = (dict(completed_at=1, next_eligible_at=time.time_ns() // 1_000_000 + 3600000),
               dict(completed_at=1, next_eligible_at='unknown'),
               dict(completed_at=1, next_eligible_at=2, injected_clock=0), None)
        for value in bad:
            with self.subTest(value=value):
                self.history = copy.deepcopy(expired)
                self.history['cooldowns']['b' * 64] = value
                self.write_policy(); self.assert_blocked()
        self.history = copy.deepcopy(expired)
        old = self.history['operations']['0000000000000002-0000000000000003']
        old['receipt']['next_eligible_at'] = time.time_ns() // 1_000_000 + 3600000
        self.write_policy(); self.assert_blocked()

    def test_other_import_acceptance_states_refuse_without_moving_support(self):
        self.fixture()
        for acceptance in ('PossiblyAccepted', 'Refused', 'ExactOk'):
            with self.subTest(acceptance=acceptance):
                self.history['operations'][self.history['active']]['acceptance'] = acceptance
                self.write_policy()
                self.assert_blocked()
                self.assertFalse(self.target.exists())

    def test_exact_active_import_account_endpoint_and_zero_effects_required(self):
        self.fixture()
        original = copy.deepcopy(self.history)
        for field, value in (('phase', 'CollectionComplete'), ('scope', 'CollectionOnly'),
                             ('account', 'c' * 64), ('direction', 'Export'),
                             ('configured_endpoint', 'http://unowned.invalid/'),
                             ('collection_complete', False), ('receipt', dict(completed_at=1, next_eligible_at=2))):
            with self.subTest(field=field):
                self.history = copy.deepcopy(original)
                self.history['operations'][self.history['active']][field] = value
                self.write_policy(); self.assert_blocked()
        for field, value in (('stage', 'Cancelled'), ('remote_outcome_unknown', True),
                             ('consent', dict(revision=5, generation=1, categories=1,
                                              account=config.repair_account(), configured_endpoint=config.REPAIR_ENDPOINT))):
            self.history = copy.deepcopy(original)
            self.history['operations'][self.history['active']]['media'][field] = value
            self.write_policy(); self.assert_blocked()
        for field in config.NUMERIC_COUNTERS:
            self.history = copy.deepcopy(original)
            self.history['operations'][self.history['active']]['media']['counters'][field] = 1
            self.write_policy(); self.assert_blocked()
        self.history = copy.deepcopy(original)
        self.history['active'] = None
        self.write_policy(); self.assert_blocked()

    def test_database_sidecar_leftovers_and_unknown_media_refuse(self):
        self.fixture()
        for name in ('collection.anki2', 'collection.anki2-wal', 'collection.anki2-shm',
                     'collection.anki2-journal', 'meguri-sidecar.json'):
            path = self.support / 'Collection' / name
            path.write_bytes(b'untouched-synthetic-evidence')
            self.assert_blocked()
            self.assertEqual(path.read_bytes(), b'untouched-synthetic-evidence')
            path.unlink()
        for name in ('rf4-server.bin', 'unknown.bin'):
            path = self.media / name
            path.write_bytes(b'unknown')
            self.assert_blocked(); path.unlink()
        (self.media / 'rf4-zero.bin').write_bytes(b'nonempty')
        self.assert_blocked()
        (self.media / 'rf4-zero.bin').write_bytes(b'')
        (self.media / 'rf4-device.bin').write_bytes(config.recipe_bytes('20261024T010101Z-17', 'device', 6144))
        self.assert_blocked()

    def test_existing_archive_symlink_parent_and_unavailable_atomic_rename_refuse(self):
        self.fixture()
        self.target.mkdir(parents=True)
        (self.target / 'receipt').write_bytes(b'existing-archive')
        self.assert_blocked()
        self.assertEqual((self.target / 'receipt').read_bytes(), b'existing-archive')
        (self.target / 'receipt').unlink(); self.target.rmdir()
        with mock.patch.object(config, 'rename_new', side_effect=ValueError('unavailable')):
            self.assert_blocked()
        archives = self.target.parent
        archives.rmdir()
        elsewhere = self.root / 'elsewhere'; elsewhere.mkdir()
        archives.symlink_to(elsewhere, target_is_directory=True)
        self.assert_blocked()
        self.assertEqual(list(elsewhere.iterdir()), [])

    def test_support_symlink_and_wrong_source_proof_refuse_without_moving(self):
        self.fixture()
        proof = config.repair_source(self.root, config.FAILED_FIXTURE_RUN)
        proof['source_run'] = '20261024T010101Z-17'
        with self.assertRaises(ValueError):
            config.archive_stale_fixture_state(proof)
        original = self.support.with_name('retained-support')
        self.support.rename(original)
        self.support.symlink_to(original, target_is_directory=True)
        self.assert_blocked()
        self.assertTrue(self.support.is_symlink())
        self.assertFalse(self.target.exists())

    def test_running_app_and_nonvm_metadata_refuse_without_touching_support(self):
        self.fixture()
        # DedicatedFixtureVMTests exercises platform, account, model and process checks.
        for reason in ('fixture app must be stopped', 'fixture dedicated VM hardware'):
            with mock.patch.object(config, 'dedicated_fixture_vm', side_effect=ValueError(reason)):
                self.assert_blocked()

    def test_postrename_verification_failure_reports_moved_and_never_removes_archive(self):
        self.fixture()
        real_read = config.bounded_read
        def fail_after_move(*args, **kwargs):
            if self.target.exists():
                raise ValueError('never-print-this')
            return real_read(*args, **kwargs)
        with mock.patch.object(config, 'bounded_read', side_effect=fail_after_move):
            proof = self.archive()
        self.assertFalse(proof['archived'])
        self.assertTrue(proof['moved'])
        self.assertEqual(proof['status'], 'preservation-unconfirmed')
        self.assertTrue(self.target.is_dir())
        self.assertFalse(self.support.exists())
        self.assertNotIn('never-print-this', json.dumps(proof))

    def test_missing_bad_or_symlinked_source_provenance_refuses(self):
        self.fixture()
        original = (self.support.stat().st_ino, self.policy.read_bytes())
        with self.assertRaises(ValueError):
            config.repair_source(self.root, '20261024T010101Z-17')
        for field, value in (('status', 'pass'), ('root', '/unowned'), ('dry_run', True),
                             ('args', ['--platform', 'ipad']), ('failures', [])):
            with self.subTest(field=field):
                saved = self.result[field]
                self.result[field] = value
                self.save()
                with self.assertRaises(ValueError):
                    config.repair_source(self.root, config.FAILED_FIXTURE_RUN)
                self.result[field] = saved
        self.save()
        self.seed['invented_operation_binding'] = 'never-accepted'
        self.save()
        with self.assertRaises(ValueError):
            config.repair_source(self.root, config.FAILED_FIXTURE_RUN)
        path = self.run / 'seed.json'
        path.unlink()
        path.symlink_to(self.run / 'result.json')
        with self.assertRaises(OSError):
            config.repair_source(self.root, config.FAILED_FIXTURE_RUN)
        path.unlink()
        with self.assertRaises(ValueError):
            config.repair_source(self.root, config.FAILED_FIXTURE_RUN)
        self.assertEqual((self.support.stat().st_ino, self.policy.read_bytes()), original)


class DedicatedFixtureVMTests(unittest.TestCase):
    def test_host_other_user_hardware_and_running_app_are_refused_without_ui_or_env(self):
        user = mock.Mock(pw_name='meguri', pw_dir='/Users/meguri')
        for platform, username, model, processes in (
                ('linux', 'meguri', b'VirtualMac2,1', b''),
                ('darwin', 'someone-else', b'VirtualMac2,1', b''),
                ('darwin', 'meguri', b'Mac15,9', b''),
                ('darwin', 'meguri', b'VirtualMac2,1', b'/fixture/processes/Meguri.app/Contents/MacOS/Meguri\n')):
            user.pw_name = username
            with mock.patch.object(config.sys, 'platform', platform), mock.patch.object(config.pwd, 'getpwuid', return_value=user), mock.patch.object(config.subprocess, 'run', side_effect=[mock.Mock(stdout=model), mock.Mock(stdout=processes)]):
                with self.assertRaises(ValueError):
                    config.dedicated_fixture_vm()

    def test_shared_vm_identity_keeps_hardware_guard_without_stopped_app_bypass(self):
        user = mock.Mock(pw_name='meguri', pw_dir='/Users/meguri')
        with mock.patch.object(config.sys, 'platform', 'darwin'), mock.patch.object(config.pwd, 'getpwuid', return_value=user), mock.patch.object(config.subprocess, 'run', return_value=mock.Mock(stdout=b'VirtualMac2,1')) as child:
            self.assertEqual(config.dedicated_fixture_identity(), Path('/Users/meguri'))
            child.assert_called_once_with(['/usr/sbin/sysctl', '-n', 'hw.model'], check=True,
                stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, timeout=5)
        with mock.patch.object(config, 'dedicated_fixture_vm', side_effect=ValueError('running app')):
            with self.assertRaises(ValueError): config.stage_media('/unselected', '20261024T010101Z-17')


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


class WatchdogTests(unittest.TestCase):
    def test_observer_schedules_one_hour_and_eof_cancels_watchdog(self):
        server = mock.Mock(server_port=19090, token='synthetic')
        threads = []
        def thread(*, target, daemon):
            child = mock.Mock()
            threads.append((target, child))
            return child
        with mock.patch.object(sys, 'argv', ['manual-transfer-wire.py', '19091']), \
                mock.patch.object(wire, 'RigServer', return_value=server), \
                mock.patch.object(wire.signal, 'signal'), \
                mock.patch.object(wire.threading, 'Thread', side_effect=thread), \
                mock.patch.object(wire.threading, 'Timer') as timer, \
                mock.patch.object(wire.os, 'read', return_value=b'') as read, \
                contextlib.redirect_stdout(io.StringIO()):
            def eof(**_kwargs):
                timer.assert_called_once_with(3600, mock.ANY)
                timer.return_value.start.assert_called_once_with()
                self.assertTrue(timer.return_value.daemon)
                server.stop.assert_not_called()
                threads[0][1].start.assert_called_once_with()
                threads[0][0]()  # controller EOF, before the watchdog fires
                read.assert_called_once_with(0, 1)
                threads[1][1].start.assert_called_once_with()
                threads[1][0]()  # run the scheduled immediate shutdown
                server.stop.assert_called_once_with()
            server.serve_forever.side_effect = eof
            wire.main()
            timer.return_value.cancel.assert_called_once_with()
            server.stop_reader.assert_called_once_with()
            server.server_close.assert_called_once_with()


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
        self.control('mark/a-export-before-consent')
        self.protocol('/msync/begin')  # A checks unchanged media, no file payload.
        self.control('mark/a-exported')
        self.control('arm-export')
        self.protocol('/sync/download')
        self.protocol('/sync/upload')
        self.control('mark/export-before-consent')
        future = self.held_request()
        self.control('abort')
        self.await_aborted(future)
        for name in ('export-cancelled', 'export-passive-history', 'export-resume-decision'):
            self.control('mark/' + name)
        self.control('release')
        self.protocol('/msync/downloadFiles')
        self.protocol('/msync/uploadChanges')
        self.control('mark/exported')
        proof = self.control('proof')
        config.verify_wire(proof)
        # An otherwise complete proof must fail each required M8 wire condition.
        for checkpoint, key in (('before-consent', 'media'), ('passive-history', 'media'),
                                ('resume-decision', 'media'), ('resumed', 'collection_transfers'),
                                ('a-export-before-consent', 'media'), ('a-exported', 'collection_transfers'),
                                ('export-passive-history', 'media'), ('export-resume-decision', 'collection'),
                                ('exported', 'held')):
            broken = json.loads(json.dumps(proof))
            broken['checkpoints'][checkpoint][key] += 1
            with self.assertRaises(ValueError):
                config.verify_wire(broken)
        for name in ('a-export-before-consent', 'a-exported'):
            broken = copy.deepcopy(proof); del broken['checkpoints'][name]
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
        status, data = request(self.server.server_port, '/__rf4/native-state', self.server.token)
        self.assertEqual(status, 503)
        config.validate_failure(json.loads(data))
        for output in ('{}', 'not-json', '{"private":"never-export"}', 'x' * (wire.READER_LIMIT + 1)):
            self.reader('print(' + repr(output) + ')')
            status, data = request(self.server.server_port, '/__rf4/native-state', self.server.token)
            self.assertEqual(status, 503)
            config.validate_failure(json.loads(data))
        # Even a valid reduced response is refused when selected context changed.
        self.reader('print(' + repr(json.dumps(value)) + ')')
        status, data = request(self.server.server_port, '/__rf4/native-state', self.server.token)
        self.assertEqual(status, 503)
        config.validate_failure(json.loads(data))

    def test_observer_freezes_import_and_both_export_identities(self):
        with tempfile.TemporaryDirectory() as temporary:
            support, _, _ = synthetic_cycle(Path(temporary).resolve())
            value = config.native_state(config.native_context(support))
        self.reader('print(' + repr(json.dumps(value)) + ')')
        self.server.reader_context = value['context']
        self.assertEqual(self.control('native-state'), value)
        for label in ('Import', 'AExport', 'Export'):
            for mutation in ('id', 'account', 'missing'):
                with self.subTest(label=label, mutation=mutation):
                    changed = copy.deepcopy(value)
                    if mutation == 'missing':
                        changed['operations'][label] = None
                    elif mutation == 'id':
                        changed['operations'][label]['id'] = '0000000000000004-0000000000000002'
                    else:
                        changed['operations'][label]['account'] = 'f' * 64
                    self.reader('print(' + repr(json.dumps(changed)) + ')')
                    self.server.reader_context = value['context']
                    response = request(self.server.server_port, '/__rf4/native-state', self.server.token)
                    # Missing Import fails schema before the frozen-identity check.
                    self.assert_failure_response(response, 'schema' if label == 'Import' and mutation == 'missing' else 'binding')

    def assert_failure_response(self, response, stage, status=503):
        actual_status, data = response
        self.assertEqual(actual_status, status)
        value = config.validate_failure(json.loads(data))
        self.assertEqual(value['failure']['stage'], stage)
        self.assertEqual(value['failure']['http_status'], status)
        self.assertNotIn(b'private', data)
        return value

    def test_nonzero_child_safe_receipt_persisted_before_teardown(self):
        with tempfile.TemporaryDirectory() as temporary:
            self.server.failure_directory = Path(temporary)
            diagnostic = config.failure('resolver', child_rc=70, script_line=10)
            self.reader('import sys; print(' + repr(json.dumps(diagnostic)) + '); sys.exit(1)')
            value = self.assert_failure_response(request(self.server.server_port, '/__rf4/native-state', self.server.token), 'resolver')
            saved = json.loads((Path(temporary) / 'native-reader-failure.json').read_bytes())
            self.assertEqual(saved, dict(version=1, failures=1, first=value, last=value))
            self.assertEqual(value['failure']['child_rc'], 70)
            bad = config.failure('read'); bad['failure']['stage'] = 'private-nonce'
            self.reader('import sys; print(' + repr(json.dumps(bad)) + '); sys.exit(1)')
            self.assert_failure_response(request(self.server.server_port, '/__rf4/native-state', self.server.token), 'schema')

    def mobile_child(self, state=None, proof=None):
        proof = proof or dict(authority=config.BOOTSTRAP_AUTHORITY, context='b' * 64,
                              marker='c' * 64, namespace='d' * 64, binding='e' * 64)
        selector = 'a' * 64
        code = ("import json,sys; request=json.loads(sys.stdin.readline()); "
                "assert set(request)=={'action','selector','bound'}; "
                "assert request['selector']==" + repr(selector) + "; "
                "proof=" + repr(proof) + "; "
                "value={'selector':request['selector'],'bootstrap':proof}; "
                "value.update({'state':json.loads(" + repr(json.dumps(state)) + ")}) if request['action']=='read' else None; "
                "print(json.dumps(value))")
        self.reader(code)
        self.server.reader_mobile = True
        return proof

    def test_mobile_observer_retains_selector_actual_digest_and_poisoned_failure(self):
        with tempfile.TemporaryDirectory() as temporary:
            support, _, _ = synthetic_native(Path(temporary).resolve())
            state = config.native_state(config.native_context(support))
        proof = self.mobile_child(state, dict(authority=config.BOOTSTRAP_AUTHORITY,
            context=state['context'], marker='c' * 64, namespace='d' * 64, binding='e' * 64))
        self.assertEqual(self.control('bootstrap'), dict(selector='a' * 64, bootstrap=proof))
        self.assertEqual(self.server.reader_bound, proof)
        self.assertTrue(self.server.bootstrap_complete)
        expected = dict(selector='a' * 64, bootstrap=proof, state=state)
        self.assertEqual(self.control('native-state'), expected)
        self.assertEqual(self.control('native-state'), expected)
        changed = dict(state, context='b' * 64)
        self.reader('print(' + repr(json.dumps(dict(selector='a' * 64, bootstrap=proof, state=changed))) + ')')
        self.assert_failure_response(request(self.server.server_port, '/__rf4/native-state', self.server.token), 'schema')
        with mock.patch.object(wire.subprocess, 'Popen') as launch:
            self.assert_failure_response(request(self.server.server_port, '/__rf4/native-state', self.server.token), 'binding')
            self.assert_failure_response(request(self.server.server_port, '/__rf4/bootstrap', self.server.token), 'binding')
            launch.assert_not_called()

    def test_mobile_bootstrap_auth_body_unknown_action_and_replay(self):
        proof = self.mobile_child()
        with mock.patch.object(wire.subprocess, 'Popen') as launch:
            self.assertEqual(request(self.server.server_port, '/__rf4/bootstrap', 'invalid')[0], 403)
            self.assertEqual(request(self.server.server_port, '/__rf4/bootstrap', self.server.token, body=b'{}')[0], 400)
            self.assertEqual(request(self.server.server_port, '/__rf4/bootstrap/path', self.server.token)[0], 409)
            launch.assert_not_called()
        self.assertFalse(self.server.bootstrap_attempted)
        self.assertEqual(self.control('bootstrap')['bootstrap'], proof)
        with mock.patch.object(wire.subprocess, 'Popen') as launch:
            self.assert_failure_response(request(self.server.server_port, '/__rf4/bootstrap', self.server.token), 'binding')
            launch.assert_not_called()

    def test_mobile_protocol_before_bootstrap_poisoned_and_never_forwarded(self):
        self.mobile_child()
        self.assert_failure_response(request(self.server.server_port, '/sync/hostKey'), 'binding', 409)
        self.assertEqual(self.upstream.observed, 0)
        with mock.patch.object(wire.subprocess, 'Popen') as launch:
            self.assert_failure_response(request(self.server.server_port, '/__rf4/bootstrap', self.server.token), 'binding')
            launch.assert_not_called()

    def test_mobile_read_before_bootstrap_poisoned_and_cannot_fallback(self):
        self.mobile_child()
        with mock.patch.object(wire.subprocess, 'Popen') as launch:
            self.assert_failure_response(request(self.server.server_port, '/__rf4/native-state', self.server.token), 'binding')
            self.assert_failure_response(request(self.server.server_port, '/__rf4/bootstrap', self.server.token), 'binding')
            launch.assert_not_called()

    def test_mobile_held_or_active_gate_forbids_bootstrap(self):
        self.mobile_child()
        self.server.gate = 'holding'; self.server.counts['held'] = 1; self.server.counts['active'] = 1
        with mock.patch.object(wire.subprocess, 'Popen') as launch:
            self.assert_failure_response(request(self.server.server_port, '/__rf4/bootstrap', self.server.token), 'binding')
            launch.assert_not_called()

    def test_mobile_failed_bootstrap_is_once_and_keeps_typed_metadata(self):
        self.mobile_child()
        diagnostic = config.failure('media', child_rc=1, script_line=525)
        self.reader('import sys; print(' + repr(json.dumps(diagnostic)) + '); sys.exit(1)')
        value = self.assert_failure_response(request(self.server.server_port, '/__rf4/bootstrap', self.server.token), 'media')
        self.assertEqual(value['failure']['child_rc'], 1)
        self.assertEqual(value['failure']['script_line'], 525)
        self.assertFalse(self.server.bootstrap_complete)
        with mock.patch.object(wire.subprocess, 'Popen') as launch:
            self.assert_failure_response(request(self.server.server_port, '/__rf4/bootstrap', self.server.token), 'binding')
            self.assert_failure_response(request(self.server.server_port, '/__rf4/native-state', self.server.token), 'binding')
            launch.assert_not_called()

    def test_mobile_concurrent_bootstrap_refuses_and_drains_only_child(self):
        self.mobile_child()
        self.reader('import time; time.sleep(60)')
        future = self.pool.submit(request, self.server.server_port, '/__rf4/bootstrap', self.server.token)
        child = self.await_reader()
        self.assert_failure_response(request(self.server.server_port, '/__rf4/bootstrap', self.server.token), 'busy', 409)
        self.assert_failure_response(future.result(timeout=2), 'binding')
        self.assertIsNotNone(child.returncode)
        self.assertTrue(child.stdout.closed)
        self.assertTrue(child.stdin.closed)
        self.assertFalse(self.server.bootstrap_complete)

    def test_mac_bootstrap_cannot_launch_staging_child(self):
        self.reader('raise SystemExit("must not launch")')
        with mock.patch.object(wire.subprocess, 'Popen') as launch:
            self.assert_failure_response(request(self.server.server_port, '/__rf4/bootstrap', self.server.token), 'binding')
            launch.assert_not_called()

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
            self.assert_failure_response(future.result(timeout=3), 'timeout')
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
        self.assert_failure_response(request(self.server.server_port, '/__rf4/native-state', self.server.token), 'busy', 409)
        # Protocol snapshots remain responsive: subprocess work holds no condition.
        self.control('snapshot')
        self.server.stop_reader()
        self.assert_failure_response(future.result(timeout=2), 'stopped')
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
                        self.assert_failure_response(future.result(timeout=2), 'stopped')
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
