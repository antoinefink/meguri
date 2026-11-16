"""The API control client must work without allocating a bridge endpoint."""
import copy
import json
import pathlib
import tempfile
import unittest

from test_edge import helper, OWNER, ROOT


SERVICE, NETWORK, ENDPOINT = '1' * 64, '2' * 64, '3' * 64
NET = 'test-project_oracle-internal'


class ControlEdges(helper.Edges):
    """Model only Docker metadata and control execution, not API transport."""
    def __init__(self, available=0):
        super().__init__(ROOT, 'test-project', 'test-run', 'test-image', 'sync-faults:8474', OWNER)
        self.temporary = tempfile.TemporaryDirectory()
        self.state = pathlib.Path(self.temporary.name)
        self.available = available
        self.endpoints = 61
        self.peak_endpoints = self.endpoints
        self.calls = []
        self.current = [SERVICE]
        self.service = {
            'Id': SERVICE, 'Labels': {
                'com.docker.compose.project': self.project,
                'com.docker.compose.service': 'sync-faults',
                'com.docker.compose.oneoff': 'False',
                'com.docker.compose.project.config_files': self.root + '/docker/compose.yml',
                'com.docker.compose.project.working_dir': self.root + '/docker'},
            'Running': True, 'NetworkMode': NET, 'PortBindings': {}, 'Ports': {'8474/tcp': None},
            'Networks': {NET: {'NetworkID': NETWORK, 'EndpointID': ENDPOINT, 'IPAddress': '172.20.0.2'}}}
        self.network = {
            'Id': NETWORK, 'Name': NET, 'Internal': True,
            'Labels': {'com.docker.compose.project': self.project, 'com.docker.compose.network': 'oracle-internal'},
            'Containers': {SERVICE: {'EndpointID': ENDPOINT, 'IPv4Address': '172.20.0.2/26'}}}
        self.proxies = {'meguri-edge-' + OWNER + '-' + '4' * 32: {'name': 'neutral'}}
        self.after_run = lambda: None

    def docker(self, *args, missing=False):
        self.calls.append(args)
        if args[0] == 'ps':
            return '\n'.join(self.current)
        if args[0] == 'inspect':
            # Production selects metadata without requesting Config.Env.
            if '--format' not in args:
                raise AssertionError('unfiltered service inspection')
            return json.dumps(self.service)
        if args[:2] == ('network', 'inspect'):
            selected = {k: v for k, v in self.network.items() if k != 'Containers'}
            selected['Binding'] = self.network['Containers'].get(SERVICE)
            return json.dumps(selected)
        if args[0] != 'run':
            raise AssertionError('unexpected Docker mutation')
        mode = args[args.index('--network') + 1]
        if mode != 'container:' + SERVICE:
            if self.available == 0:
                raise helper.EdgeError('synthetic exhausted address pool')
            self.peak_endpoints = max(self.peak_endpoints, self.endpoints + 1)
        method, path = args[-2:]
        if method == 'DELETE':
            name = path.removeprefix('/proxies/')
            status = 204 if self.proxies.pop(name, None) is not None else 404
            body = ''
        else:
            status, body = 200, json.dumps(self.proxies)
        self.after_run()
        return json.dumps({'status': status, 'body': body})


class ControlTransport(unittest.TestCase):
    def edge(self, available=0):
        edge = ControlEdges(available)
        self.addCleanup(edge.temporary.cleanup)
        return edge

    def refused(self, edge):
        before = copy.deepcopy(edge.proxies)
        with self.assertRaises(helper.EdgeError):
            edge.api('DELETE', next(iter(edge.proxies)))
        self.assertEqual(edge.proxies, before)
        self.assertFalse(any(c[0] == 'run' for c in edge.calls))

    def test_same_api_semantics_with_free_or_exhausted_pool_and_no_new_endpoint(self):
        for available in (3, 0):
            with self.subTest(available=available):
                edge = self.edge(available)
                original = copy.deepcopy(edge.proxies)
                self.assertEqual(edge.api('GET'), original)
                proxy = next(iter(original))
                self.assertIsNone(edge.api('DELETE', proxy))
                self.assertIsNone(edge.api('DELETE', proxy))  # Existing 404 semantics.
                self.assertEqual(edge.api('GET'), {})
                self.assertEqual(edge.peak_endpoints, edge.endpoints)
                controls = [c for c in edge.calls if c[0] == 'run']
                self.assertEqual(len(controls), 4)
                for call in controls:
                    self.assertEqual(call[call.index('--network') + 1], 'container:' + SERVICE)
                    self.assertEqual(call[-3], '127.0.0.1:8474')
                    self.assertIn('--rm', call)
                    self.assertIn('--read-only', call)
                    self.assertIn('no-new-privileges', call)
                    self.assertIn('ALL', call)
                    self.assertNotIn('--publish', call)
                    self.assertIn(edge.image, call)

    def test_absent_ambiguous_or_nonimmutable_service_refuses_before_control(self):
        for ids in ([], [SERVICE, '5' * 64], [SERVICE[:12]], [SERVICE, SERVICE]):
            with self.subTest(ids=ids):
                edge = self.edge(3)
                edge.current = ids
                self.refused(edge)

    def test_wrong_checkout_role_or_stopped_service_refuses_before_control(self):
        for label in self.edge().service['Labels']:
            with self.subTest(label=label):
                edge = self.edge(3)
                edge.service['Labels'][label] = 'foreign'
                self.refused(edge)
        for field, value in (('Id', '5' * 64), ('Running', False), ('NetworkMode', 'host'),
                             ('PortBindings', {'8474/tcp': [{'HostIp': '127.0.0.1', 'HostPort': '8474'}]}),
                             ('Ports', {'8474/tcp': [{'HostIp': '0.0.0.0', 'HostPort': '8474'}]})):
            with self.subTest(field=field):
                edge = self.edge(3)
                edge.service[field] = value
                self.refused(edge)

    def test_unexpected_network_or_unproved_isolation_refuses_before_control(self):
        for failure in ('extra', 'absent', 'internal', 'project', 'role', 'id', 'name', 'binding', 'ip', 'endpoint'):
            with self.subTest(failure=failure):
                edge = self.edge(3)
                if failure == 'extra':
                    edge.service['Networks']['other'] = {}
                elif failure == 'absent':
                    edge.service['Networks'] = {}
                elif failure == 'internal':
                    edge.network['Internal'] = False
                elif failure in ('project', 'role'):
                    key = 'project' if failure == 'project' else 'network'
                    edge.network['Labels']['com.docker.compose.' + key] = 'foreign'
                elif failure in ('id', 'name'):
                    edge.network['Id' if failure == 'id' else 'Name'] = 'foreign'
                elif failure == 'binding':
                    edge.network['Containers'] = {}
                elif failure == 'ip':
                    edge.network['Containers'][SERVICE]['IPv4Address'] = '172.20.0.3/26'
                else:
                    edge.network['Containers'][SERVICE]['EndpointID'] = '6' * 64
                self.refused(edge)

    def test_fault_configuration_cannot_select_an_arbitrary_loopback_authority(self):
        for faults in ('127.0.0.1:8474', 'sync-faults:80', 'other:8474', 'sync-faults:08474',
                       'sync-faults:8474/path', 'sync-faults:8474\n'):
            with self.subTest(faults=faults):
                edge = self.edge(3)
                edge.faults = faults
                self.refused(edge)
                self.assertEqual(edge.calls, [])

    def test_identity_stays_pinned_across_get_delete_and_rechecks(self):
        for replacement in ('replacement', 'ambiguous', 'stopped', 'rebound'):
            with self.subTest(replacement=replacement):
                edge = self.edge(3)
                edge.api('GET')
                if replacement == 'replacement':
                    edge.current = ['5' * 64]
                    edge.service['Id'] = edge.current[0]
                elif replacement == 'ambiguous':
                    edge.current.append('5' * 64)
                elif replacement == 'stopped':
                    edge.service['Running'] = False
                else:
                    edge.service['Networks'][NET]['EndpointID'] = '6' * 64
                    edge.network['Containers'][SERVICE]['EndpointID'] = '6' * 64
                edge.calls.clear()
                self.refused(edge)

    def test_change_during_control_is_not_accepted_as_a_successful_recheck(self):
        edge = self.edge(3)
        edge.after_run = lambda: edge.current.append('5' * 64)
        with self.assertRaises(helper.EdgeError):
            edge.api('GET')
        self.assertEqual(len([c for c in edge.calls if c[0] == 'run']), 1)


if __name__ == '__main__':
    unittest.main()
