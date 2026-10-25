"""Exercise resource ownership and socket forwarding with synthetic peers."""
import asyncio
import concurrent.futures
import importlib.util
import json
import pathlib
import signal
import socket
import struct
import tempfile
import threading
import time
import unittest
from unittest import mock

ROOT = pathlib.Path(__file__).resolve().parents[3]


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


helper = load("edge_helper", ROOT / "tools/mg.d/conformance-edge.py")
forward = load("edge_forward", ROOT / "tools/mg.d/conformance-forward.py")
admission = load("edge_admission", ROOT / "tools/mg.d/conformance-admission.py")
OWNER, A, B = "a" * 32, "c" * 32, "d" * 32


class FakeEdges(helper.Edges):
    """Namespaced Docker/API model. Runtime proof is the separate MG harness."""
    def __init__(self, resources=None, run="test-run", owner=OWNER, proxies=None):
        super().__init__(ROOT, "test-project", run, "test-image", "sync-faults:8474", owner)
        self.temporary = tempfile.TemporaryDirectory()
        self.state = pathlib.Path(self.temporary.name)
        self.resources = resources if resources is not None else {}
        self.proxies = proxies if proxies is not None else {}
        self.calls, self.mapping = [], None
        self.model_lock = threading.RLock()
        self.fail_remove = self.fail_delete = self.fail_start = False
        self.sticky_remove = False
        if resources is None:
            for index, token in enumerate((A, B) if owner == OWNER else ("1" * 32, "2" * 32)):
                labels = dict(self.labels, **{"meguri.mg.purpose": helper.WORKER_PURPOSE,
                                             helper.WORKER: token})
                self.resources[("container", token)] = self.worker_state(token, index, labels)
                self.proxies[self.route(token)["proxy"]] = {
                    "name": self.route(token)["proxy"], "listen": "0.0.0.0:" + str(30001 + index),
                    "upstream": "172.20.0." + str(10 + index) + ":27701"}

    def worker_state(self, token, index, labels):
        return {"Id": token, "Name": "/worker-" + token, "State": {"Running": True},
                "HostConfig": {"AutoRemove": False},
                "Config": {"Labels": labels, "Hostname": token[:12]},
                "NetworkSettings": {"Ports": {}, "Networks": {
                    "test-project_oracle-internal": {"IPAddress": "172.20.0." + str(10 + index)}}}}

    def route(self, worker=A):
        return {"proxy": "meguri-edge-" + self.owner + "-" + worker, "worker": worker}

    def allocate(self, worker=A):
        return self.acquire("http://sync-faults:" + ("30001" if worker == A else "30002") + "/",
                            self.route(worker), worker)

    def api(self, method, proxy=None):
        with self.model_lock:
            return self._api(method, proxy)

    def _api(self, method, proxy=None):
        self.calls.append(("api", method, proxy))
        if method == "DELETE":
            if self.fail_delete:
                raise helper.EdgeError("synthetic proxy delete failure")
            self.proxies.pop(proxy, None)
            return None
        return dict(self.proxies)

    def docker(self, *args, missing=False):
        with self.model_lock:
            return self._docker(*args, missing=missing)

    def _docker(self, *args, missing=False):
        self.calls.append(args)
        words = list(args)
        if words[:2] == ["network", "create"] or words[0] == "create":
            kind = "network" if words[0] == "network" else "container"
            labels = dict(words[i+1].split("=", 1) for i, word in enumerate(words) if word == "--label")
            name = words[-1] if kind == "network" else words[words.index("--name")+1]
            selected = 42000 + len(self.resources)
            mapping = self.mapping if self.mapping is not None else {
                "8080/tcp": [{"HostIp": "127.0.0.1", "HostPort": str(selected)}]}
            self.resources[(kind, name)] = {"Id": name, "Name": name, "Labels": labels, "Config": {"Labels": labels},
                "State": {"Running": True}, "NetworkSettings": {"Ports": mapping}}
        elif words[0] == "ps" or words[:2] == ["network", "ls"]:
            kind = "container" if words[0] == "ps" else "network"
            filters = dict(words[i+1][6:].split("=", 1) for i, word in enumerate(words) if word == "--filter")
            return "\n".join(name for (k, name), state in self.resources.items()
                             if k == kind and all(state["Config"]["Labels"].get(key) == value
                                                  for key, value in filters.items()))
        elif words[0] == "rm" or words[:2] == ["network", "rm"]:
            if self.fail_remove:
                raise helper.EdgeError("synthetic edge removal failure")
            kind = "container" if words[0] == "rm" else "network"
            if not self.sticky_remove:
                for name in words[2:]:
                    self.resources.pop((kind, name), None)
        elif words[0] == "inspect":
            return json.dumps([self.resources[("container", words[1])]])
        elif words[:2] == ["network", "inspect"]:
            return json.dumps([self.resources[("network", words[2])]])
        elif words[0] == "start" and self.fail_start:
            raise helper.EdgeError("lost start reply")
        return ""

    def edge_names(self):
        return [name for (kind, name), state in self.resources.items() if kind == "container"
                and state["Config"]["Labels"].get("meguri.mg.purpose") == helper.PURPOSE]


class Ownership(unittest.TestCase):
    def test_target_rejected_before_any_allocation(self):
        for target in ["http://elsewhere:30001/", "http://sync-faults:8474/", "https://sync-faults:30001/",
                       "http://user:secret@sync-faults:30001/", "http://sync-faults/",
                       "http://sync-faults:0/", "http://sync-faults:65536/", "http://sync-faults:30001/x",
                       "http://sync-faults:30001/?x=1", "http://sync-faults:30001/#x",
                       "http://SYNC-faults:30001/", "http://127.0.0.1:30001/", "http://sync-faults:30001/\n"]:
            edge = FakeEdges()
            with self.subTest(target=target), self.assertRaises(helper.EdgeError):
                edge.acquire(target, edge.route(), A)
            self.assertEqual(edge.calls, [])

    def test_exact_request_and_missing_or_substituted_identity(self):
        edge = FakeEdges()
        for expected in [None, "", B, "C" * 32]:
            with self.assertRaises(helper.EdgeError):
                edge.acquire("http://sync-faults:30001/", edge.route(), expected)
        # Complete B tuple, including the same numeric backend port, is refused
        # BEFORE any Docker or proxy operation using A's trusted checkout.
        edge.calls.clear()
        with self.assertRaises(helper.EdgeError):
            edge.acquire("http://sync-faults:30002/", edge.route(B), A)
        self.assertEqual(edge.calls, [])
        self.assertEqual(edge.proxies[edge.route(B)["proxy"]]["upstream"], "172.20.0.11:27701")
        self.assertIn("endpoint", edge.allocate(B))
        for request in [{"cmd": "acquire", "target": "http://sync-faults:30001/"},
                        {"cmd": "release", "lease": ""}, {"cmd": "release", "lease": 1}, [],
                        {"cmd": "release", "lease": "foreign", "extra": 1}]:
            with self.assertRaises(helper.EdgeError):
                edge.request(request)
        with self.assertRaises(helper.EdgeError):
            json.loads('{"cmd":"acquire","cmd":"release"}', object_pairs_hook=helper.strict_object)

    def test_exact_worker_labels_listen_and_namespace(self):
        for failure in ("worker", "owner", "checkout", "project", "run-id", "purpose", "upstream", "listen"):
            edge = FakeEdges()
            if failure == "upstream":
                edge.proxies[edge.route()["proxy"]]["upstream"] = "172.20.0.11:27701"
            elif failure == "listen":
                edge.proxies[edge.route()["proxy"]]["listen"] = "0.0.0.0:30002"
            else:
                key = helper.OWNER if failure == "owner" else "meguri.mg." + failure
                edge.resources[("container", A)]["Config"]["Labels"][key] = "foreign"
            with self.subTest(failure=failure), self.assertRaises(helper.EdgeError):
                edge.allocate()
            self.assertEqual(edge.edge_names(), [])
            self.assertFalse(any(c[:2] == ("api", "DELETE") for c in edge.calls))

    def test_owned_idempotence_and_foreign_refusal(self):
        edge = FakeEdges()
        response = edge.allocate()
        foreign = FakeEdges(edge.resources, "other-run", "e" * 32, edge.proxies)
        with self.assertRaises(helper.EdgeError):
            foreign.remove(response["lease"])
        edge.remove(response["lease"])
        edge.remove(response["lease"])
        self.assertEqual(edge.edge_names(), [])
        self.assertNotIn(edge.route()["proxy"], edge.proxies)
        self.assertIn(edge.route(B)["proxy"], edge.proxies)
        # Confirmed release permits normal same-worker route acquisition.
        edge.proxies[edge.route()["proxy"]] = {"name": edge.route()["proxy"],
            "listen": "0.0.0.0:30001", "upstream": "172.20.0.10:27701"}
        self.assertIn("lease", edge.allocate())

    def test_remove_failure_keeps_proxy_namespace_and_blocks_restart(self):
        for sticky in (False, True):
            edge = FakeEdges()
            lease = edge.allocate()["lease"]
            edge.fail_remove, edge.sticky_remove = not sticky, sticky
            with self.assertRaises(helper.EdgeError):
                edge.remove(lease)
            self.assertTrue(edge.edge_names())
            self.assertIn(edge.route()["proxy"], edge.proxies)
            self.assertIn(("container", A), edge.resources)
            self.assertFalse(admission.admitted(edge.root, edge.project, edge.docker))
            # Other namespace progresses independently, without a global lock.
            self.assertIn("endpoint", edge.allocate(B))

    def test_delete_failure_is_reported_with_inert_reservation(self):
        edge = FakeEdges()
        lease = edge.allocate()["lease"]
        edge.fail_delete = True
        with self.assertRaises(helper.EdgeError):
            edge.remove(lease)
        self.assertFalse(edge.edge_names())
        self.assertIn(edge.route()["proxy"], edge.proxies)
        self.assertIn(("container", A), edge.resources)
        self.assertFalse(json.loads((edge.state / (lease + ".json")).read_text())["released"])
        edge.fail_delete = False
        edge.remove(lease)

    def test_pre_token_sweep_orders_edges_proxy_worker_and_preserves_foreign(self):
        edge = FakeEdges()
        edge.allocate()  # response may be lost
        foreign = FakeEdges(run="foreign-run", owner="e" * 32)
        edge.resources.update(foreign.resources)
        edge.proxies.update(foreign.proxies)
        saved = dict(foreign.resources)
        edge.calls.clear()
        edge.sweep()
        self.assertEqual(edge.resources, saved)
        deletes = [i for i, c in enumerate(edge.calls) if c[:2] == ("api", "DELETE")]
        removals = [(i, c) for i, c in enumerate(edge.calls) if c[0] == "rm"]
        self.assertLess(removals[0][0], min(deletes))
        self.assertGreater(removals[-1][0], max(deletes))
        self.assertEqual(set(edge.proxies), set(foreign.proxies))
        edge.sweep()  # exact current ownership is idempotent

    def test_pre_token_remove_failure_never_deletes_proxy_or_worker(self):
        edge = FakeEdges()
        edge.allocate()
        saved = dict(edge.proxies)
        edge.fail_remove = True
        with self.assertRaises(helper.EdgeError):
            edge.sweep()
        self.assertEqual(edge.proxies, saved)
        self.assertIn(("container", A), edge.resources)

    def test_interrupted_acquire_cleans_before_token_or_reserves_on_failure(self):
        for blocked in (False, True):
            edge = FakeEdges()
            edge.fail_start = True
            edge.fail_remove = blocked
            with self.assertRaises(helper.EdgeError):
                edge.allocate()
            self.assertEqual(bool(edge.edge_names()), blocked)
            self.assertEqual(edge.route()["proxy"] in edge.proxies, blocked)
            self.assertIn(("container", A), edge.resources)

    def test_immutable_routes_readiness_and_invalid_mapping(self):
        edge = FakeEdges()
        with mock.patch.object(socket, "create_connection", side_effect=AssertionError("probe")):
            a, b = edge.allocate(), edge.allocate(B)
        self.assertNotEqual(a["endpoint"], b["endpoint"])
        self.assertNotEqual(a["lease"], b["lease"])
        with self.assertRaises(helper.EdgeError):
            edge.allocate()
        creates = [c for c in edge.calls if c[0] == "create"]
        self.assertEqual([c[-2:] for c in creates], [("sync-faults", "30001"), ("sync-faults", "30002")])
        for call in creates:
            self.assertIn("127.0.0.1::8080", call)
            self.assertNotIn("/var/run/docker.sock", " ".join(call))
        probes = [c for c in edge.calls if c[0] == "exec"]
        self.assertEqual(len(probes), 2)
        self.assertTrue(all("connect" not in c[-1] and "/proc/net/tcp" in c[-1] for c in probes))
        for mapping in [{}, {"8080/tcp": [{"HostIp": "0.0.0.0", "HostPort": "42001"}]},
                        {"8080/tcp": [{"HostIp": "127.0.0.1", "HostPort": "0"}]}]:
            edge = FakeEdges()
            edge.mapping = mapping
            with self.assertRaises(helper.EdgeError):
                edge.allocate()
            self.assertEqual(edge.edge_names(), [])

    def test_docker_child_is_cancelled_and_reaped(self):
        edge = FakeEdges()
        child = mock.Mock(pid=1234)
        child.communicate.side_effect = helper.EdgeError("cancelled")
        with mock.patch.object(helper.subprocess, "Popen", return_value=child), \
             mock.patch.object(helper.os, "killpg") as kill:
            with self.assertRaises(helper.EdgeError):
                helper.Edges.docker(edge, "inspect", "own-edge")
        kill.assert_called_once_with(1234, signal.SIGKILL)
        child.wait.assert_called_once_with()


class OwnerFence(unittest.TestCase):
    def peer(self, edge):
        peer = FakeEdges(edge.resources, proxies=edge.proxies)
        peer.state = edge.state
        peer.model_lock = edge.model_lock
        return peer

    def test_closed_admission_is_persistent_and_runs_no_docker(self):
        edge = FakeEdges()
        edge.sweep()
        late = self.peer(edge)
        with self.assertRaisesRegex(helper.EdgeError, "closing"):
            late.allocate()
        self.assertEqual(late.calls, [])
        self.assertTrue((edge.state / "closing").is_file())

    def test_two_paused_allocations_share_admission_and_sweep_waits(self):
        self.race(publishing=False)

    def test_already_publishing_allocation_is_removed_before_sweep(self):
        self.race(publishing=True)

    def race(self, publishing):
        edge = FakeEdges()
        peers = [self.peer(edge), self.peer(edge)]
        paused = threading.Barrier(3)
        resume, closed = threading.Event(), threading.Event()
        tokens = []
        for peer in peers:
            original_save, original_docker = peer.save, peer.docker
            def save(token, record, original=original_save):
                original(token, record)
                if not record["released"]:
                    tokens.append(token)
                    if not publishing:
                        paused.wait(timeout=3)
                        if not resume.wait(3):
                            raise AssertionError("test did not resume allocation")
            def docker(*args, original=original_docker, **kwargs):
                result = original(*args, **kwargs)
                if publishing and args[0] == "start":
                    paused.wait(timeout=3)
                    if not resume.wait(3):
                        raise AssertionError("test did not resume publication")
                return result
            peer.save, peer.docker = save, docker
        mark = helper.admission.OwnerAdmission.mark
        def marked(owner, name):
            mark(owner, name)
            if name == "closing":
                closed.set()
        with mock.patch.object(helper.admission.OwnerAdmission, "mark", marked), \
             concurrent.futures.ThreadPoolExecutor(max_workers=3) as pool:
            allocations = [pool.submit(peers[0].allocate, A), pool.submit(peers[1].allocate, B)]
            try:
                # Both reach this barrier while holding shared admission. An
                # accidental exclusive/global allocation lock fails here.
                paused.wait(timeout=3)
                swept = pool.submit(edge.sweep)
                self.assertTrue(closed.wait(3))
                self.assertFalse(swept.done())
                self.assertEqual(len(tokens), 2)
                for token in tokens:
                    self.assertFalse(json.loads((edge.state / (token + ".json")).read_text())["released"])
                self.assertIn(("container", A), edge.resources)
                self.assertIn(("container", B), edge.resources)
                self.assertEqual(len(edge.edge_names()), 2 if publishing else 0)
            finally:
                resume.set()
            for future in allocations:
                with self.assertRaisesRegex(helper.EdgeError, "closing"):
                    future.result(timeout=3)
            swept.result(timeout=3)
        self.assertEqual(edge.resources, {})
        self.assertEqual(edge.proxies, {})
        if not publishing:
            self.assertFalse(any(c[0] == "create" for peer in peers for c in peer.calls))

    def test_bounded_exclusive_failure_and_cancel_keep_fence_and_reservations(self):
        for cancelled in (False, True):
            edge = FakeEdges()
            lease = edge.allocate()["lease"]
            before = dict(edge.resources)
            peer = self.peer(edge)
            if cancelled:
                def interrupt(*_args, **_kwargs):
                    helper.interrupted(signal.SIGTERM, None)
                with mock.patch.object(helper.admission, "locked", side_effect=interrupt):
                    with self.assertRaisesRegex(helper.EdgeError, "interrupted"):
                        peer.sweep()
            else:
                with edge.admission().hold(time.monotonic() + 3):
                    peer.deadline = time.monotonic() - 1
                    with self.assertRaisesRegex(helper.EdgeError, "timed out"):
                        peer.sweep()
            self.assertTrue((edge.state / "closing").exists())
            self.assertEqual(before, edge.resources)
            self.assertFalse(json.loads((edge.state / (lease + ".json")).read_text())["released"])
            self.assertIn(edge.route()["proxy"], edge.proxies)
            with self.assertRaisesRegex(helper.EdgeError, "closing"):
                self.peer(edge).allocate(B)

    def test_killed_mutator_marker_prevents_sweep_and_release_flags(self):
        edge = FakeEdges()
        lease = edge.allocate()["lease"]
        edge.admission().begin_mutation()  # durable before any Docker spawn
        edge.calls.clear()
        with self.assertRaisesRegex(helper.EdgeError, "uncertain"):
            edge.sweep()
        self.assertEqual(edge.calls, [])
        self.assertTrue(edge.edge_names())
        self.assertFalse(json.loads((edge.state / (lease + ".json")).read_text())["released"])

    def test_interrupted_start_keeps_durable_markers(self):
        edge = FakeEdges()
        process = mock.Mock(pid=1234)
        process.communicate.side_effect = helper.EdgeError("daemon request interrupted")
        with mock.patch.object(helper.subprocess, "Popen", return_value=process), \
             mock.patch.object(helper.os, "killpg"):
            with self.assertRaisesRegex(helper.EdgeError, "daemon request interrupted"):
                helper.Edges.docker(edge, "start", "own-edge")
        self.assertTrue((edge.state / "uncertain").exists())
        self.assertTrue(list(edge.state.glob("pending-*")))
        with self.assertRaisesRegex(helper.EdgeError, "uncertain"):
            edge.sweep()

    def test_pending_launch_denies_service_replacement(self):
        with tempfile.TemporaryDirectory() as root:
            fence = admission.OwnerAdmission(pathlib.Path(root) / "build/mg/conformance-routes" / OWNER)
            fence.begin_mutation()
            command = mock.Mock(side_effect=AssertionError("reservation must be checked before daemon access"))
            self.assertFalse(admission.admitted(root, "test-project", command))
            command.assert_not_called()


class Sockets(unittest.IsolatedAsyncioTestCase):
    async def asyncSetUp(self):
        self.loop = asyncio.get_running_loop()
        self.relay = forward.load_relay(ROOT / "docker/sync-edge/forward.py")
        self.upstream = socket.socket()
        self.upstream.bind(("127.0.0.1", 0))
        self.upstream.listen()
        self.upstream.setblocking(False)
        self.listener = socket.socket()
        self.listener.bind(("127.0.0.1", 0))
        self.listener.listen()
        self.listener.setblocking(False)
        self.client = socket.socket()
        self.client.setblocking(False)
        await self.loop.sock_connect(self.client, self.listener.getsockname())
        accepted, _ = await self.loop.sock_accept(self.listener)
        accepted.setblocking(False)
        self.transfer = asyncio.create_task(self.relay.serve(self.loop, accepted, *self.upstream.getsockname()))
        self.server, _ = await asyncio.wait_for(self.loop.sock_accept(self.upstream), 2)
        self.server.setblocking(False)

    async def asyncTearDown(self):
        self.transfer.cancel()
        await asyncio.gather(self.transfer, return_exceptions=True)
        for sock in (self.client, self.server, self.upstream, self.listener):
            sock.close()

    async def recv(self, sock):
        return await asyncio.wait_for(self.loop.sock_recv(sock, 65536), 2)

    async def test_client_half_close_allows_upstream_response(self):
        payload = b"opaque\x00request\r\n"
        await self.loop.sock_sendall(self.client, payload)
        self.client.shutdown(socket.SHUT_WR)
        self.assertEqual(await self.recv(self.server), payload)
        self.assertEqual(await self.recv(self.server), b"")
        await self.loop.sock_sendall(self.server, b"response after FIN")
        self.server.shutdown(socket.SHUT_WR)
        self.assertEqual(await self.recv(self.client), b"response after FIN")
        self.assertEqual(await self.recv(self.client), b"")
        await asyncio.wait_for(self.transfer, 2)

    async def test_upstream_half_close_allows_client_to_finish(self):
        self.server.shutdown(socket.SHUT_WR)
        self.assertEqual(await self.recv(self.client), b"")
        await self.loop.sock_sendall(self.client, b"still writing")
        self.client.shutdown(socket.SHUT_WR)
        self.assertEqual(await self.recv(self.server), b"still writing")
        self.assertEqual(await self.recv(self.server), b"")
        await asyncio.wait_for(self.transfer, 2)

    async def test_upstream_reset_reaches_client(self):
        self.server.setsockopt(socket.SOL_SOCKET, socket.SO_LINGER, struct.pack("ii", 1, 0))
        self.server.close()
        with self.assertRaises(ConnectionResetError):
            await self.recv(self.client)
        await asyncio.wait_for(self.transfer, 2)

    async def test_client_reset_reaches_upstream(self):
        self.client.setsockopt(socket.SOL_SOCKET, socket.SO_LINGER, struct.pack("ii", 1, 0))
        self.client.close()
        with self.assertRaises(ConnectionResetError):
            await self.recv(self.server)
        await asyncio.wait_for(self.transfer, 2)

class Launcher(unittest.TestCase):
    def test_holder_owns_namespace_and_entrypoint_owns_driver_clock(self):
        import contextlib
        import types
        worker = load("worker_launcher", ROOT / "tools/mg.d/conformance-worker.py")
        owner_admission = mock.Mock()
        owner_admission.hold.return_value = contextlib.nullcontext()
        admission_module = types.SimpleNamespace(launch_admission=lambda _: contextlib.nullcontext(),
                                                OwnerAdmission=lambda _: owner_admission)
        fake_spec = types.SimpleNamespace(loader=mock.Mock())
        argv = ["launcher", "--root", str(ROOT), "--project", "test-project", "--run", "test-run",
                "--owner", OWNER, "--worker", A, "--handoff", "/tmp/handoff with space",
                "--faults", "sync-faults:8474", "--tz", "UTC"]
        def inspected(*args, **kwargs):
            command = started.call_args.args[0]
            labels = dict(command[i+1].split("=", 1) for i, word in enumerate(command) if word == "--label")
            return json.dumps([{"State": {"Running": True}, "HostConfig": {"AutoRemove": False},
                                "Config": {"Labels": labels}}]).encode()
        with mock.patch.object(worker.sys, "argv", argv), \
             mock.patch.object(worker.importlib.util, "spec_from_file_location", return_value=fake_spec), \
             mock.patch.object(worker.importlib.util, "module_from_spec", return_value=admission_module), \
             mock.patch.object(worker.subprocess, "run") as started, \
             mock.patch.object(worker.subprocess, "check_output", side_effect=inspected), \
             mock.patch.object(worker.os, "execvp") as attached:
            worker.main()
        command = started.call_args.args[0]
        self.assertNotIn("--rm", command)
        self.assertIn("-d", command)
        self.assertEqual(command[-5:], ["--entrypoint", "python3", "oracle", "-u", "/opt/oracle/worker_holder.py"])
        self.assertIn("ORACLE_SYNC_EDGE_OWNER=" + OWNER, command)
        self.assertIn("ORACLE_SYNC_WORKER=" + A, command)
        self.assertIn("meguri.mg.worker=" + A, command)
        self.assertIn("LD_PRELOAD=", command)
        self.assertIn("/tmp/handoff with space:/handoff", command)
        name = "meguri-conf-worker-" + OWNER + "-" + A
        attached.assert_called_once_with("docker", ["docker", "exec", "-i", name,
                                                   "/opt/oracle/entrypoint.sh", "worker"])

    def test_closed_owner_refuses_holder_launch_before_docker(self):
        import types
        worker = load("closed_worker_launcher", ROOT / "tools/mg.d/conformance-worker.py")
        fake_spec = types.SimpleNamespace(loader=mock.Mock())
        with tempfile.TemporaryDirectory() as root:
            owner = admission.OwnerAdmission(pathlib.Path(root) / "build/mg/conformance-routes" / OWNER)
            owner.mark("closing")
            argv = ["launcher", "--root", root, "--project", "test-project", "--run", "test-run",
                    "--owner", OWNER, "--worker", A, "--handoff", "/tmp/handoff",
                    "--faults", "sync-faults:8474", "--tz", "UTC"]
            with mock.patch.object(worker.sys, "argv", argv), \
                 mock.patch.object(worker.importlib.util, "spec_from_file_location", return_value=fake_spec), \
                 mock.patch.object(worker.importlib.util, "module_from_spec", return_value=admission), \
                 mock.patch.object(worker.subprocess, "run") as started, \
                 mock.patch.object(worker.subprocess, "check_output") as inspected, \
                 mock.patch.object(worker.os, "execvp") as attached:
                with self.assertRaisesRegex(admission.AdmissionError, "closing"):
                    worker.main()
            started.assert_not_called()
            inspected.assert_not_called()
            attached.assert_not_called()
