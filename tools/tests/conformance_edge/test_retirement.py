"""Scoped normal retirement must prove absence, never clean up live routes."""
import concurrent.futures
import json
import threading
import time
import unittest
from unittest import mock

# Reuse the ownership model used by the existing edge lifecycle tests.
from test_edge import FakeEdges, helper, OWNER, A, B


class Retirement(unittest.TestCase):
    def edge(self):
        edge = FakeEdges()
        self.addCleanup(edge.temporary.cleanup)
        for token in (A, B):
            state = edge.resources.pop(('container', token))
            state['Id'] = token * 2
            state['Name'] = '/meguri-conf-worker-' + OWNER + '-' + token
            edge.resources[('container', token * 2)] = state
        edge.proxies.clear()
        return edge

    def unchanged(self, edge, action):
        resources = dict(edge.resources)
        proxies = dict(edge.proxies)
        with self.assertRaises(helper.EdgeError):
            action()
        self.assertEqual(edge.resources, resources)
        self.assertEqual(edge.proxies, proxies)
        self.assertFalse(any(c[0] == 'rm' or c[:2] == ('api', 'DELETE') for c in edge.calls))

    def publish(self, edge, worker=A):
        index = 0 if worker == A else 1
        route = edge.route(worker)
        edge.proxies[route['proxy']] = {'name': route['proxy'], 'listen': f'0.0.0.0:{30001+index}',
                                        'upstream': f'172.20.0.{10+index}:27701'}
        return edge.allocate(worker)['lease']

    def test_only_candidate_retires_while_peer_route_remains_active(self):
        edge = self.edge()
        lease = self.publish(edge, B)
        before = dict(edge.proxies)
        edge.retire_worker(A, A * 2)
        self.assertNotIn(('container', A * 2), edge.resources)
        self.assertIn(('container', B * 2), edge.resources)
        self.assertTrue(edge.edge_names())
        self.assertEqual(edge.proxies, before)
        self.assertFalse((edge.state / 'closing').exists())
        self.assertTrue((edge.state / ('retired-' + A)).exists())
        with self.assertRaisesRegex(helper.EdgeError, 'retired'):
            edge.allocate(A)
        edge.remove(lease)  # Peer release and terminal cleanup still work.
        edge.sweep()
        self.assertEqual(edge.resources, {})

    def test_reservations_and_pre_token_publications_are_retained(self):
        for remaining in ('record', 'container', 'network'):
            with self.subTest(remaining=remaining):
                edge = self.edge()
                self.publish(edge)
                edge.proxies.clear()
                if remaining == 'record':
                    edge.resources = {k:v for k,v in edge.resources.items() if k[1] in (A * 2, B * 2)}
                else:
                    for path in edge.state.glob('*.json'):
                        path.unlink()
                    edge.resources = {k:v for k,v in edge.resources.items()
                                      if k[1] in (A * 2, B * 2) or k[0] == remaining}
                edge.calls.clear()
                self.unchanged(edge, lambda: edge.retire_worker(A, A * 2))

    def test_proxy_targets_aliases_and_ambiguous_backends_are_retained(self):
        for host in ('172.20.0.10', A * 2, A[:12], 'meguri-conf-worker-' + OWNER + '-' + A,
                     'shared-alias', 'unknown-backend'):
            with self.subTest(host=host):
                edge = self.edge()
                edge.resources[('container', A * 2)]['NetworkSettings']['Networks'][
                    'test-project_oracle-internal']['Aliases'] = ['shared-alias']
                # Even a foreign proxy can target this namespace. Preserve it.
                edge.proxies['foreign-proxy'] = {'upstream': host + ':27701'}
                self.unchanged(edge, lambda: edge.retire_worker(A, A * 2))

    def test_uncertain_peer_publication_blocks_retirement_and_foreign_resources_survive(self):
        edge = self.edge()
        self.publish(edge, B)
        # An owner with a malformed publication is uncertain, even when its
        # worker label claims a peer. No edge may be deleted to make progress.
        for (kind, _), state in edge.resources.items():
            if kind == 'container' and state['Config']['Labels']['meguri.mg.purpose'] == helper.PURPOSE:
                state['Name'] = '/unexpected-publication'
        edge.calls.clear()
        self.unchanged(edge, lambda: edge.retire_worker(A, A * 2))

        edge = self.edge()
        foreign = FakeEdges(run='foreign-run', owner='e' * 32)
        self.addCleanup(foreign.temporary.cleanup)
        for index, state in enumerate(foreign.resources.values()):
            state['NetworkSettings']['Networks']['test-project_oracle-internal']['IPAddress'] = f'172.20.0.{50+index}'
        for index, proxy in enumerate(foreign.proxies.values()):
            proxy['upstream'] = f'172.20.0.{50+index}:27701'
        edge.resources.update(foreign.resources)
        edge.proxies.update(foreign.proxies)
        edge.retire_worker(A, A * 2)
        for key, state in foreign.resources.items():
            self.assertEqual(edge.resources[key], state)
        self.assertEqual(edge.proxies, foreign.proxies)

    def test_exact_identity_and_reinspection_precede_deletion(self):
        for field in ('checkout', 'project', 'run-id', 'purpose', 'edge-owner', 'worker', 'Name', 'Id', 'Running'):
            with self.subTest(field=field):
                edge = self.edge()
                state = edge.resources[('container', A * 2)]
                if field in ('Name', 'Id'):
                    state[field] = 'foreign'
                elif field == 'Running':
                    state['State']['Running'] = False
                else:
                    state['Config']['Labels']['meguri.mg.' + field] = 'foreign'
                self.unchanged(edge, lambda: edge.retire_worker(A, A * 2))
        edge = self.edge()
        original = edge.inspect
        count = 0
        def replaced(cid):
            nonlocal count
            state = original(cid)
            if cid == A * 2:
                count += 1
                if count == 2:
                    state = dict(state, Name='/replacement')
            return state
        edge.inspect = replaced
        self.unchanged(edge, lambda: edge.retire_worker(A, A * 2))

    def test_pending_uncertain_and_terminal_owners_never_retire(self):
        for marker in ('pending-abandoned', 'uncertain', 'closing'):
            edge = self.edge()
            edge.admission().mark(marker)
            self.unchanged(edge, lambda: edge.retire_worker(A, A * 2))
            self.assertEqual(edge.calls, [])
            self.assertTrue((edge.state / marker).exists())

    def test_failed_or_unconfirmed_removal_preserves_worker_and_fence(self):
        for sticky in (False, True):
            edge = self.edge()
            edge.fail_remove, edge.sticky_remove = not sticky, sticky
            with self.assertRaises(helper.EdgeError):
                edge.retire_worker(A, A * 2)
            self.assertIn(('container', A * 2), edge.resources)
            self.assertTrue((edge.state / ('retired-' + A)).exists())
            self.assertFalse((edge.state / 'closing').exists())
            self.assertFalse(any(c[:2] == ('api', 'DELETE') for c in edge.calls))
            with self.assertRaises(helper.EdgeError):
                edge.admission().require_worker_open(A)

    def test_retirement_drains_inflight_shared_publisher_without_closing_owner(self):
        edge = self.edge()
        entered, resume, draining = threading.Event(), threading.Event(), threading.Event()
        locked = helper.admission.locked
        def observed(stream, exclusive, deadline):
            if exclusive:
                draining.set()
            return locked(stream, exclusive, deadline)
        def publish():
            with edge.admission().hold(time.monotonic() + 3, publishing=True):
                entered.set()
                if not resume.wait(3):
                    raise AssertionError('publisher barrier timed out')
                edge.save(edge.scope + '.' + '1' * 32, {'route': edge.route(),
                    'target': 'http://sync-faults:30001/', 'expected_worker': A, 'released': False})
        with mock.patch.object(helper.admission, 'locked', observed), \
             concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
            published = pool.submit(publish)
            self.assertTrue(entered.wait(3))
            retired = pool.submit(edge.retire_worker, A, A * 2)
            try:
                self.assertTrue(draining.wait(3))
                self.assertFalse(retired.done())
                self.assertFalse((edge.state / 'closing').exists())
            finally:
                resume.set()
            published.result(timeout=3)
            with self.assertRaisesRegex(helper.EdgeError, 'reserved'):
                retired.result(timeout=3)
        self.assertIn(('container', A * 2), edge.resources)
        edge.admission().require_worker_open(B)

    def test_delayed_acquire_is_refused_after_retirement_releases_exclusive_admission(self):
        edge = self.edge()
        inspected, resume, acquiring = threading.Event(), threading.Event(), threading.Event()
        api = edge.api
        def paused(method, proxy=None):
            inspected.set()
            if not resume.wait(3):
                raise AssertionError('retirement barrier timed out')
            return api(method, proxy)
        edge.api = paused
        def acquire():
            acquiring.set()
            return edge.allocate()
        with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
            retired = pool.submit(edge.retire_worker, A, A * 2)
            self.assertTrue(inspected.wait(3))
            late = pool.submit(acquire)
            try:
                self.assertTrue(acquiring.wait(3))
                self.assertFalse(late.done())
            finally:
                resume.set()
            retired.result(timeout=3)
            with self.assertRaisesRegex(helper.EdgeError, 'retired'):
                late.result(timeout=3)
        self.assertFalse(edge.edge_names())
        edge.admission().require_worker_open(B)
