# SPDX-License-Identifier: MIT
"""Finite output binding for the owned Desktop candidate invocation.

This checks finite counts, identities and retained bytes. The pinned test tool's
separate validate invocation owns artifact JSON semantics. This helper never
executes a fixture, chooses a target, or grants release/broad-catalog acceptance.
"""
import hashlib
import io
import json
import os
import http.client
import shutil
import pathlib
import re
import stat
import sys
import tarfile


SHA256 = re.compile(r"[0-9a-f]{64}\Z")
COUNT_KEYS = ("total", "passed", "failed", "expected_case_count",
              "selected_case_count", "completed_case_count")
INVOCATION_KEYS = {"source_commit", "batch_base", "source_sha256", "lock_sha256",
                   "executable_sha256", "mg_job", "arguments", "run_id", "author_session"}
FINAL_COMPARISON_KEYS = {"root", "server_arguments", "data_volume", "owner_nonce"}
FINAL_SEGMENTS = ("final-full", "final-normal", "final-media", "final-migration",
                  "final-replacement-upload", "final-replacement-download",
                  "final-metadata-only", "final-fresh-fresh", "final-rich")


def require(condition, message):
    if not condition:
        raise ValueError(message)


def regular(path):
    require(stat.S_ISREG(path.lstat().st_mode), "missing regular retained file")
    return path.read_bytes()


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, "duplicate JSON key")
        result[key] = value
    return result


def load(path):
    return json.loads(regular(path), object_pairs_hook=unique_object,
                      parse_constant=lambda _: require(False, "nonfinite JSON value"))


def count(value):
    return type(value) is int and 0 <= value <= 2147483647


def same(left, right):
    # Preserve JSON boolean/numeric distinctions and array order.
    return json.dumps(left, sort_keys=True) == json.dumps(right, sort_keys=True)


def observations(items):
    require(isinstance(items, list) and items, "missing observations")
    result = {}
    for item in items:
        require(set(item) == {"name", "value"} and isinstance(item["name"], str)
                and item["name"] and item["name"] not in result,
                "invalid or duplicate observation name")
        result[item["name"]] = item["value"]
    return result


PACKAGE_KEYS = {"image_id", "inputs_sha256", "runtime_inventory_sha256",
                "image_inspection_sha256", "assets_sha256"}
PEER_KEYS = {"format", "kind", "server", "source_dirty", "profile", "features",
             "executable_artifact_sha256", "lock_artifact_sha256",
             "mg_build_receipt_sha256", "build_invocation", "execution_binding_sha256", "package"}
RUNTIME_BASE = "usr/share/meguri-sync/"
EXECUTABLE = "usr/local/bin/meguri-sync-server"


def digest(data):
    return hashlib.sha256(data).hexdigest()


def artifact(directory, sha256):
    require(isinstance(sha256, str) and SHA256.fullmatch(sha256), "invalid package artifact digest")
    data = regular(directory / sha256)
    require(digest(data) == sha256, "mismatched package artifact bytes")
    return data


def decode(data):
    return json.loads(data, object_pairs_hook=unique_object,
                      parse_constant=lambda _: require(False, "nonfinite JSON value"))


def asset_file(path):
    """Read actual single-link bytes, without following a substituted link."""
    with os.fdopen(os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK), "rb") as source:
        before = os.fstat(source.fileno())
        require(stat.S_ISREG(before.st_mode) and before.st_nlink == 1,
                "frontend asset must be a regular single-link file")
        data = source.read()
        after, bound = os.fstat(source.fileno()), path.lstat()
        require((before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns,
                 before.st_ctime_ns, before.st_nlink) ==
                (after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns,
                 after.st_ctime_ns, after.st_nlink)
                and (bound.st_dev, bound.st_ino, bound.st_mode, bound.st_nlink) ==
                    (after.st_dev, after.st_ino, after.st_mode, 1),
                "frontend asset changed during read")
    return data


def asset_rows(data):
    """The original embed.mjs path grammar and slash-to-underscore mapping."""
    rows = decode(data)
    require(isinstance(rows, list) and rows, "missing compiled frontend manifest")
    mapped, paths = {}, []
    for row in rows:
        require(isinstance(row, dict) and set(row) == {"path", "sha256", "bytes"},
                "invalid frontend asset row")
        path = row["path"]
        require(isinstance(path, str) and (path == "/index.html" or re.fullmatch(
            r"/assets/[A-Za-z0-9_.-]+-[A-Za-z0-9_-]{8,}\.(js|css|png|woff2)", path)),
            "invalid original frontend asset path")
        name = path[1:].replace("/", "_")
        require(name not in mapped, "duplicate or ambiguous frontend asset mapping")
        require(isinstance(row["sha256"], str) and SHA256.fullmatch(row["sha256"])
                and type(row["bytes"]) is int and row["bytes"] > 0,
                "invalid frontend asset identity or length")
        mapped[name] = row
        paths.append(path)
    require(paths[0] == "/index.html" and paths[1:] == sorted(paths[1:])
            and any(path.endswith(".js") for path in paths),
            "incomplete or unordered compiled frontend manifest")
    return mapped


def asset_payload(row, data):
    require(len(data) == row["bytes"] and digest(data) == row["sha256"],
            "frontend asset payload identity or length mismatch")


def export_package_assets(directory):
    """Admit only the two fixed docker-cp archives; never extract host paths."""
    product = directory / "product"
    require(stat.S_ISDIR(product.lstat().st_mode), "missing actual runtime product")
    manifest = asset_file(product / "assets.json")
    rows = asset_rows(manifest)
    payloads = {}
    for filename, expected in (("prepared-manifest.tar", {"assets.json"}),
                               ("prepared-embedded.tar", {"embedded/" + name for name in rows})):
        seen, files = set(), {}
        with tarfile.open(fileobj=io.BytesIO(asset_file(directory / filename))) as archive:
            for entry in archive:
                name = entry.name[2:] if entry.name.startswith("./") else entry.name
                if entry.isdir():
                    name = name.rstrip("/")
                require(name not in seen, "duplicate compiled frontend export path")
                seen.add(name)
                if filename == "prepared-embedded.tar" and name == "embedded" and entry.isdir():
                    continue
                require(name in expected and entry.isfile() and not entry.linkname
                        and entry.sparse is None, "unsafe or unexpected compiled frontend export")
                data = archive.extractfile(entry).read()
                require(len(data) == entry.size, "truncated compiled frontend export")
                files[name] = data
        require(set(files) == expected, "missing compiled frontend export payload")
        if filename == "prepared-manifest.tar":
            require(files["assets.json"] == manifest, "smoke/runtime frontend manifest disagreement")
        else:
            payloads = {name: files["embedded/" + name] for name in rows}
    for name, row in rows.items():
        asset_payload(row, payloads[name])
    # Separate tooling evidence: this does not extend the runtime inventory.
    retained = product / "frontend-assets"
    retained.mkdir()
    for name, data in payloads.items():
        with (retained / name).open("xb") as output:
            output.write(data)
        asset_payload(rows[name], asset_file(retained / name))


def source_entries(data):
    result = {}
    with tarfile.open(fileobj=io.BytesIO(data)) as archive:
        for entry in archive:
            name = entry.name
            require(not pathlib.PurePosixPath(name).is_absolute()
                    and ".." not in pathlib.PurePosixPath(name).parts, "invalid package source path")
            if entry.isdir():
                continue
            require(entry.isfile() and name not in result, "nonregular or duplicate package source")
            result[name] = archive.extractfile(entry).read()
    return result


def package_artifacts(package, server, receipt, directory):
    """Rehash the actual dependency closure, never a summary-only identity."""
    require(set(package) == PACKAGE_KEYS, "invalid package identity keys")
    details = receipt["details"]["package"]
    require(receipt["job"] == "sync-server-package" and receipt["status"] == "pass"
            and receipt["dry_run"] is False and receipt["args"] == []
            and same(receipt["commit"], {"sha": server["source_commit"], "dirty": False})
            and details["source_commit"] == server["source_commit"]
            and details["features"] == [] and details["profile"] == "release",
            "incompatible final package producer")
    require(re.fullmatch(r"sha256:[0-9a-f]{64}", package["image_id"])
            and all(details[key] == package[key] for key in
                    ("image_id", "inputs_sha256", "runtime_inventory_sha256")),
            "package differs from canonical receipt")
    inputs = decode(artifact(directory, package["inputs_sha256"]))
    inventory = decode(artifact(directory, package["runtime_inventory_sha256"]))
    image = decode(artifact(directory, package["image_inspection_sha256"]))
    require(len(image) == 1 and image[0]["Id"] == package["image_id"]
            and image[0]["Config"]["User"] == "10001:10001"
            and image[0]["Config"]["Entrypoint"] == ["/" + EXECUTABLE]
            and image[0]["Config"]["Labels"]["org.opencontainers.image.revision"] == server["source_commit"],
            "incompatible actual package image")
    require(inputs["source_commit"] == server["source_commit"] == server["build"]
            and inputs["features"] == [] and inputs["profile"] == "release"
            and inputs["source_archive_sha256"] == server["source_sha256"],
            "package source differs from peer")
    source = source_entries(artifact(directory, server["source_sha256"]))
    rows = inputs["inputs"]
    require(len(rows) == len(source) and len({row["path"] for row in rows}) == len(rows),
            "incomplete package source inputs")
    for row in rows:
        data = source[row["path"]]
        require(type(row["bytes"]) is int and len(data) == row["bytes"]
                and digest(data) == row["sha256"], "package source input mismatch")
    require(digest(source["server/Cargo.lock"]) == server["lock_sha256"]
            and regular(directory / server["lock_sha256"]) == source["server/Cargo.lock"],
            "package lock differs from frozen source")
    version = re.search(rb'^version\s*=\s*"([^"]+)"', source["server/Cargo.toml"], re.MULTILINE)
    require(version is not None and version[1].decode() == server["version"]
            and server["kind"] == "meguri_sync", "package server identity differs from source")
    for metadata in inventory.values():
        if "sha256" in metadata:
            data = artifact(directory, metadata["sha256"])
            require(type(metadata["bytes"]) is int and len(data) == metadata["bytes"],
                    "runtime inventory byte count mismatch")
    require(inventory[EXECUTABLE]["sha256"] == server["executable_sha256"]
            and inventory[RUNTIME_BASE + "assets.json"]["sha256"] == package["assets_sha256"]
            and inventory[RUNTIME_BASE + "package-inputs.json"]["sha256"] == package["inputs_sha256"]
            and inventory[RUNTIME_BASE + "covered-source.tar"]["sha256"] == inputs["covered_source_sha256"]
            and inventory[RUNTIME_BASE + "rust-notices.json"]["sha256"] == inputs["rust_notices_sha256"],
            "runtime package identity mismatch")
    for row in asset_rows(artifact(directory, package["assets_sha256"])).values():
        data = asset_file(directory / row["sha256"])
        asset_payload(row, data)


def final_peer(peer, directory, invocation, *, preflight=False):
    run = invocation["run_id"]
    require(FINAL_COMPARISON_KEYS <= set(invocation)
            and isinstance(invocation["root"], str) and invocation["root"].startswith("/")
            and isinstance(invocation["data_volume"], str) and invocation["data_volume"]
            and isinstance(invocation["owner_nonce"], str)
            and re.fullmatch(r"[0-9a-f]{32}", invocation["owner_nonce"])
            and isinstance(invocation["server_arguments"], list)
            and len(invocation["server_arguments"]) == 6
            and invocation["server_arguments"][:2] == ["serve", "/data/owner"]
            and isinstance(invocation["server_arguments"][2], str)
            and invocation["server_arguments"][2].startswith("https://")
            and invocation["server_arguments"][3:5] == ["0.0.0.0:8080", "--trusted-peer"]
            and isinstance(invocation["server_arguments"][5], str)
            and invocation["server_arguments"][5], "missing trusted final comparison inputs")
    require(directory.is_dir() and not directory.is_symlink(), "missing retained package artifact store")
    require(set(peer) == PEER_KEYS and peer["format"] == "meguri-sync-package-v1"
            and peer["kind"] == ("final_package_preflight" if preflight else "final_package")
            and peer["source_dirty"] is False
            and peer["profile"] == "release" and peer["features"] == [], "invalid final package receipt")
    server = peer["server"]
    require(set(server) == {"kind", "version", "build", "source_commit", "source_sha256", "lock_sha256", "executable_sha256"}
            and re.fullmatch(r"[0-9a-f]{40}", server["source_commit"])
            and all(SHA256.fullmatch(server[key]) for key in
                    ("source_sha256", "lock_sha256", "executable_sha256")), "invalid final server identity")
    require(peer["executable_artifact_sha256"] == server["executable_sha256"]
            and peer["lock_artifact_sha256"] == server["lock_sha256"], "final peer artifact mismatch")
    require(all(same(invocation[key], server[key]) for key in
                ("source_commit", "source_sha256", "lock_sha256", "executable_sha256")),
            "final peer differs from trusted source or executable")
    receipt = decode(artifact(directory, peer["mg_build_receipt_sha256"]))
    require(receipt["root"] == invocation["root"] and receipt["exit_code"] == 0
            and type(receipt["exit_code"]) is int and receipt["failures"] == [],
            "package producer root or result mismatch")
    require(same(peer["build_invocation"], {"job": receipt["job"], "run_id": receipt["run_id"],
                                          "arguments": receipt["args"]}), "package invocation mismatch")
    package_artifacts(peer["package"], server, receipt, directory)
    binding = decode(artifact(directory, peer["execution_binding_sha256"]))
    binding_keys = {"format", "endpoint", "server", "run_id", "container_id", "labels",
                             "observed_executable_sha256", "package", "server_process",
                             "container_inspection_before_sha256"}
    if not preflight:
        binding_keys.add("container_inspection_after_sha256")
    require(set(binding) == binding_keys
            and binding["format"] == "meguri-sync-candidate-execution-v1"
            and binding["endpoint"] == "http://127.0.0.1:8080/"
            and same(binding["server"], server) and same(binding["package"], peer["package"])
            and binding["run_id"] == run and binding["labels"]["meguri.mg.run-id"] == run,
            "invalid final execution binding")
    process = binding["server_process"]
    require(set(process) == {"pid", "executable_sha256"} and type(process["pid"]) is int
            and 1 <= process["pid"] <= 2147483647
            and process["executable_sha256"] == binding["observed_executable_sha256"] == server["executable_sha256"],
            "unmeasured final server process")
    inspections = [decode(artifact(directory, binding[key])) for key in
                   ("container_inspection_before_sha256",) +
                   (() if preflight else ("container_inspection_after_sha256",))]
    require(all(isinstance(items, list) and len(items) == 1 for items in inspections),
            "invalid final container inspections")
    for items in inspections:
        inspection = items[0]
        config, host = inspection["Config"], inspection["HostConfig"]
        require(inspection["Id"] == binding["container_id"]
                and re.fullmatch(r"[0-9a-f]{64}", inspection["Id"])
                and inspection["Name"] == "/meguri-local-" + run
                and inspection["Image"] == peer["package"]["image_id"] and inspection["State"]["Running"]
                and config["User"] == "10001:10001" and config["Entrypoint"] == ["/" + EXECUTABLE]
                and config["Cmd"] == invocation["server_arguments"]
                and config["Labels"]["meguri.mg.run-id"] == run
                and config["Labels"]["meguri.sync.owner"] == invocation["owner_nonce"]
                and inspection["State"]["Running"] is True
                and host["ReadonlyRootfs"] is True and host["NetworkMode"] == "none"
                and not host["PortBindings"] and host["CapDrop"] == ["ALL"]
                and not host.get("CapAdd") and not host["Privileged"]
                and "no-new-privileges:true" in host["SecurityOpt"], "incompatible owned final container")
        mounts = [mount for mount in inspection["Mounts"] if mount["Destination"] != "/tmp"]
        require(len(mounts) == 1 and mounts[0]["Destination"] == "/data"
                and mounts[0]["Type"] == "volume" and mounts[0]["Name"] == invocation["data_volume"]
                and mounts[0]["RW"] is True, "incompatible owned final data mount")
    if not preflight:
        before, after = (items[0] for items in inspections)
        require(all(same(before[key], after[key]) for key in
                    ("Created", "Config", "Mounts", "HostConfig"))
                and before["State"]["StartedAt"] == after["State"]["StartedAt"],
                "owned server changed during peer execution")
    return binding


def final_invocation(invocation, stage):
    keys = INVOCATION_KEYS | FINAL_COMPARISON_KEYS | {"child_phase", "child_stage"}
    if stage == "record":
        keys.add("execute_admission_sha256")
    require(set(invocation) == keys and invocation["child_phase"] in FINAL_SEGMENTS
            and invocation["child_stage"] == stage, "invalid final child invocation")


def final_selection(phase):
    root = pathlib.Path(__file__).resolve().parents[2] / "conformance/recordings/scenarios/meguri-sync-server"
    original = [load(root / ("candidate-" + name + "-selection.json"))
                for name in ("full", "normal", "media")]
    additions = load(root / "candidate-final-additions.json")
    groups = [selection["cases"] for selection in original] + [additions["cases"]]
    require([len(cases) for cases in groups] == [11, 10, 4, 6]
            and len({case["id"] for cases in groups for case in cases}) == 31,
            "final expansion must preserve all 31 distinct cases")
    if phase == "final":
        cases = [case for selection in original for case in selection["cases"]] + additions["cases"]
    else:
        require(phase in FINAL_SEGMENTS, "invalid final segment selection")
        index = FINAL_SEGMENTS.index(phase)
        cases = original[index]["cases"] if index < 3 else [additions["cases"][index - 3]]
    return dict(format="meguri-sync-selection-v1", lane_kind="candidate", phase=phase, cases=cases)


def execute_admission(output, expected, preflight_data):
    """Admit retained execute bytes without granting any compatibility credit."""
    final_invocation(expected, "execute")
    store = output / "artifacts"
    peer = decode(preflight_data)
    final_peer(peer, store, expected, preflight=True)
    admission_data = regular(output / "execute-admission.json")
    admission = decode(admission_data)
    require(set(admission) == {"format", "child_phase", "invocation_sha256",
                               "preflight_peer_sha256", "observations_sha256"}
            and admission["format"] == "meguri-sync-final-execute-v1"
            and admission["child_phase"] == expected["child_phase"], "invalid execute admission")
    invocation_data = regular(output / "invocation.json")
    invocation = decode(invocation_data)
    require(set(invocation) == set(expected) | {"selection"}
            and all(same(invocation[key], expected[key]) for key in expected)
            and same(invocation["selection"], [case["id"] for case in
                                               final_selection(expected["child_phase"])["cases"]]),
            "execute invocation differs from trusted inputs")
    require(artifact(store, admission["invocation_sha256"]) == invocation_data
            and artifact(store, admission["preflight_peer_sha256"]) == preflight_data
            and artifact(store, admission["observations_sha256"]) ==
            regular(output / "candidate-observations.json"), "execute admission bytes mismatch")
    return digest(admission_data)


def raw_identity(path):
    metadata = path.lstat()
    require(stat.S_ISREG(metadata.st_mode) and metadata.st_nlink == 1,
            "observations must remain regular single-link bytes")
    return [metadata.st_dev, metadata.st_ino, metadata.st_uid]


def execute_snapshot(output, expected, preflight_data):
    admission_sha256 = execute_admission(output, expected, preflight_data)
    require(artifact(output / "artifacts", admission_sha256) ==
            regular(output / "execute-admission.json"), "missing retained execute admission")
    require(not any((output / name).exists() or (output / name).is_symlink()
                    for name in ("evidence.json", "summary.json", "selection.json", "fixture.json", "fixture-manifest.json")),
            "execute must grant zero final credit")
    return dict(admission_sha256=admission_sha256,
                raw_identity=raw_identity(output / "candidate-observations.json"))


def record_handoff(output, expected, peer, snapshot):
    """Resolve the original execute bytes in the SAME output's immutable CAS."""
    final_invocation(expected, "record")
    require(snapshot["admission_sha256"] == expected["execute_admission_sha256"],
            "record admission differs from captured execute")
    store = output / "artifacts"
    admission_data = artifact(store, expected["execute_admission_sha256"])
    require(admission_data == regular(output / "execute-admission.json"),
            "record changed execute admission")
    admission = decode(admission_data)
    require(set(admission) == {"format", "child_phase", "invocation_sha256",
                               "preflight_peer_sha256", "observations_sha256"}
            and admission["format"] == "meguri-sync-final-execute-v1"
            and admission["child_phase"] == expected["child_phase"], "invalid execute admission")
    original = decode(artifact(store, admission["invocation_sha256"]))
    execute_expected = {key: value for key, value in expected.items()
                        if key != "execute_admission_sha256"}
    execute_expected["child_stage"] = "execute"
    require(set(original) == set(execute_expected) | {"selection"}
            and all(same(original[key], execute_expected[key]) for key in execute_expected)
            and same(original["selection"], [case["id"] for case in
                                            final_selection(expected["child_phase"])["cases"]]),
            "record changed original execute invocation")
    preflight = decode(artifact(store, admission["preflight_peer_sha256"]))
    before = final_peer(preflight, store, execute_expected, preflight=True)
    binding = final_peer(peer, store, expected)
    require(same({key: value for key, value in binding.items()
                  if key != "container_inspection_after_sha256"}, before),
            "record changed preflight execution binding")
    require(same({key: value for key, value in peer.items()
                  if key not in {"kind", "execution_binding_sha256"}},
                 {key: value for key, value in preflight.items()
                  if key not in {"kind", "execution_binding_sha256"}}),
            "record changed preflight package identity")
    raw = output / "candidate-observations.json"
    require(raw_identity(raw) == snapshot["raw_identity"]
            and regular(raw) == artifact(store, admission["observations_sha256"]),
            "record changed execute observations")
    invocation = load(output / "invocation.json")
    require(set(invocation) == set(expected) | {"selection"}
            and all(same(invocation[key], expected[key]) for key in expected)
            and same(invocation["selection"], original["selection"]),
            "record changed invocation or selection")


def retain(directory, data):
    sha256 = digest(data)
    target = directory / sha256
    if target.exists() or target.is_symlink():
        require(regular(target) == data, "occupied retained artifact")
    else:
        with target.open("xb") as output:
            output.write(data)
    return sha256


def write_json(path, value):
    with path.open("x") as output:
        json.dump(value, output, sort_keys=True)
        output.write("\n")


def prepare_final(input_dir, package_store, identity, receipt_data, before_data, process, expected):
    final_invocation(expected, "execute")
    require(not any(input_dir.iterdir()), "preflight input must start empty")
    shutil.copytree(package_store, input_dir / "artifacts")
    store = input_dir / "artifacts"
    binding = dict(format="meguri-sync-candidate-execution-v1", endpoint="http://127.0.0.1:8080/",
                   server=identity["server"], package=identity["package"], run_id=expected["run_id"],
                   container_id=decode(before_data)[0]["Id"], labels={"meguri.mg.run-id": expected["run_id"]},
                   observed_executable_sha256=process["executable_sha256"], server_process=process,
                   container_inspection_before_sha256=retain(store, before_data))
    receipt = decode(receipt_data)
    peer = dict(format="meguri-sync-package-v1", kind="final_package_preflight", server=identity["server"],
                source_dirty=False, profile="release", features=[], package=identity["package"],
                executable_artifact_sha256=identity["server"]["executable_sha256"],
                lock_artifact_sha256=identity["server"]["lock_sha256"],
                mg_build_receipt_sha256=retain(store, receipt_data),
                build_invocation=dict(job=receipt["job"], run_id=receipt["run_id"], arguments=receipt["args"]),
                execution_binding_sha256=retain(store, json.dumps(binding, sort_keys=True).encode()))
    final_peer(peer, store, expected, preflight=True)
    write_json(input_dir / "preflight.json", peer)


def complete_final(input_dir, execute_input, execute_store, after_data, process, expected):
    final_invocation(expected, "record")
    require(not any(input_dir.iterdir()), "record input must start empty")
    shutil.copytree(execute_store, input_dir / "artifacts")
    store = input_dir / "artifacts"
    preflight = load(execute_input / "preflight.json")
    binding = decode(artifact(store, preflight["execution_binding_sha256"]))
    require(same(binding["server_process"], process), "actual server process changed after execute")
    binding["container_inspection_after_sha256"] = retain(store, after_data)
    peer = dict(preflight, kind="final_package",
                execution_binding_sha256=retain(store, json.dumps(binding, sort_keys=True).encode()))
    final_peer(peer, store, expected)
    write_json(input_dir / "peer.json", peer)


def measure_process(invocation):
    # A separate tool joins ONLY the owned container PID namespace. Docker's
    # init has one direct service child; never assume init is the server.
    expected = ["/" + EXECUTABLE] + invocation["server_arguments"]
    children = pathlib.Path("/proc/1/task/1/children").read_text().split()
    matches = []
    for text in children:
        require(text.isdecimal() and 1 < int(text) <= 2147483647, "invalid owned child PID")
        process = pathlib.Path("/proc") / text
        command = process.joinpath("cmdline").read_bytes().split(b"\0")
        if command == [part.encode() for part in expected] + [b""]:
            matches.append(dict(pid=int(text), executable_sha256=digest(process.joinpath("exe").read_bytes())))
    require(len(matches) == 1 and matches[0]["executable_sha256"] == invocation["executable_sha256"],
            "cannot prove actual owned server process")
    return matches[0]


def setup_account():
    # Readiness is established by the factory's listening event. One POST,
    # with no retry, runs in separate tooling with a readonly setup-data mount.
    credentials = decode(sys.stdin.buffer.read())
    require(set(credentials) == {"username", "password"} and all(
        isinstance(value, str) and value for value in credentials.values()), "invalid synthetic credentials")
    credentials["token"] = pathlib.Path("/data/owner/state/setup-token").read_text().strip()
    connection = http.client.HTTPConnection("127.0.0.1", 8080, timeout=20)
    try:
        connection.request("POST", "/api/setup", body=json.dumps(credentials).encode(), headers={
            "Content-Type": "application/json", "Origin": "https://candidate.example.test"})
        response = connection.getresponse()
        require(response.status == 201, "synthetic account setup failed")
        response.read()
    finally:
        connection.close()


def retain_package(directory, destination):
    """Copy the canonical package's real dependencies into the existing store."""
    require(destination.is_dir() and not destination.is_symlink(), "missing owned artifact store")
    def retain(data):
        sha256 = digest(data)
        target = destination / sha256
        if target.exists():
            require(regular(target) == data, "occupied package artifact")
        else:
            with target.open("xb") as output:
                output.write(data)
        return sha256
    receipt = load(directory / "result.json")
    inputs_data = regular(directory / "context/package-inputs.json")
    inventory_data = regular(directory / "product/runtime-inventory.json")
    inputs, inventory = decode(inputs_data), decode(inventory_data)
    source_data = regular(directory / "source.tar")
    source = source_entries(source_data)
    version = re.search(rb'^version\s*=\s*"([^"]+)"', source["server/Cargo.toml"], re.MULTILINE)
    require(version is not None, "missing frozen package version")
    server = dict(kind="meguri_sync", version=version[1].decode(), build=inputs["source_commit"],
                  source_commit=inputs["source_commit"], source_sha256=retain(source_data),
                  lock_sha256=retain(source["server/Cargo.lock"]),
                  executable_sha256=inventory[EXECUTABLE]["sha256"])
    package = dict(image_id=receipt["details"]["package"]["image_id"],
                   inputs_sha256=retain(inputs_data), runtime_inventory_sha256=retain(inventory_data),
                   image_inspection_sha256=retain(regular(directory / "image.json")),
                   assets_sha256=inventory[RUNTIME_BASE + "assets.json"]["sha256"])
    # Runtime paths never become host paths. Read regular tar members only.
    seen = set()
    with tarfile.open(directory / "runtime.tar") as runtime:
        for entry in runtime:
            name = entry.name[2:] if entry.name.startswith("./") else entry.name
            require(name not in seen, "duplicate runtime export path")
            seen.add(name)
            if entry.isfile():
                data = runtime.extractfile(entry).read()
                metadata = inventory[name]
                require(metadata["bytes"] == len(data) and metadata["sha256"] == retain(data),
                        "runtime export differs from inventory")
    require(all(name in seen for name in inventory), "missing exported runtime dependency")
    product = directory / "product"
    require(stat.S_ISDIR(product.lstat().st_mode), "missing actual runtime product")
    manifest = asset_file(product / "assets.json")
    require(manifest == artifact(destination, package["assets_sha256"]),
            "retained frontend manifest differs from runtime")
    rows = asset_rows(manifest)
    closure = product / "frontend-assets"
    require(closure.exists() and stat.S_ISDIR(closure.lstat().st_mode),
            "missing compiled frontend payload evidence")
    require({path.name for path in closure.iterdir()} == set(rows),
            "incomplete or unexpected compiled frontend payload evidence")
    for name, row in rows.items():
        data = asset_file(closure / name)
        asset_payload(row, data)
        require(retain(data) == row["sha256"], "retained frontend payload digest mismatch")
    retain(regular(directory / "result.json"))
    package_artifacts(package, server, receipt, destination)
    return {"server": server, "package": package}


def validate(output, expected, peer, selection, summary, invocation):
    outer_phase = expected['arguments'][3]
    keys = INVOCATION_KEYS
    if outer_phase == 'final':
        if 'child_phase' in expected:
            keys = keys | FINAL_COMPARISON_KEYS
            final_invocation(expected, 'record')
            keys = keys | {'child_phase', 'child_stage', 'execute_admission_sha256'}
    require(set(expected) == keys, "invalid owned invocation inputs")
    require(set(invocation) == keys | {"selection"}, "invalid final invocation fields")
    require(all(same(invocation[key], expected[key]) for key in keys),
            "stale or mismatched final invocation identity")
    require(all(same(item["executable_sha256"], peer["server"]["executable_sha256"])
                for item in (expected, invocation)), "invocation executable differs from owned candidate")
    require(all(same(item[key], peer["server"][key])
                for item in (expected, invocation)
                for key in ("source_commit", "source_sha256", "lock_sha256")),
            "invocation source or lock differs from owned candidate")
    require(outer_phase in {'full', 'normal', 'media', 'final'}, 'unsupported owned candidate phase')
    phase = expected.get('child_phase', outer_phase)
    required = (len(final_selection(phase)['cases']) if outer_phase == 'final' else
                {'full':11, 'normal':10, 'media':4}[phase])
    if outer_phase == 'final':
        require(expected['arguments'][1] == 'desktop', 'unsupported automated final lane')
        if 'child_phase' in expected:
            final_peer(peer, output / 'artifacts', expected)
        actual_selection = load(output / 'selection.json')
        require(all(same(actual_selection[key], selection[key])
                    for key in ('format', 'lane_kind', 'phase', 'cases')),
                'final selection differs from frozen contract')
    require(selection["format"] == "meguri-sync-selection-v1"
            and selection["lane_kind"] == "candidate" and selection["phase"] == phase,
            "invalid fixed selection")
    contracts = selection["cases"]
    ids = [case["id"] for case in contracts]
    require(len(ids) == required and len(set(ids)) == required, "invalid selected phase expansion")
    selected = invocation["selection"]
    require(isinstance(selected, list) and len(selected) == required
            and len(set(selected)) == required and
            (same(selected, ids) if outer_phase == 'final' else set(selected) == set(ids)),
            "final invocation selection differs from selected phase contract")
    require(all(count(summary[key]) for key in COUNT_KEYS), "invalid actual summary counts")
    require(type(summary["selection_credit"]) is bool
            and type(summary["broad_catalog_credit"]) is bool, "invalid actual credit flags")
    require(summary.get("skipped", 0) == 0 and type(summary.get("skipped", 0)) is int,
            "hidden skipped output")
    if outer_phase == 'final':
        require(all(summary.get(key, 0) == 0 and type(summary.get(key, 0)) is int
                    for key in ("missing", "unavailable")), "hidden missing or unavailable output")
    require(summary["expected_case_count"] == summary["selected_case_count"] == required,
            "missing or zero phase selection")
    evidence = load(output / "evidence.json")
    evidence_keys = {"format", "document_kind", "baseline_release", "source_commit",
                     "source_sha256", "lock_sha256", "batch_base", "mg_job", "arguments",
                     "expected_case_count", "selected_case_count", "completed_case_count", "cases"}
    require(evidence_keys <= set(evidence) <= evidence_keys | {"executable_sha256"},
            "invalid evidence fields")
    if "executable_sha256" in evidence:
        require(same(evidence["executable_sha256"], peer["server"]["executable_sha256"]),
                "evidence executable differs from owned candidate")
    require(evidence["format"] == "meguri-sync-server-v1"
            and evidence["document_kind"] == "evidence" and evidence["baseline_release"] == "26.09.3",
            "invalid evidence format")
    for key in ("source_commit", "source_sha256", "lock_sha256", "batch_base", "mg_job", "arguments"):
        require(same(evidence[key], expected[key]), "stale or mismatched evidence identity")
    cases = evidence["cases"]
    require(isinstance(cases, list) and len(cases) == required
            and len({case["id"] for case in cases}) == required
            and {case["id"] for case in cases} == set(ids), "missing or duplicate output cases")
    if outer_phase == 'final':
        require(same([case["id"] for case in cases], ids), "final case order differs from frozen selection")
    completed = sum(case["status"] in ("matched", "different", "failed", "sealed") for case in cases)
    passed = sum(case["status"] in ("matched", "sealed") for case in cases)
    for key, actual in (("expected_case_count", required), ("selected_case_count", len(cases)),
                        ("completed_case_count", completed)):
        require(count(evidence[key]) and evidence[key] == actual == summary[key],
                "mismatched actual execution counts")
    require(summary["total"] == len(cases) and summary["passed"] == passed
            and summary["failed"] == completed - passed, "mismatched actual outcome totals")
    require(completed == passed == required and summary["failed"] == 0, "incomplete or failed actual execution")
    require(summary["selection_credit"] and not summary["broad_catalog_credit"],
            "invalid finite coverage claim")
    require(not (output / "artifacts").is_symlink()
            and (output / "artifacts").is_dir(), "missing retained artifacts directory")
    contracts = {case["id"]: case for case in contracts}
    client_identity = None
    for case in cases:
        contract = contracts[case["id"]]
        require(case["id"].split(":")[0] == case["family"], "case family differs from ID")
        for key in ("family", "classification", "spec_ids", "variants", "expected"):
            require(same(case[key], contract[key]), "case differs from fixed selected contract")
        # Only the recorder's optional fixture binding is dynamic.
        actions = case["actions"]
        require(isinstance(actions, list) and all(isinstance(action, dict) for action in actions)
                and same([{key: value for key, value in action.items() if key != "fixture_sha256"}
                          for action in actions], contract["actions"]),
                "case differs from fixed selected contract")
        # Intermediate output cannot assert an independently reviewed seal.
        require(case["status"] == "matched", "candidate output has no independent seal")
        lane = case["lane"]
        require(lane["kind"] == "candidate" and same(lane["server"], peer["server"]),
                "case is not bound to this owned candidate")
        client = lane["client"]
        require(client["kind"] == "desktop" and client["version"] == "26.09.4"
                and client["build"] == "c06370ff" and SHA256.fullmatch(client["runtime_sha256"]),
                "missing actual Desktop identity")
        require(client_identity is None or same(client_identity, client), "inconsistent Desktop identity")
        client_identity = client
        expected_values = observations(case["expected"])
        observed = observations(case["observed"])
        require(all(key in observed and same(value, observed[key])
                    for key, value in expected_values.items()), "matched output has mismatched observations")
        roles = set()
        fixtures = set()
        for artifact in case["artifacts"]:
            digest = artifact["sha256"]
            require(isinstance(digest, str) and SHA256.fullmatch(digest), "invalid artifact digest")
            require(type(artifact["bytes"]) is int and artifact["bytes"] >= 0, "invalid artifact byte count")
            data = regular(output / "artifacts" / digest)
            require(len(data) == artifact["bytes"] and hashlib.sha256(data).hexdigest() == digest,
                    "missing or mismatched retained artifact bytes")
            roles.add(artifact["role"])
            if artifact["role"] == "synthetic_input":
                fixtures.add(digest)
        require({"synthetic_input", "neutral_observation", "mg_receipt"} <= roles,
                "missing required case artifact roles")
        require(case["fixture_sha256"] in fixtures, "fixture is not a retained synthetic input")
        require(all(action.get("fixture_sha256", case["fixture_sha256"]) in fixtures
                    for action in case["actions"]), "action fixture is not retained")


def assembly_handoff(segments, output, expected, controls):
    require(set(expected) == INVOCATION_KEYS and expected['arguments'][3] == 'final',
            "invalid outer assembly invocation")
    require(segments.is_dir() and not segments.is_symlink()
            and {path.name for path in segments.iterdir()} == set(FINAL_SEGMENTS),
            "assembly requires exactly nine fixed segments")
    storage, containers, volumes, nonces = set(), set(), set(), set()
    for path in (output, output / 'artifacts'):
        metadata = path.lstat()
        require(stat.S_ISDIR(metadata.st_mode) and metadata.st_uid == os.getuid(),
                "assembly requires its own owned output")
        storage.add((metadata.st_dev, metadata.st_ino))
    combined, origins, client, package = [], [], None, None
    for phase in FINAL_SEGMENTS:
        segment = segments / phase
        for path in (segment, segment / 'artifacts'):
            metadata = path.lstat()
            identity = (metadata.st_dev, metadata.st_ino)
            require(stat.S_ISDIR(metadata.st_mode) and metadata.st_uid == os.getuid()
                    and identity not in storage, "segments must retain distinct owned storage")
            storage.add(identity)
        for name in ('invocation.json', 'selection.json', 'evidence.json', 'summary.json',
                     'candidate-observations.json'):
            regular(segment / name)
        control = controls / phase
        invocation = load(control / 'record-invocation.json')
        require(all(same(invocation[key], expected[key]) for key in INVOCATION_KEYS)
                and invocation['child_phase'] == phase, "segment differs from outer invocation")
        peer = load(control / 'record-input/peer.json')
        record_handoff(segment, invocation, peer, load(control / 'execute-snapshot.json'))
        validate(segment, invocation, peer, final_selection(phase), load(segment / 'summary.json'),
                 load(segment / 'invocation.json'))
        binding = final_peer(peer, segment / 'artifacts', invocation)
        for value, seen in ((binding['container_id'], containers), (invocation['data_volume'], volumes),
                            (invocation['owner_nonce'], nonces)):
            require(value not in seen, "final segments reused a runtime or storage identity")
            seen.add(value)
        require(package is None or same(package, peer['package']), "segments used different package images")
        package = peer['package']
        evidence = load(segment / 'evidence.json')
        for case in evidence['cases']:
            actual = case['lane']['client']
            require(client is None or same(client, actual), "segments used different Desktop runtimes")
            client = actual
        combined.extend(evidence['cases'])
        ids = [case['id'] for case in final_selection(phase)['cases']]
        origins.extend((segment, ids) for case in evidence['cases'])
        for path in (segment / 'artifacts').iterdir():
            require(artifact(output / 'artifacts', path.name) == artifact(segment / 'artifacts', path.name),
                    "assembly lost or changed a segment artifact")
    validate(output, expected, peer, final_selection('final'), load(output / 'summary.json'),
             load(output / 'invocation.json'))
    aggregate = load(output / 'evidence.json')['cases']
    final_ids = [case['id'] for case in final_selection('final')['cases']]
    require(len(aggregate) == len(combined), "assembly changed segment case count")
    for actual, original, (segment, ids) in zip(aggregate, combined, origins):
        if same(actual, original):
            continue
        require(same({key: value for key, value in actual.items() if key != 'artifacts'},
                     {key: value for key, value in original.items() if key != 'artifacts'})
                and len(actual['artifacts']) == len(original['artifacts']),
                "assembly changed segment cases, actions or outcomes")
        for new, old in zip(actual['artifacts'], original['artifacts']):
            if same(new, old):
                continue
            require(new['role'] == old['role'] == 'mg_receipt'
                    and same({key: value for key, value in new.items() if key not in {'sha256', 'bytes'}},
                             {key: value for key, value in old.items() if key not in {'sha256', 'bytes'}}),
                    "assembly changed a segment artifact descriptor")
            old_data = artifact(segment / 'artifacts', old['sha256'])
            new_data = artifact(output / 'artifacts', new['sha256'])
            require(len(old_data) == old['bytes'] and len(new_data) == new['bytes'],
                    "assembly receipt byte count mismatch")
            before, after = decode(old_data), decode(new_data)
            fields = {'selection', 'completed_ids'}
            require(isinstance(before, dict) and isinstance(after, dict)
                    and fields <= set(before) == set(after)
                    and same({key: value for key, value in before.items() if key not in fields},
                             {key: value for key, value in after.items() if key not in fields})
                    and all(same(before[key], ids) and same(after[key], final_ids) for key in fields),
                    "assembly receipt differs from the exact two-field selection derivation")


def main():
    command = sys.argv[1] if len(sys.argv) > 1 else ""
    if command == "--export-package-assets" and len(sys.argv) == 3:
        try:
            export_package_assets(pathlib.Path(sys.argv[2]))
            return 0
        except (OSError, ValueError, KeyError, TypeError, tarfile.TarError) as error:
            sys.stderr.write("compiled frontend export rejected: " + str(error) + "\n")
            return 1
    if command == "--check-assembly" and len(sys.argv) == 6:
        try:
            assembly_handoff(pathlib.Path(sys.argv[2]), pathlib.Path(sys.argv[3]),
                             load(pathlib.Path(sys.argv[4])), pathlib.Path(sys.argv[5]))
            return 0
        except (OSError, ValueError, KeyError, TypeError, AttributeError, tarfile.TarError):
            sys.stderr.write("final assembly is incomplete or mismatched; zero final credit\n")
            return 1
    if command in {"--prepare-final", "--complete-final", "--measure-process", "--setup-account"}:
        try:
            paths = list(map(pathlib.Path, sys.argv[2:]))
            if command == "--prepare-final":
                input_dir, store, identity, receipt, before, process, expected = paths
                prepare_final(input_dir, store, load(identity), regular(receipt), regular(before),
                              load(process), load(expected))
            elif command == "--complete-final":
                input_dir, execute_input, execute_store, after, process, expected = paths
                complete_final(input_dir, execute_input, execute_store, regular(after), load(process), load(expected))
            elif command == "--measure-process":
                print(json.dumps(measure_process(load(paths[0])), sort_keys=True))
            else:
                setup_account()
            return 0
        except (OSError, ValueError, KeyError, TypeError, AttributeError, tarfile.TarError,
                http.client.HTTPException):
            sys.stderr.write("owned final preparation or measurement failed; zero final credit\n")
            return 1
    if len(sys.argv) == 6 and sys.argv[1] == "--admit-execute":
        try:
            snapshot = execute_snapshot(pathlib.Path(sys.argv[2]), load(pathlib.Path(sys.argv[3])),
                                        regular(pathlib.Path(sys.argv[4])))
            write_json(pathlib.Path(sys.argv[5]), snapshot)
            print(snapshot["admission_sha256"])
            return 0
        except (OSError, ValueError, KeyError, TypeError, AttributeError, tarfile.TarError):
            sys.stderr.write("execute admission is incomplete or mismatched; zero final credit\n")
            return 1
    if len(sys.argv) == 6 and sys.argv[1] == "--check-record":
        try:
            record_handoff(pathlib.Path(sys.argv[2]), load(pathlib.Path(sys.argv[3])),
                           load(pathlib.Path(sys.argv[4])), load(pathlib.Path(sys.argv[5])))
            return 0
        except (OSError, ValueError, KeyError, TypeError, AttributeError, tarfile.TarError):
            sys.stderr.write("record handoff is incomplete or mismatched\n")
            return 1
    if len(sys.argv) == 4 and sys.argv[1] == "--retain-package":
        try:
            print(json.dumps(retain_package(pathlib.Path(sys.argv[2]), pathlib.Path(sys.argv[3])), sort_keys=True))
            return 0
        except (OSError, ValueError, KeyError, TypeError, tarfile.TarError) as error:
            sys.stderr.write("final package artifacts are incomplete or mismatched: " + str(error) + "\n")
            return 1
    output, expected_path, peer_path, selection_path = map(pathlib.Path, sys.argv[1:])
    report = {"summary": None, "invocation": None, "errors": []}
    # Keep independently parsed actual outputs available even on validation failure.
    for field, filename in (("summary", "summary.json"), ("invocation", "invocation.json")):
        try:
            value = load(output / filename)
            require(isinstance(value, dict), "runtime output must be a JSON object")
            report[field] = value
        except (OSError, ValueError, TypeError):
            report["errors"].append("missing or invalid " + filename)
    if not report["errors"]:
        try:
            selection = load(selection_path)
            expected = load(expected_path)
            if expected["arguments"][3] == "final":
                require(selection_path.name == "candidate-final-additions.json", "invalid final expansion source")
                selection = final_selection(expected.get("child_phase", "final"))
            validate(output, expected, load(peer_path), selection,
                     report["summary"], report["invocation"])
        except ValueError as error:
            report["errors"].append(str(error))
        except (OSError, KeyError, TypeError, AttributeError, tarfile.TarError):
            report["errors"].append("missing or malformed required runtime evidence")
    print(json.dumps(report, sort_keys=True))
    return bool(report["errors"])


if __name__ == "__main__":
    sys.exit(main())
