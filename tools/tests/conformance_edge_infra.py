"""MG-only real Docker route/namespace transport harness.

Neutral byte servers are transport fixtures, never conformance executors or
scenario results. Production T1 still runs the real driver and native sync.
No oracle implementation is imported, mounted or inspected by this harness.
"""
import argparse
import concurrent.futures
import importlib.util
import json
import os
import pathlib
import select
import signal
import socket
import subprocess
import sys
import threading
import time
import uuid


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


# One backend per namespace, all at the SAME numeric port. Stdout reports
# readiness/counts without making a network connection. The stdin owner can
# model orderly EOF and independent self-exit while the real holder stays up.
BACKEND = r'''
import os,socket,sys,threading
listener=socket.socket(); listener.setsockopt(socket.SOL_SOCKET,socket.SO_REUSEADDR,1)
listener.bind(('0.0.0.0',27701)); listener.listen(); count=0
identity=sys.argv[1].encode()
def serve():
 global count
 while True:
  c,_=listener.accept(); count+=1
  try:
   c.recv(4096); c.sendall(identity); c.shutdown(socket.SHUT_WR)
  finally: c.close()
threading.Thread(target=serve,daemon=True).start()
print('ready',flush=True)
for line in sys.stdin:
 if line.strip()=='count': print(count,flush=True)
 if line.strip()=='selfexit': os._exit(7)
'''


# Only test-controlled barriers are injected; allocation, process admission,
# Docker publication and the production helper CLI are otherwise unchanged.
PAUSED_ALLOCATOR = r'''
import importlib.util,os,pathlib,select,signal,sys
root,phase,paused,resume=sys.argv[1:5]
paused,resume=int(paused),int(resume)
sys.argv=[sys.argv[0]]+sys.argv[5:]
spec=importlib.util.spec_from_file_location('paused_edges',pathlib.Path(root)/'tools/mg.d/conformance-edge.py')
m=importlib.util.module_from_spec(spec); spec.loader.exec_module(m)
def pause():
 if phase.startswith('cancel-'): signal.signal(signal.SIGTERM,signal.SIG_DFL)
 os.write(paused,b'P'); os.close(paused)
 if not select.select([resume],[],[],15)[0] or os.read(resume,1)!=b'R':
  raise m.EdgeError('allocation barrier cancelled')
 os.close(resume)
save,docker=m.Edges.save,m.Edges.docker
def saved(self,token,record):
 save(self,token,record)
 if phase in ('record','cancel-record') and not record['released']: pause()
def published(self,*args,**kwargs):
 result=docker(self,*args,**kwargs)
 if phase in ('published','cancel-published') and args[0]=='exec' and result is not None: pause()
 return result
m.Edges.save=saved; m.Edges.docker=published
sys.exit(m.main())
'''


def read_line(child):
    if not select.select([child.stdout], [], [], 15)[0]:
        raise AssertionError("fixture driver response timed out")
    line = child.stdout.readline()
    if not line:
        raise AssertionError("fixture driver exited")
    return line.decode().strip()


def run(args):
    root = pathlib.Path(args.root).resolve()
    edge_module = load("infra_edges", root / "tools/mg.d/conformance-edge.py")
    admission = load("infra_admission", root / "tools/mg.d/conformance-admission.py")
    edges = edge_module.Edges(root, args.project, args.run, args.image, "sync-faults:8474", args.owner)
    processes = []
    names, tokens, targets, routes, leases = [], [], [], [], []
    contexts = load("infra_contexts", root / "tools/mg.d/conformance-owners.py")

    def remember(owner):
        contexts.register(root, args.project, args.run, args.run_dir,
                          owner.owner, args.image, "sync-faults:8474")
        owners.append(owner)
        return owner

    owners = []
    remember(edges)
    peer = edge_module.Edges(root, args.project, args.run, args.image,
                               "sync-faults:8474", args.peer_owner)
    remember(peer)

    def docker(*argv):
        return subprocess.check_output(["docker", *argv], stderr=subprocess.DEVNULL, timeout=25).decode().strip()

    def fresh(owner=None):
        owner = edges if owner is None else owner
        owner.deadline = time.monotonic() + 25
        return owner

    def control(method, path, payload):
        # Direct, temporary internal API client; no host API publication.
        code = ("import json,sys,urllib.request,urllib.error; "
                "r=urllib.request.Request('http://sync-faults:8474'+sys.argv[2],"
                "data=sys.argv[3].encode(),method=sys.argv[1],headers={'Content-Type':'application/json'});"
                "\ntry:\n with urllib.request.urlopen(r,timeout=3) as f: print(f.status)"
                "\nexcept urllib.error.HTTPError as e: print(e.code)")
        return int(docker("run", "--rm", "--label", "meguri.mg.run-id=" + args.run,
                          "--network", args.project + "_oracle-internal", "--env", "LD_PRELOAD=",
                          "--entrypoint", "python3", args.image, "-I", "-B", "-c", code,
                          method, path, json.dumps(payload)))

    def start(owner, index):
        token = uuid.uuid4().hex
        name = "meguri-conf-worker-" + owner.owner + "-" + token
        labels = dict(owner.labels, **{"meguri.mg.purpose": edge_module.WORKER_PURPOSE,
                                      edge_module.WORKER: token})
        compose = ["compose", "-p", args.project, "-f", str(root / "docker/compose.yml"),
                   "run", "-d", "-T", "--no-deps", "--name", name]
        for key, value in labels.items():
            compose += ["--label", key + "=" + value]
        compose += ["-e", "ORACLE_SYNC_EDGE_OWNER=" + owner.owner, "-e", "ORACLE_SYNC_WORKER=" + token,
                    "-e", "LD_PRELOAD=", "--entrypoint", "python3", "oracle", "-u", "/opt/oracle/worker_holder.py"]
        with owner.admission().hold(time.monotonic() + 25, publishing=True), admission.launch_admission(str(root)):
            owner.admission().require_open()
            pending = owner.admission().begin_mutation()
            docker(*compose)
            pending.unlink()
        state = fresh(owner).inspect(name)
        assert state["State"]["Running"] and not state["HostConfig"]["AutoRemove"]
        # This synthetic socket driver belongs ONLY to the transport fixture;
        # real worker launch/entrypoint is checked separately below.
        process = subprocess.Popen(["docker", "exec", "-i", name, "python3", "-u", "-c", BACKEND, token],
                                 stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
        assert read_line(process) == "ready"
        ip = state["NetworkSettings"]["Networks"][args.project + "_oracle-internal"]["IPAddress"]
        proxy = "meguri-edge-" + owner.owner + "-" + uuid.uuid4().hex
        # Distinct fixture slots; no backend or edge readiness connection.
        assert control("POST", "/proxies", {"name": proxy, "listen": "0.0.0.0:" + str(40000 + int(args.owner[:4], 16) % 20000 + index),
                                              "upstream": ip + ":27701", "enabled": True}) == 201
        actual = fresh(owner).api("GET")[proxy]
        port = actual["listen"].rsplit(":", 1)[1]
        assert int(port) > 0
        return token, name, process, "http://sync-faults:" + port + "/", {"proxy": proxy, "worker": token}

    def exchange(endpoint, token):
        port = int(endpoint.removeprefix("http://127.0.0.1:").rstrip("/"))
        with socket.create_connection(("127.0.0.1", port), 5) as client:
            client.sendall(b"neutral byte transport")
            client.shutdown(socket.SHUT_WR)
            assert client.recv(4096) == token.encode()
            assert client.recv(4096) == b""

    def new_owner():
        owner = edge_module.Edges(root, args.project, args.run, args.image,
                                  "sync-faults:8474", uuid.uuid4().hex)
        return remember(owner)

    def allocation_sweep_race(phase, index):
        owner = new_owner()
        token, name, backend_process, target, route = start(owner, index)
        processes.append(backend_process)
        actual = fresh(owner).api("GET")[route["proxy"]]
        pause_read, pause_write = os.pipe()
        resume_read, resume_write = os.pipe()
        command = ["python3", "-I", "-B", "-c", PAUSED_ALLOCATOR, str(root), phase,
                   str(pause_write), str(resume_read), "--root", str(root), "--project", args.project,
                   "--run", args.run, "--image", args.image, "--faults", "sync-faults:8474", "--owner", owner.owner]
        process = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                 stderr=subprocess.DEVNULL, start_new_session=True,
                                 pass_fds=(pause_write, resume_read))
        os.close(pause_write); os.close(resume_read)
        process.stdin.write(json.dumps({"cmd": "acquire", "target": target, "route": route,
                                     "expected_worker": token}).encode())
        process.stdin.close()
        # Lose the CLI/reply pipe independently of the helper process group.
        process.stdout.close()
        closed = threading.Event()
        class SweepAdmission(edge_module.admission.OwnerAdmission):
            def mark(self, name):
                super().mark(name)
                if name == "closing":
                    closed.set()
        sweeper = edge_module.Edges(root, args.project, args.run, args.image,
                                    "sync-faults:8474", owner.owner)
        sweeper.admission = lambda: SweepAdmission(sweeper.state)
        old_endpoint = None
        try:
            assert select.select([pause_read], [], [], 15)[0], "allocation did not reach barrier"
            assert os.read(pause_read, 1) == b"P"
            records = list(owner.state.glob("*.json"))
            assert len(records) == 1 and not json.loads(records[0].read_text())["released"]
            ids = fresh(owner).docker("ps", "-aq", *owner.filters()).split()
            if phase == "published":
                assert len(ids) == 1
                mapping = fresh(owner).inspect(ids[0])["NetworkSettings"]["Ports"]["8080/tcp"][0]
                old_endpoint = "http://127.0.0.1:" + mapping["HostPort"] + "/"
                exchange(old_endpoint, token)
            else:
                assert not ids
            with concurrent.futures.ThreadPoolExecutor(max_workers=1) as pool:
                swept = pool.submit(fresh(sweeper).sweep)
                try:
                    assert closed.wait(5), "sweep did not close admission"
                    assert not swept.done(), "sweep passed an active publisher"
                    assert not json.loads(records[0].read_text())["released"]
                    assert fresh(owner).inspect(name)["State"]["Running"]
                    assert route["proxy"] in fresh(owner).api("GET")
                finally:
                    os.write(resume_write, b"R")
                assert process.wait(timeout=20) != 0, "orphan allocation returned success"
                swept.result(timeout=30)
            assert json.loads(records[0].read_text())["released"]
            assert not fresh(owner).docker("ps", "-aq", *owner.filters()).split()
            assert not fresh(owner).docker("network", "ls", "-q", *owner.filters()).split()
            assert not fresh(owner).workers()
            # Fresh helper processes see terminal closure; unit coverage also
            # checks the holder launcher before any Docker command.
            late = subprocess.run(["python3", "-I", "-B", str(root / "tools/mg.d/conformance-edge.py"),
                                   "--root", str(root), "--project", args.project, "--run", args.run,
                                   "--image", args.image, "--faults", "sync-faults:8474", "--owner", owner.owner],
                                  input=json.dumps({"cmd": "acquire", "target": target, "route": route,
                                                    "expected_worker": token}).encode(),
                                  stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=10)
            assert late.returncode != 0
            # Reassign the exact old proxy listener to a live peer whose
            # backend uses the same numeric port. No old route may reach it.
            upstream = fresh(peer).api("GET")[peer_route["proxy"]]["upstream"]
            replacement = "meguri-edge-" + peer.owner + "-" + uuid.uuid4().hex
            assert control("POST", "/proxies", {"name": replacement, "listen": actual["listen"],
                                                "upstream": upstream, "enabled": True}) == 201
            if old_endpoint is not None:
                port = int(old_endpoint.removeprefix("http://127.0.0.1:").rstrip("/"))
                try:
                    with socket.create_connection(("127.0.0.1", port), 3):
                        raise AssertionError("old edge listener survived sweep")
                except ConnectionRefusedError:
                    pass
            peer_process.stdin.write(b"count\n"); peer_process.stdin.flush()
            assert read_line(peer_process) == "0", "late edge reached peer after port reuse"
            assert control("DELETE", "/proxies/" + replacement, {}) in (204, 200)
        finally:
            os.close(pause_read); os.close(resume_write)
            if process.poll() is None:
                process.terminate(); process.wait(timeout=10)

    def cancellation_fixture():
        owner = new_owner()
        token, name, process, target, route = start(owner, 8)
        processes.append(process)
        if args.cancel_fixture == "published":
            paused_read, paused_write = os.pipe()
            resume_read, resume_write = os.pipe()
            command = ["python3", "-I", "-B", "-c", PAUSED_ALLOCATOR, str(root), "cancel-published",
                       str(paused_write), str(resume_read), "--root", str(root), "--project", args.project,
                       "--run", args.run, "--image", args.image, "--faults", "sync-faults:8474", "--owner", owner.owner]
            allocator = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                         stderr=subprocess.DEVNULL, pass_fds=(paused_write, resume_read))
            processes.append(allocator)
            os.close(paused_write); os.close(resume_read)
            allocator.stdin.write(json.dumps({"cmd": "acquire", "target": target, "route": route,
                                             "expected_worker": token}).encode())
            allocator.stdin.close()
            allocator.stdout.close()
            assert select.select([paused_read], [], [], 15)[0], "publication barrier timed out"
            assert os.read(paused_read, 1) == b"P"
            assert len(fresh(owner).docker("ps", "-aq", *owner.filters()).split()) == 1
        else:
            assert not list(owner.state.glob("*.json")), "before-token fixture acquired a lease"
        # This pipe is private test coordination, never an MG report artifact.
        payload = dict(run=args.run, run_dir=args.run_dir, owner=owner.owner,
                       worker=name, route=route)
        os.write(args.ready_fd, json.dumps(payload).encode() + b"\n")
        os.close(args.ready_fd)
        signal.pause()  # SIGTERM uses the default disposition: no Python finally.
        raise AssertionError("cancellation fixture resumed")

    def cancel_job(phase):
        ready_read, ready_write = os.pipe()
        log = pathlib.Path(args.run_dir) / ("cancel-" + phase + ".log")
        with log.open("wb") as output:
            job = subprocess.Popen([str(root / "tools/mg"), "conformance-route-test",
                                    "--cancel-fixture", phase, "--ready-fd", str(ready_write)],
                                   stdout=subprocess.PIPE, stderr=output, pass_fds=(ready_write,))
            os.close(ready_write)
            try:
                assert select.select([ready_read], [], [], 15)[0], "MG fixture did not reach cancellation barrier"
                with os.fdopen(ready_read, "rb") as ready:
                    ready_read = None
                    payload = json.loads(ready.readline())
                owner = edge_module.Edges(root, args.project, payload["run"], args.image,
                                          "sync-faults:8474", payload["owner"])
                trusted = contexts.records(root, args.project, payload["run"], payload["run_dir"])
                assert len(trusted) == 3
                assert any(record["owner"] == owner.owner for record in trusted)
                assert fresh(owner).inspect(payload["worker"])["State"]["Running"]
                assert payload["route"]["proxy"] in fresh(owner).api("GET")
                # End the actual MG job, not a helper: its parent must read the
                # extra owner's durable record after killing the harness tree.
                job.terminate()
                stdout, _ = job.communicate(timeout=30)
                assert job.returncode == 2
                result = json.loads(pathlib.Path(stdout.decode().strip()).read_text())
                assert result["status"] == "error"
                assert any(failure["id"] == "cancelled" for failure in result["failures"])
                assert not any(failure["id"] == "live/edge-cleanup" for failure in result["failures"])
                assert not (pathlib.Path(payload["run_dir"]) / "cancel-finally").exists()
                assert (owner.state / "closing").exists(), "parent never swept the additional owner"
                assert not fresh(owner).docker("ps", "-aq", *owner.filters()).split()
                assert not fresh(owner).docker("network", "ls", "-q", *owner.filters()).split()
                assert not fresh(owner).workers()
                assert payload["route"]["proxy"] not in fresh(owner).api("GET")
                records = list(owner.state.glob("*.json"))
                assert len(records) == (1 if phase == "published" else 0)
                assert all(json.loads(record.read_text())["released"] for record in records)
                # These are real resources from a different run. Both the main
                # owner and the peer remain live through nested parent cleanup.
                for name in names + [peer_name]:
                    assert fresh().inspect(name)["State"]["Running"]
                for route in routes + [peer_route]:
                    assert route["proxy"] in fresh().api("GET")
            finally:
                if ready_read is not None:
                    os.close(ready_read)
                if job.poll() is None:
                    job.terminate()
                    job.communicate(timeout=30)

    try:
        if args.cancel_fixture:
            cancellation_fixture()
        # Four independent namespaces are simultaneously alive; no session lock.
        with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
            started = list(pool.map(lambda i: start(edges, i), range(4)))
        for token, name, process, target, route in started:
            tokens.append(token); names.append(name); processes.append(process)
            targets.append(target); routes.append(route)
        assert len(set(fresh().inspect(name)["NetworkSettings"]["Networks"][args.project + "_oracle-internal"]["IPAddress"]
                       for name in names)) == 4
        peer_token, peer_name, peer_process, peer_target, peer_route = start(peer, 4)
        processes.append(peer_process)
        cancel_job("before-token")
        cancel_job("published")
        allocation_sweep_race("record", 6)
        allocation_sweep_race("published", 7)
        # B's complete tuple is consistent but never authority for A.
        try:
            fresh().acquire(targets[1], routes[1], tokens[0])
            raise AssertionError("cross-worker tuple accepted")
        except edge_module.EdgeError:
            pass
        def allocate(index):
            allocator = edge_module.Edges(root, args.project, args.run, args.image,
                                          "sync-faults:8474", edges.owner)
            return fresh(allocator).acquire(targets[index], routes[index], tokens[index])
        with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
            leases.extend(pool.map(allocate, range(4)))
        for index in range(4):
            processes[index].stdin.write(b"count\n"); processes[index].stdin.flush()
            assert read_line(processes[index]) == "0", "startup probe contacted the backend"
        with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
            list(pool.map(lambda pair: exchange(pair[0]["endpoint"], pair[1]), zip(leases, tokens)))
        # Independent exit/EOF leaves the container/address held. Inject only
        # removal failure; production helper and Docker route remain real.
        processes[0].stdin.write(b"selfexit\n"); processes[0].stdin.flush(); processes[0].wait(timeout=10)
        processes[1].stdin.close(); processes[1].wait(timeout=10)
        original = edges.docker
        def blocked(*argv, **kwargs):
            if argv[:2] == ("rm", "-f"):
                raise edge_module.EdgeError("injected edge removal failure")
            return original(*argv, **kwargs)
        edges.docker = blocked
        try:
            fresh().remove(leases[0]["lease"])
            raise AssertionError("unproved removal succeeded")
        except edge_module.EdgeError:
            pass
        assert fresh().inspect(names[0])["State"]["Running"]
        assert routes[0]["proxy"] in fresh().api("GET")
        assert not admission.admitted(str(root), args.project, docker)
        # While the original edge survives, its proxy keeps its listening port.
        # Pointing a replacement at the other backend's equal port must fail.
        upstream_b = fresh().api("GET")[routes[1]["proxy"]]["upstream"]
        listen_a = fresh().api("GET")[routes[0]["proxy"]]["listen"]
        assert control("POST", "/proxies", {"name": "meguri-edge-" + args.owner + "-" + uuid.uuid4().hex,
                                               "listen": listen_a, "upstream": upstream_b}) >= 400
        # No old edge may reach B; the dead backend is still an allocated
        # namespace, while two unaffected peers continue to exchange bytes.
        try:
            exchange(leases[0]["endpoint"], tokens[1])
            raise AssertionError("old edge reached peer")
        except (OSError, AssertionError) as error:
            if str(error) == "old edge reached peer":
                raise
        exchange(leases[2]["endpoint"], tokens[2])
        exchange(leases[3]["endpoint"], tokens[3])
        edges.docker = original
        # Proxy-delete failure is visible after edge removal; reservation stays.
        original_api = edges.api
        def no_delete(method, proxy=None):
            if method == "DELETE":
                raise edge_module.EdgeError("injected proxy delete failure")
            return original_api(method, proxy)
        edges.api = no_delete
        try:
            fresh().remove(leases[0]["lease"])
            raise AssertionError("proxy delete failure hidden")
        except edge_module.EdgeError:
            pass
        assert fresh().inspect(names[0])["State"]["Running"]
        assert routes[0]["proxy"] in fresh().api("GET")
        edges.api = original_api
        # Lost acquire replies and before-token proxies are covered by owned
        # sweep; foreign owner, worker and proxy must survive it.
        fresh().sweep()
        assert fresh(peer).inspect(peer_name)["State"]["Running"]
        assert peer_route["proxy"] in fresh(peer).api("GET")
        fresh().sweep()
        edges = new_owner()
        # Real launch contract: holder first, exactly one entrypoint driver,
        # no --rm, no replacement launch in a retained namespace after EOF.
        worker = uuid.uuid4().hex
        launcher = ["python3", "-I", "-B", str(root / "tools/mg.d/conformance-worker.py"),
                    "--root", str(root), "--project", args.project, "--run", args.run,
                    "--owner", edges.owner, "--worker", worker, "--handoff", args.handoff,
                    "--faults", "sync-faults:8474", "--tz", "UTC"]
        scenario = json.loads(pathlib.Path(args.scenario).read_text())["value"]
        begin = {"cmd": "begin", "id": scenario["id"], "scenario": scenario, "caller_actors": ["meguri"]}
        # Normal edge-first end permits a second session on this same driver.
        process = subprocess.Popen(launcher, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
        processes.append(process)
        process.stdin.write(b'{"cmd":"hello"}\n'); process.stdin.flush()
        assert json.loads(read_line(process))["ok"] is True
        for _ in range(2):
            process.stdin.write(json.dumps(begin).encode() + b"\n"); process.stdin.flush()
            begun = json.loads(read_line(process))
            assert begun["ok"] is True
            sync = begun["sync"]
            assert sync["route"]["worker"] == worker
            lease = fresh().acquire(sync["endpoints"]["actors"], sync["route"], worker)
            fresh().remove(lease["lease"])
            process.stdin.write(b'{"cmd":"end"}\n'); process.stdin.flush()
            assert json.loads(read_line(process))["ok"] is True
        # EOF after begin must defer caller-owned proxy deletion. Acquire then
        # lose the driver: the namespace and named proxy remain reserved.
        process.stdin.write(json.dumps(begin).encode() + b"\n"); process.stdin.flush()
        sync = json.loads(read_line(process))["sync"]
        lease = fresh().acquire(sync["endpoints"]["actors"], sync["route"], worker)
        process.stdin.close(); process.wait(timeout=15)
        name = "meguri-conf-worker-" + edges.owner + "-" + worker
        assert fresh().inspect(name)["State"]["Running"]
        assert sync["route"]["proxy"] in fresh().api("GET")
        assert subprocess.run(launcher, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                              stderr=subprocess.DEVNULL, timeout=25).returncode != 0
        fresh().remove(lease["lease"])
        fresh().sweep()
        # Also lose begin's reply entirely, including the before-token case.
        edges = new_owner()
        worker2 = uuid.uuid4().hex
        launcher2 = list(launcher)
        launcher2[launcher2.index("--owner") + 1] = edges.owner
        launcher2[launcher2.index("--worker") + 1] = worker2
        process = subprocess.Popen(launcher2, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
        processes.append(process)
        process.stdin.write(json.dumps(begin).encode() + b"\n"); process.stdin.flush()
        process.stdin.close(); process.wait(timeout=15)
        assert fresh().inspect("meguri-conf-worker-" + edges.owner + "-" + worker2)["State"]["Running"]
        fresh().sweep()
    finally:
        if args.cancel_fixture:
            (pathlib.Path(args.run_dir) / "cancel-finally").touch()
        edges.docker = edge_module.Edges.docker.__get__(edges)
        edges.api = edge_module.Edges.api.__get__(edges)
        # Never force workers away if removal fails: propagate and retain them.
        try:
            for owner in owners:
                fresh(owner).sweep()
        finally:
            for process in processes:
                if process.stdin and not process.stdin.closed:
                    process.stdin.close()
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill(); process.wait()


def main():
    parser = argparse.ArgumentParser()
    for key in ("root", "project", "run", "run-dir", "image", "owner", "peer-owner", "handoff"):
        parser.add_argument("--" + key, required=True)
    parser.add_argument("--scenario")
    parser.add_argument("--cancel-fixture", choices=("before-token", "published"))
    parser.add_argument("--ready-fd", type=int)
    args = parser.parse_args()
    if args.cancel_fixture:
        if args.ready_fd is None:
            parser.error("cancellation fixture needs its coordination pipe")
    elif args.scenario is None:
        parser.error("transport run needs its scenario input")
    run(args)
    print("whole-route infrastructure transport checks passed")


if __name__ == "__main__":
    main()
