"""Trusted host-only native T1 edge allocator (sync-ops.md S8.4).

One request per process. Transport state is never written to a report. Docker
ownership filters cover resources even before acquire returns a lease token.
"""
import argparse
import hashlib
import importlib.util
import ipaddress
import json
import pathlib
import os
import re
import signal
import subprocess
import sys
import time
import uuid

PURPOSE = "conformance-actor-edge"
WORKER_PURPOSE = "conformance-worker"
OWNER = "meguri.mg.edge-owner"
WORKER = "meguri.mg.worker"
PROXY = "meguri.mg.proxy"
PORT = 8080
LISTENER = ("import sys; up = {int(l.split()[1].rsplit(':', 1)[1], 16) "
            "for l in list(open('/proc/net/tcp'))[1:] if l.split()[3] == '0A'}; "
            "sys.exit(0 if 8080 in up else 1)")


_spec = importlib.util.spec_from_file_location(
    "route_admission", pathlib.Path(__file__).with_name("conformance-admission.py"))
admission = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(admission)
EdgeError = admission.AdmissionError


def strict_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise EdgeError("duplicate member")
        result[key] = value
    return result


def target_address(target, faults):
    """Accept the exact root URL authority, without URL normalization."""
    configured = re.fullmatch(r"([a-zA-Z0-9][a-zA-Z0-9_.-]*):([0-9]+)", faults)
    if not configured or not 0 < int(configured[2]) <= 65535:
        raise EdgeError("invalid trusted fault configuration")
    match = re.fullmatch(r"http://([a-zA-Z0-9][a-zA-Z0-9_.-]*):([0-9]+)/", target)
    if (not match or match[1] != configured[1] or not 0 < int(match[2]) <= 65535
            or int(match[2]) == int(configured[2])):
        raise EdgeError("target refused")
    return match[1], int(match[2])


class Edges:
    def __init__(self, root, project, run, image, faults, owner):
        self.root = str(pathlib.Path(root).resolve())
        self.project, self.run, self.image, self.faults = project, run, image, faults
        self.owner = owner
        if not all((project, run, image)) or not isinstance(owner, str) or not re.fullmatch(r"[0-9a-f]{32}", owner):
            raise EdgeError("missing trusted configuration")
        self.scope = hashlib.sha256((self.root + '\0' + project + '\0' + run + '\0' + owner).encode()).hexdigest()[:24]
        self.labels = {"meguri.mg.run-id": run, "meguri.mg.checkout": self.root,
                       "meguri.mg.project": project, "meguri.mg.purpose": PURPOSE, OWNER: owner}
        self.deadline = time.monotonic() + 20
        # Keep route ownership in temporary local state, outside reports.
        # Record it ahead of creation and preserve it when replies are lost.
        self.state = pathlib.Path(self.root) / "build/mg/conformance-routes" / owner

    def admission(self):
        return admission.OwnerAdmission(self.state)

    def docker(self, *args, missing=False):
        remaining = self.deadline - time.monotonic()
        if remaining <= 0:
            raise EdgeError("edge operation timed out")
        # Never surface Docker diagnostics: they may contain runtime routes.
        mutation = args[0] in ("create", "start", "rm") or args[:2] in (
            ("network", "create"), ("network", "connect"), ("network", "rm"))
        pending = self.admission().begin_mutation() if mutation else None
        child = None
        try:
            child = subprocess.Popen(["docker", *args], stdin=subprocess.DEVNULL,
                                     stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                                     start_new_session=True)
            out, _ = child.communicate(timeout=min(5, remaining))
        except BaseException:
            # Killing a Docker client does not cancel an accepted daemon
            # mutation. Retain all reservations if its outcome is ambiguous.
            if mutation:
                self.admission().mark("uncertain")
            if child is not None:
                os.killpg(child.pid, signal.SIGKILL)
                child.wait()
            raise
        if pending is not None:
            pending.unlink()
        if child.returncode and not missing:
            raise EdgeError("Docker edge operation failed")
        return out.decode().strip() if child.returncode == 0 else None

    def filters(self, token=None):
        labels = dict(self.labels)
        if token is not None:
            labels["meguri.mg.edge-lease"] = token
        return [arg for key, value in labels.items() for arg in ("--filter", f"label={key}={value}")]

    def label_args(self, token, route):
        return [arg for key, value in dict(self.labels, **{"meguri.mg.edge-lease": token, WORKER: route["worker"], PROXY: route["proxy"]}).items()
                for arg in ("--label", f"{key}={value}")]

    def owned_token(self, token):
        if not re.fullmatch(re.escape(self.scope) + r"\.[0-9a-f]{32}", token):
            raise EdgeError("foreign or invalid lease")

    def inspect(self, name):
        value = json.loads(self.docker("inspect", name))
        if not isinstance(value, list) or len(value) != 1:
            raise EdgeError("invalid resource inspection")
        return value[0]

    def workers(self):
        labels = dict(self.labels, **{"meguri.mg.purpose": WORKER_PURPOSE})
        filters = [a for k, v in labels.items() for a in ("--filter", f"label={k}={v}")]
        result = {}
        for cid in self.docker("ps", "-aq", *filters).split():
            state = self.inspect(cid)
            actual = state["Config"]["Labels"]
            token = actual.get(WORKER, "")
            if (any(actual.get(k) != v for k, v in labels.items())
                    or not re.fullmatch(r"[0-9a-f]{32}", token) or token == self.owner
                    or token in result or state["HostConfig"].get("AutoRemove")):
                raise EdgeError("worker ownership refused")
            result[token] = state
        return result

    def api(self, method, proxy=None):
        # A short-lived host-owned control invocation, never exec in a retained
        # worker. No listening control service and no published API. Arguments
        # are direct argv, and neither accounts nor scenario data enter here.
        target_address("http://" + self.faults.split(":")[0] + ":1/", self.faults)
        path = "/proxies" + ("/" + proxy if proxy else "")
        if proxy is not None and not self.proxy_name(proxy):
            raise EdgeError("proxy ownership refused")
        script = ("import json,sys,urllib.request,urllib.error; "
                  "u='http://'+sys.argv[1]+sys.argv[3]; "
                  "r=urllib.request.Request(u,method=sys.argv[2]); "
                  "\ntry:\n with urllib.request.urlopen(r,timeout=3) as f: "
                  "print(json.dumps({'status':f.status,'body':f.read().decode()}))"
                  "\nexcept urllib.error.HTTPError as e: "
                  "print(json.dumps({'status':e.code,'body':''}))")
        name = "meguri-conf-control-" + uuid.uuid4().hex
        out = self.docker("run", "--rm", "--name", name,
                          "--label", "meguri.mg.run-id=" + self.run,
                          "--label", "meguri.mg.purpose=conformance-route-control",
                          "--network", self.project + "_oracle-internal",
                          "--read-only", "--cap-drop", "ALL", "--security-opt", "no-new-privileges",
                          "--env", "LD_PRELOAD=", "--entrypoint", "python3", self.image,
                          "-I", "-B", "-c", script, self.faults, method, path)
        response = json.loads(out)
        if method == "DELETE" and response["status"] == 404:
            return None
        if not 200 <= response["status"] < 300:
            raise EdgeError("proxy operation failed")
        return json.loads(response["body"]) if response["body"] else None

    def proxy_name(self, proxy):
        return isinstance(proxy, str) and re.fullmatch(
            "meguri-edge-" + self.owner + r"-[0-9a-f]{32}", proxy) is not None

    def validate_route(self, target, route, expected_worker, proxies=None, allow_missing=False):
        host, port = target_address(target, self.faults)
        if (not isinstance(route, dict) or set(route) != {"proxy", "worker"}
                or not isinstance(expected_worker, str)
                or not re.fullmatch(r"[0-9a-f]{32}", expected_worker)
                or expected_worker == self.owner or route.get("worker") != expected_worker
                or not self.proxy_name(route.get("proxy"))):
            raise EdgeError("route identity refused")
        # The trusted checkout identity is tested BEFORE querying or mutating
        # any proxy, so B's complete consistent tuple cannot substitute for A.
        workers = self.workers()
        if expected_worker not in workers:
            raise EdgeError("retained worker missing")
        worker = workers[expected_worker]
        if not worker["State"]["Running"]:
            raise EdgeError("retained namespace stopped")
        proxies = self.api("GET") if proxies is None else proxies
        proxy = proxies.get(route["proxy"])
        if proxy is None and allow_missing:
            return host, port
        if not isinstance(proxy, dict) or proxy.get("name") != route["proxy"]:
            raise EdgeError("named proxy missing")
        listen = re.fullmatch(r"(0\.0\.0\.0|" + re.escape(host) + r"):([0-9]+)", proxy.get("listen", ""))
        if not listen or int(listen[2]) != port:
            raise EdgeError("proxy listen mismatch")
        upstream = re.fullmatch(r"([a-zA-Z0-9_.-]+):([0-9]+)", proxy.get("upstream", ""))
        if not upstream or not 0 < int(upstream[2]) <= 65535:
            raise EdgeError("invalid backend")
        # Only the retained worker's unique name, id, hostname or private
        # address. Compose's shared service alias 'oracle' is never trusted.
        network = worker["NetworkSettings"]["Networks"].get(self.project + "_oracle-internal")
        if not network or not network.get("IPAddress"):
            raise EdgeError("retained worker network missing")
        ipaddress.ip_address(network["IPAddress"])
        names = {network["IPAddress"], worker["Id"], worker["Id"][:12],
                 worker["Name"].lstrip("/"), worker["Config"]["Hostname"]}
        if upstream[1] not in names:
            raise EdgeError("backend namespace mismatch")
        return host, port

    def save(self, token, record):
        self.state.mkdir(mode=0o700, parents=True, exist_ok=True)
        temporary = self.state / (token + "." + uuid.uuid4().hex + ".tmp")
        with temporary.open("x", encoding="utf-8") as stream:
            os.chmod(temporary, 0o600)
            json.dump(record, stream)
            stream.flush()
            os.fsync(stream.fileno())
        temporary.replace(self.state / (token + ".json"))

    def remove_edges(self, token=None):
        if token is not None:
            self.owned_token(token)
        workers = self.workers()
        ids = self.docker("ps", "-aq", *self.filters(token)).split()
        networks = self.docker("network", "ls", "-q", *self.filters(token)).split()
        # Match all ownership fields and the exact generated name before any
        # mutation. A run/owner label alone never authorizes deleting a route.
        for kind, names in (("container", ids), ("network", networks)):
            for name in names:
                state = self.inspect(name) if kind == "container" else json.loads(
                    self.docker("network", "inspect", name))[0]
                labels = state["Config"]["Labels"] if kind == "container" else state["Labels"]
                lease = labels.get("meguri.mg.edge-lease", "")
                self.owned_token(lease)
                if (any(labels.get(k) != v for k, v in self.labels.items())
                        or (token is not None and lease != token)
                        or labels.get(WORKER) not in workers or not self.proxy_name(labels.get(PROXY))
                        or state["Name"].lstrip("/") != "meguri-conf-edge-" + lease.replace(".", "-")):
                    raise EdgeError("edge resource ownership refused")
        if ids:
            self.docker("rm", "-f", *ids)
        # Absence after rm proves all publications and pumps/connections gone.
        # A failed/ambiguous inspection is an error, never evidence of removal.
        if self.docker("ps", "-aq", *self.filters(token)).split():
            raise EdgeError("edge removal unproved")
        if networks:
            self.docker("network", "rm", *networks)
        if self.docker("network", "ls", "-q", *self.filters(token)).split():
            raise EdgeError("edge network removal unproved")

    def remove(self, token=None):
        if token is None:
            return self.sweep()
        with self.admission().hold(self.deadline):
            return self._remove(token)

    def _remove(self, token):
        # Caller holds owner admission through the complete release.
        self.owned_token(token)
        self.admission().require_certain()
        # Per-lease lock only: independent workers retain full concurrency.
        self.state.mkdir(mode=0o700, parents=True, exist_ok=True)
        with (self.state / (token + ".lock")).open("a") as lock:
            with admission.locked(lock, True, self.deadline):
                return self._remove_locked(token)

    def _remove_locked(self, token):
        path = self.state / (token + ".json")
        if not path.is_file():
            raise EdgeError("unknown lease")
        record = json.loads(path.read_text())
        if record.get("released"):
            return
        route, target = record["route"], record["target"]
        self.validate_route(target, route, record["expected_worker"], allow_missing=True)
        self.remove_edges(token)
        # All edges for this exact proxy must be gone, including pre-token
        # publications. A proxy can never be shared by two live leases.
        if self.docker("ps", "-aq", *self.filters(),
                       "--filter", "label=" + PROXY + "=" + route["proxy"]).split():
            raise EdgeError("proxy still published")
        self.api("DELETE", route["proxy"])
        if route["proxy"] in self.api("GET"):
            raise EdgeError("proxy deletion unproved")
        record["released"] = True
        self.save(token, record)

    def sweep(self):
        # Publish closure BEFORE waiting for active shared publishers. On
        # timeout/interruption the fence stays closed and resources reserved.
        with self.admission().hold(self.deadline, exclusive=True):
            return self._sweep()

    def _sweep(self):
        # Edges first, even if begin/acquire/POST replies never reached us.
        self.remove_edges()
        workers = self.workers()
        proxies = self.api("GET")
        for name, proxy in proxies.items():
            if not self.proxy_name(name):
                continue
            listen = proxy.get("listen", "").rsplit(":", 1)[-1]
            target = "http://" + self.faults.split(":")[0] + ":" + listen + "/"
            owners = []
            for token in workers:
                route = {"proxy": name, "worker": token}
                try:
                    self.validate_route(target, route, token, proxies)
                    owners.append(token)
                except EdgeError:
                    pass
            if len(owners) != 1:
                raise EdgeError("pre-token proxy ownership unproved")
            self.api("DELETE", name)
        # Confirm exact proxy absence before retiring any retained namespace.
        if any(self.proxy_name(name) for name in self.api("GET")):
            raise EdgeError("proxy removal unproved")
        ids = [worker["Id"] for worker in workers.values()]
        if ids:
            self.docker("rm", "-f", *ids)
        if self.workers():
            raise EdgeError("worker removal could not be confirmed")
        if self.state.exists():
            for path in self.state.glob("*.json"):
                record = json.loads(path.read_text())
                record["released"] = True
                self.save(path.stem, record)

    def acquire(self, target, route, expected_worker):
        with self.admission().hold(self.deadline, publishing=True):
            return self._acquire(target, route, expected_worker)

    def _acquire(self, target, route, expected_worker):
        host, port = self.validate_route(target, route, expected_worker)
        # Reject a second publication for a route, including a lost response.
        if self.docker("ps", "-aq", *self.filters(),
                       "--filter", "label=" + PROXY + "=" + route["proxy"]).split():
            raise EdgeError("route already published")
        token = self.scope + "." + uuid.uuid4().hex
        name = "meguri-conf-edge-" + token.replace('.', '-')
        self.save(token, {"route": route, "target": target, "expected_worker": expected_worker, "released": False})
        try:
            self.admission().require_open()
            labels = self.label_args(token, route)
            self.docker("network", "create", *labels, "--opt",
                        "com.docker.network.bridge.enable_ip_masquerade=false", name)
            # Create first, attach both networks, then start the immutable
            # route. Names and all ownership labels exist before start/token.
            self.admission().require_open()
            self.docker("create", "--name", name, *labels, "--init",
                        "--network", self.project + "_oracle-internal",
                        "--publish", "127.0.0.1::8080", "--read-only", "--cap-drop", "ALL",
                        "--security-opt", "no-new-privileges", "--env", "LD_PRELOAD=",
                        "--mount", f"type=bind,src={self.root}/tools/mg.d/conformance-forward.py,dst=/opt/forward.py,readonly",
                        "--mount", f"type=bind,src={self.root}/docker/sync-edge/forward.py,dst=/opt/rig_forward.py,readonly",
                        "--entrypoint", "python", self.image, "-B", "/opt/forward.py", host, str(port))
            self.admission().require_open()
            self.docker("network", "connect", name, name)
            self.admission().require_open()
            self.docker("start", name)
            # Inspect actual listening state, never connect to the edge or
            # fault proxy. Poll startup once under the fixed operation budget.
            while self.docker("exec", name, "python", "-c", LISTENER, missing=True) is None:
                state = json.loads(self.docker("inspect", name))[0]
                if not state["State"]["Running"]:
                    raise EdgeError("edge listener exited")
                time.sleep(0.05)
            state = json.loads(self.docker("inspect", name))[0]
            mapping = state["NetworkSettings"]["Ports"]
            if set(mapping) != {"8080/tcp"} or len(mapping["8080/tcp"]) != 1:
                raise EdgeError("invalid edge publication")
            published = mapping["8080/tcp"][0]
            if published["HostIp"] != "127.0.0.1" or not re.fullmatch(r"[0-9]+", published["HostPort"]):
                raise EdgeError("invalid edge publication")
            selected = int(published["HostPort"])
            if not 0 < selected <= 65535:
                raise EdgeError("invalid edge port")
            self.admission().require_open()
            return {"endpoint": f"http://127.0.0.1:{selected}/", "lease": token}
        except BaseException:
            # Fresh cleanup budget, including interrupt before returning token.
            self.deadline = time.monotonic() + 8
            self._remove(token)
            raise

    def request(self, request):
        if not isinstance(request, dict):
            raise EdgeError("invalid request")
        if set(request) == {"cmd", "target", "route", "expected_worker"} and request["cmd"] == "acquire":
            if not isinstance(request["target"], str) or not request["target"]:
                raise EdgeError("invalid target")
            return self.acquire(request["target"], request["route"], request["expected_worker"])
        if set(request) == {"cmd", "lease"} and request["cmd"] == "release":
            if not isinstance(request["lease"], str) or not request["lease"]:
                raise EdgeError("invalid lease")
            self.remove(request["lease"])
            return {"released": True}
        raise EdgeError("invalid request members")


def interrupted(_signal, _frame):
    raise EdgeError("edge operation interrupted")


def main():
    parser = argparse.ArgumentParser()
    for arg in ("root", "project", "run", "image", "faults", "owner"):
        parser.add_argument("--" + arg, required=True)
    parser.add_argument("--cleanup", action="store_true")
    args = parser.parse_args()
    for sig in (signal.SIGTERM, signal.SIGINT, signal.SIGHUP, signal.SIGALRM):
        signal.signal(sig, interrupted)
    signal.alarm(29)
    edges = Edges(args.root, args.project, args.run, args.image, args.faults, args.owner)
    try:
        if args.cleanup:
            edges.remove()
        else:
            data = sys.stdin.buffer.read(16385)
            if len(data) > 16384:
                raise EdgeError("request too large")
            request = json.loads(data, object_pairs_hook=strict_object)
            response = edges.request(request)
            try:
                print(json.dumps(response, separators=(",", ":")), flush=True)
            except BaseException:
                if request["cmd"] == "acquire":
                    edges.deadline = time.monotonic() + 8
                    edges.remove(response["lease"])
                raise
    except BaseException:
        print("conformance edge unavailable", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
