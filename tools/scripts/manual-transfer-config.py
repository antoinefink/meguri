#!/usr/bin/env python3
"""RF4 synthetic account derivation. Secrets travel only over stdin/stdout pipes."""
import json
import hashlib
import sys
import http.client
import os
import re
import signal
import stat
import ctypes
import pwd
import subprocess
import time
from contextlib import contextmanager
from pathlib import Path


MEDIA_NAMES = ("rf4-server.bin", "rf4-device.bin", "rf4-zero.bin")
FAILED_FIXTURE_RUN = "20261023T234838Z-84101"
FIXTURE_PURPOSE = "meguri-test-only-manual-transfer-media-v1"


def recipe_run(value):
    if type(value) is not str or re.fullmatch(r"[0-9]{8}T[0-9]{6}Z-[0-9]{1,12}", value) is None:
        raise ValueError("fixture run identity")
    return value


def recipe_bytes(run, label, size):
    recipe_run(run)
    return b"".join(hashlib.sha256(("RF4:%s:%s:%d" % (run, label, block)).encode()).digest()
                    for block in range((size + 31) // 32))[:size]


def recipe_files(run):
    # Independently derive expectations; staged or observed bytes are never an oracle.
    return {MEDIA_NAMES[0]: (recipe_bytes(run, "server-c", 4096), recipe_bytes(run, "server-d", 8192)),
            MEDIA_NAMES[1]: (recipe_bytes(run, "device", 6144),), MEDIA_NAMES[2]: (b"",)}


def dedicated_fixture_vm():
    # No environment-provided home, clock, process arguments or account inventory.
    user = pwd.getpwuid(os.getuid())
    if sys.platform != "darwin" or user.pw_name != "meguri" or user.pw_dir != "/Users/meguri":
        raise ValueError("fixture dedicated VM user")
    model = subprocess.run(["/usr/sbin/sysctl", "-n", "hw.model"], check=True,
                           stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, timeout=5).stdout
    if model.strip() != b"VirtualMac2,1":
        raise ValueError("fixture dedicated VM hardware")
    # Check each process path for the app executable; leave all processes running.
    processes = subprocess.run(["/bin/ps", "-axo", "comm="], check=True,
                              stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, timeout=5).stdout
    if len(processes) > 1024 * 1024 or any(line.rsplit(b"/", 1)[-1] == b"Meguri"
                                         for line in processes.splitlines()):
        raise ValueError("fixture app must be stopped")
    return Path(user.pw_dir)


def rename_new(source_fd, source, destination_fd, destination):
    """Atomic, exclusive, same-filesystem rename. No copy/delete fallback."""
    libc = ctypes.CDLL(None, use_errno=True)
    if sys.platform == "darwin":
        function, flags = libc.renameatx_np, 4  # RENAME_EXCL
    elif sys.platform == "linux":  # Synthetic temp-directory tests only.
        function, flags = libc.renameat2, 1  # RENAME_NOREPLACE
    else:
        raise ValueError("exclusive rename unavailable")
    function.argtypes = [ctypes.c_int, ctypes.c_char_p, ctypes.c_int, ctypes.c_char_p, ctypes.c_uint]
    function.restype = ctypes.c_int
    if function(source_fd, os.fsencode(source), destination_fd, os.fsencode(destination), flags):
        raise OSError(ctypes.get_errno(), "exclusive fixture rename refused")


@contextmanager
def fixture_child(parent, name, create=False):
    # All names are fixed literals, recipe IDs, or context digests, never HTTP paths.
    if create:
        try:
            os.mkdir(name, 0o700, dir_fd=parent)
        except FileExistsError:
            pass
    fd = os.open(name, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=parent)
    try:
        info = os.fstat(fd)
        if info.st_uid != os.getuid() or info.st_mode & 0o022:
            raise ValueError("fixture directory ownership")
        yield fd
    finally:
        os.close(fd)


def fixture_write_new(parent, name, data):
    fd = os.open(name, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600, dir_fd=parent)
    try:
        with os.fdopen(fd, "wb", closefd=False) as stream:
            stream.write(data)
            stream.flush()
            os.fsync(fd)
        os.fchmod(fd, 0o400)
    finally:
        os.close(fd)


def fixture_marker(context, run):
    return dict(purpose=FIXTURE_PURPOSE, context=fingerprint(context), run=recipe_run(run))


def fixture_prior_runs(home, context):
    path = home / "MeguriTestArchives/ManualTransferFixtureMedia" / fingerprint(context) / "ownership"
    # Missing storage is distinct from symlinks, denial or malformed ownership.
    try:
        with directory(path) as parent:
            names = os.listdir(parent)
            if len(names) > 256:
                raise ValueError("fixture ownership bound")
            runs = []
            for name in names:
                run = recipe_run(name.removesuffix(".json"))
                if name != run + ".json":
                    raise ValueError("fixture marker name")
                data, _ = bounded_read(parent, name, 1024)
                if strict_json(data) != fixture_marker(context, run):
                    raise ValueError("fixture marker provenance")
                runs.append(run)
            return runs
    except FileNotFoundError:
        return []


def stage_media(support, run):
    """Test controller only. Never install Collection or interpret transfer policy."""
    home = dedicated_fixture_vm()
    run = recipe_run(run)
    support = Path(support)
    context = native_context(support)
    mac = home / "Library/Containers/com.antoinefink.meguri.dev/Data/Library/Application Support"
    sim = re.fullmatch(re.escape(str(home)) + r"/Library/Developer/CoreSimulator/Devices/[0-9A-Fa-f-]{36}/data/Containers/Data/Application/[0-9A-Fa-f-]{36}/Library/Application Support", str(support))
    if support != mac and sim is None:
        raise ValueError("fixture selected container scope")
    prior = fixture_prior_runs(home, context)
    # These two directories may be absent on the first launch. mkdir is scoped
    # to these exact children and never replaces an existing directory or data.
    with directory(support.parent) as library, fixture_child(library, "Application Support", True) as support_fd:
        with fixture_child(support_fd, "Collection", create=True) as collection:
            with fixture_child(collection, "collection.media", create=True) as media:
                current = {name: bounded_read(media, name, 8192, missing=True) for name in MEDIA_NAMES}
                known = {name: {data for old in prior for data in recipe_files(old)[name]} for name in MEDIA_NAMES}
                # Preflight ALL reserved collisions before recording ownership or
                # moving a file. Matching bytes alone do not establish ownership.
                if any(fact is not None and fact[0] not in known[name] for name, fact in current.items()):
                    raise ValueError("fixture unknown reserved collision")
                with directory(home) as home_fd, fixture_child(home_fd, "MeguriTestArchives", True) as archives:
                    with fixture_child(archives, "ManualTransferFixtureMedia", True) as fixtures:
                        with fixture_child(fixtures, fingerprint(context), True) as storage:
                            with fixture_child(storage, "ownership", True) as owners:
                                marker = json.dumps(fixture_marker(context, run), sort_keys=True).encode()
                                if run not in prior:
                                    if len(prior) >= 256:
                                        raise ValueError("fixture ownership bound")
                                    fixture_write_new(owners, run + ".json", marker)
                            with fixture_child(storage, "originals", True) as originals:
                                with fixture_child(originals, run, True) as saved, fixture_child(storage, "pending", True) as pending:
                                    desired = {MEDIA_NAMES[0]: None, MEDIA_NAMES[1]: recipe_bytes(run, "device", 6144), MEDIA_NAMES[2]: b""}
                                    for name in MEDIA_NAMES:
                                        fact, data = current[name], desired[name]
                                        # A current-run server file is also archived: a
                                        # fresh real Import must prove its own download.
                                        if fact is not None and fact[0] == data:
                                            continue
                                        if bounded_read(media, name, 8192, missing=True) != fact:
                                            raise ValueError("fixture changed before staging")
                                        if fact is not None:
                                            target = name + "-" + hashlib.sha256(fact[0]).hexdigest()
                                            rename_new(media, name, saved, target)
                                            retained, identity = bounded_read(saved, target, 8192)
                                            if retained != fact[0] or identity[:3] != fact[1][:3]:
                                                raise ValueError("fixture archive preservation unconfirmed")
                                            os.fsync(saved)
                                        if data is not None:
                                            temporary = run + "-" + name
                                            staged = bounded_read(pending, temporary, 8192, missing=True)
                                            if staged is None:
                                                fixture_write_new(pending, temporary, data)
                                            elif staged[0] != data:
                                                raise ValueError("fixture interrupted publication")
                                            rename_new(pending, temporary, media, name)
                                            if bounded_read(media, name, 8192)[0] != data:
                                                raise ValueError("fixture publication unconfirmed")
                                    os.fsync(media)
    if native_context(support) != context:
        raise ValueError("fixture container changed")
    return dict(purpose=FIXTURE_PURPOSE, context=fingerprint(context), staged=2)


def repair_source(root, run):
    """Only this run's own bounded public result/seed; never manifests/logs/env."""
    if run != FAILED_FIXTURE_RUN:
        raise ValueError("repair exact source run required")
    root = Path(root)
    with directory(root / "build/mg/manual-transfer-check" / run) as parent:
        owner = os.fstat(parent)
        if owner.st_uid != os.getuid() or owner.st_mode & 0o022:
            raise ValueError("repair source ownership")
        result_data, _ = bounded_read(parent, "result.json", 262144)
        seed_data, _ = bounded_read(parent, "seed.json", 4096)
        for name in ("result.json", "seed.json"):
            info = os.stat(name, dir_fd=parent, follow_symlinks=False)
            if info.st_uid != os.getuid() or info.st_mode & 0o022:
                raise ValueError("repair source file ownership")
    result, seed = strict_json(result_data), strict_json(seed_data)
    if (result.get("root") != str(root) or result.get("job") != "manual-transfer-check" or
            result.get("run_id") != run or result.get("args") != ["--platform", "mac"] or
            result.get("dry_run") is not False or result.get("status") != "fail" or
            type(result.get("failures")) is not list or
            not any(type(f) is dict and f.get("id") == "manual-transfer/execution" for f in result["failures"]) or
            type(result.get("details")) is not dict or result["details"].get("native_transfer_complete") is not False):
        raise ValueError("repair failed controlled source required")
    keys(seed, ("notes", "cards", "reviews", "deckID", "noteID", "cardID", "server_time_ms",
                "distinct_accounts", "empty_nondefault_b", "media_seeded", "media_names"))
    for key, expected in (("notes", 1), ("cards", 1), ("reviews", 0), ("deckID", 1)):
        if type(seed[key]) is not int or seed[key] != expected:
            raise ValueError("repair seed counts")
    for key in ("noteID", "cardID", "server_time_ms"):
        if number(seed[key], (1 << 63) - 1) == 0:
            raise ValueError("repair seed identity")
    if any(seed[key] is not True for key in ("distinct_accounts", "empty_nondefault_b", "media_seeded")) or seed["media_names"] != list(MEDIA_NAMES):
        raise ValueError("repair synthetic source required")
    # The seed belongs to the exact no-symlink source-run directory above.
    # Its reserved media recipe is independently checked in the VM against RUN,
    # never against observed bytes. Whole-tree retention needs no origin claim
    # for OS files or old receipts: this is authorized reversible VM repair.
    return dict(purpose="environment-repair-not-M8", source_run=run, archived=False,
                source_result_sha256=hashlib.sha256(result_data).hexdigest(),
                source_seed_sha256=hashlib.sha256(seed_data).hexdigest())


REPAIR_ARCHIVE = "MeguriTestArchives/ManualTransferSupport-" + FAILED_FIXTURE_RUN
REPAIR_SUPPORT = "Library/Containers/com.antoinefink.meguri.dev/Data/Library/Application Support"
REPAIR_ENDPOINT = "http://127.0.0.1:18765/"


def repair_account():
    # Same public AccountRef encoding as Meguri; no credential/config reads.
    value = hashlib.sha256(b"meguri-manual-account-v1")
    for text in (REPAIR_ENDPOINT, "meguri-c"):
        data = text.encode()
        value.update(len(data).to_bytes(8, "little"))
        value.update(data)
    return value.hexdigest()


def repair_absent_databases(parent):
    # lstat names only: even dangling links count as live/leftover evidence.
    for name in os.listdir(parent):
        if name.startswith(("collection.anki2", "meguri-sidecar.json")) or name.endswith(
                (".anki2", ".anki2-wal", ".anki2-shm", ".anki2-journal")):
            raise ValueError("repair database or activation present")


def repair_policy(support):
    # Reuse the strict read-only schema reducer without changing M8 selectors.
    state = native_state(native_context(support))
    with directory(support / "ManualTransfers") as parent:
        policy, identity = bounded_read(parent, "manual-transfers.json", SIDECAR_LIMIT)
    history = strict_json(policy)["manualTransfers"]
    active = history["operations"].get(history["active"])
    selected = state["operations"]["Import"]
    if (active is None or selected["id"] != history["active"] or
            active["direction"] != "Import" or active["scope"] != "CollectionAndMedia" or
            active["phase"] != "MediaIncomplete" or active["collection_complete"] is not True or
            active["account"] != repair_account() or active["configured_endpoint"] != REPAIR_ENDPOINT or
            active["target_endpoint"] != REPAIR_ENDPOINT or active["acceptance"] != "ExactOk" or
            active["boundary"] is not None or active["receipt"] is not None):
        raise ValueError("repair exact active import required")
    media = active["media"]
    if (media["stage"] != "AwaitingConsent" or media["consent"] is not None or
            media["failure_kind"] is not None or media["remote_outcome_unknown"] is not False or
            media["exclusions_verified"] is not False or
            any(media["counters"][key] != 0 for key in NUMERIC_COUNTERS) or
            media["counters"]["resynced"] is not False or
            media["counters"]["empty"] != [] or media["counters"]["oversized"] != []):
        raise ValueError("repair untouched media decision required")
    # Include every old receipt as well as EVERY account's cooldown. No injected
    # time, expiry reset or account-based exception. Unknown schemas fail closed.
    if any(record["phase"] == "Activating" or record["boundary"] == "ActivationInterrupted"
           for record in history["operations"].values()):
        raise ValueError("repair live activation evidence")
    old_receipts = [record["receipt"] for record in history["operations"].values()
                    if record["receipt"] is not None]
    deadlines = list(history["cooldowns"].values()) + old_receipts
    now = time.time_ns() // 1_000_000
    for value in deadlines:
        if receipt(value) is None or value["next_eligible_at"] > now:
            raise ValueError("repair cooldown not expired")
    return policy, identity, dict(operations=len(history["operations"]),
                                  cooldowns=len(history["cooldowns"]), receipts=len(old_receipts),
                                  vm_time_ms=now, state_guards_proven=True,
                                  cooldown_guards_proven=True)


def archive_stale_fixture_state(source):
    """Leased controller's VM-only repair; preserve truthful post-move failures."""
    result = dict(purpose="environment-repair-not-M8", source_run=FAILED_FIXTURE_RUN,
                  archived=False, status="blocked", archive_location="~/" + REPAIR_ARCHIVE)
    try:
        return _archive_stale_fixture_state(source, result)
    except Exception:
        if result.get("moved"):
            result.update(archived=False, status="preservation-unconfirmed")
            return result
        raise


def _archive_stale_fixture_state(source, result):
    """Never walk unknown children or dereference links within the support tree."""
    keys(source, ("purpose", "source_run", "archived", "source_result_sha256", "source_seed_sha256"))
    if (source["purpose"] != "environment-repair-not-M8" or source["source_run"] != FAILED_FIXTURE_RUN or
            source["archived"] is not False):
        raise ValueError("repair owned source required")
    digest(source["source_result_sha256"]); digest(source["source_seed_sha256"])
    home = dedicated_fixture_vm()  # exact OS/user/home/hardware and stopped app
    support = home / REPAIR_SUPPORT
    result.update(source)
    with directory(support.parent) as library, fixture_child(library, "Application Support") as current:
        before = os.fstat(current)
        repair_absent_databases(current)
        with fixture_child(current, "Collection") as collection:
            repair_absent_databases(collection)
            with fixture_child(collection, "collection.media") as media:
                if set(os.listdir(media)) != {"rf4-device.bin", "rf4-zero.bin"}:
                    raise ValueError("repair exact media names required")
                facts = {name: bounded_read(media, name, 8192) for name in MEDIA_NAMES[1:]}
                for name in MEDIA_NAMES[1:]:
                    if facts[name][0] != recipe_files(FAILED_FIXTURE_RUN)[name][0]:
                        raise ValueError("repair source recipe mismatch")
        policy, policy_identity, guards = repair_policy(support)
        old_sha = hashlib.sha256(policy).hexdigest()
        result.update(guards, policy_sha256_before=old_sha,
                      support_device=before.st_dev, support_inode=before.st_ino,
                      recipe_files=2, recipe_bytes=sum(len(fact[0]) for fact in facts.values()))
        with directory(home) as user, fixture_child(user, "MeguriTestArchives", True) as archives:
            target = "ManualTransferSupport-" + FAILED_FIXTURE_RUN
            if os.fstat(archives).st_dev != before.st_dev:
                raise ValueError("repair archive filesystem")
            try:
                os.stat(target, dir_fd=archives, follow_symlinks=False)
            except FileNotFoundError:
                pass
            else:
                raise ValueError("repair archive target exists")
            # Recheck under the same open parents immediately before ONE rename.
            # No rebuilding, launch, policy edits, recursive reads or deletions.
            if dedicated_fixture_vm() != home:
                raise ValueError("repair VM changed")
            if signature(os.stat("Application Support", dir_fd=library, follow_symlinks=False)) != signature(before):
                raise ValueError("repair support changed")
            repair_absent_databases(current)
            with fixture_child(current, "Collection") as collection:
                repair_absent_databases(collection)
                with fixture_child(collection, "collection.media") as media:
                    if set(os.listdir(media)) != set(facts) or any(
                            bounded_read(media, name, 8192) != fact for name, fact in facts.items()):
                        raise ValueError("repair media changed")
            checked_policy, checked_identity, checked_guards = repair_policy(support)
            if checked_policy != policy or checked_identity != policy_identity:
                raise ValueError("repair policy changed")
            with directory(support) as named_support, directory(home / "MeguriTestArchives") as named_archive:
                if ((os.fstat(named_support).st_dev, os.fstat(named_support).st_ino) != (before.st_dev, before.st_ino) or
                        (os.fstat(named_archive).st_dev, os.fstat(named_archive).st_ino) !=
                        (os.fstat(archives).st_dev, os.fstat(archives).st_ino)):
                    raise ValueError("repair named parent changed")
            result.update(checked_guards)
            rename_new(library, "Application Support", archives, target)
            # From this point EVERY failure says bytes moved and gives the logical
            # persistent location. Never roll back, remove, or recreate anything.
            result.update(status="preservation-unconfirmed", moved=True)
            try:
                with fixture_child(archives, target) as saved:
                    after = os.fstat(saved)
                    if (after.st_dev, after.st_ino) != (before.st_dev, before.st_ino):
                        raise ValueError("repair directory preservation")
                    with fixture_child(saved, "ManualTransfers") as transfers:
                        retained, retained_identity = bounded_read(transfers, "manual-transfers.json", SIDECAR_LIMIT)
                    new_sha = hashlib.sha256(retained).hexdigest()
                    if retained_identity != policy_identity or new_sha != old_sha:
                        raise ValueError("repair policy preservation")
                    try:
                        os.stat("Application Support", dir_fd=library, follow_symlinks=False)
                    except FileNotFoundError:
                        pass
                    else:
                        raise ValueError("repair source reappeared")
                    os.fsync(saved)
                    os.fsync(archives)
                    os.fsync(library)
                    result.update(archived=True, status="archived", policy_sha256_after=new_sha,
                                  archived_device=after.st_dev, archived_inode=after.st_ino)
            except Exception:
                # Only numeric aggregates/digests and a logical VM location leave.
                return result
    return result


def wire_config(config=None):
    if config is None:
        config = json.load(sys.stdin)
    port, token = config["port"], config["token"]
    if type(port) is not int or not 1024 <= port <= 65535:
        raise ValueError("invalid observer port")
    if not isinstance(token, str) or len(token) != 64 or any(c not in '0123456789abcdef' for c in token):
        raise ValueError("invalid observer capability")
    return port, token


def verify_wire(proof):
    checkpoints = proof["checkpoints"]
    names = ("startup", "before-consent", "cancelled", "passive-history",
             "resume-decision", "resumed", "export-before-consent", "exported")
    if set(checkpoints) != set(names):
        raise ValueError("missing native wire checkpoints")
    for point in [proof] + [checkpoints[name] for name in names]:
        expected = {"media", "collection", "collection_transfers", "active", "held", "aborted", "errors", "gate"}
        if set(point) != (expected | {"checkpoints"} if point is proof else expected):
            raise ValueError("unexpected observed fields")
        if any(type(point[key]) is not int or point[key] < 0 for key in (
                "media", "collection", "collection_transfers", "active", "held", "aborted", "errors")):
            raise ValueError("invalid observed aggregates")
        if point["errors"] or point["active"]:
            raise ValueError("undrained or failed observed request")
        if point["gate"] not in ("armed", "holding", "aborted", "released"):
            raise ValueError("invalid observed gate state")
    start, before, cancelled, passive, decision, resumed, export_before, exported = (
        checkpoints[name] for name in names)
    if start["media"] != 0 or before["media"] != 0 or before["collection_transfers"] < 1:
        raise ValueError("media before consent or no real collection import")
    if not (cancelled["media"] > 0 and cancelled["held"] == cancelled["aborted"] == 1):
        raise ValueError("missing held file request and cancellation")
    if not (cancelled["media"] == passive["media"] == decision["media"] < resumed["media"]):
        raise ValueError("passive startup or offline decision sent media")
    if len({point["collection_transfers"] for point in (before, cancelled, passive, decision, resumed)}) != 1:
        raise ValueError("collection resent during media resume")
    if decision["collection"] != resumed["collection"] or passive["collection"] != decision["collection"]:
        raise ValueError("media decision/resume sent collection protocol")
    if resumed["media"] != export_before["media"] or exported["media"] <= export_before["media"]:
        raise ValueError("export consent wire boundary missing")
    if exported["collection_transfers"] <= resumed["collection_transfers"]:
        raise ValueError("missing distinct account export")
    if proof["gate"] != "released" or proof["held"] != 1 or proof["aborted"] != 1:
        raise ValueError("gate not drained/released")
    return proof


def wire_proof():
    port, token = wire_config()
    connection = http.client.HTTPConnection('127.0.0.1', port, timeout=5)
    try:
        connection.request('POST', '/__rf4/proof', body=b'', headers={'X-RF4-Control': token})
        response = connection.getresponse()
        if response.status != 200:
            raise ValueError("observer unavailable")
        body = response.read(16385)
        if len(body) > 16384:
            raise ValueError("observer proof bound")
        return verify_wire(json.loads(body))
    finally:
        connection.close()


# The bridge schema is shared with the observer, never with product code.
NATIVE_LIMIT = 65536
SIDECAR_LIMIT = 2 * 1024 * 1024
U64 = (1 << 64) - 1
NUMERIC_COUNTERS = ("uploaded_files", "uploaded_deletions", "downloaded_files",
                    "downloaded_deletions", "checked", "bytes_downloaded",
                    "bytes_uploaded", "awaiting")
PHASES = ("Preparing", "Preview", "ConfirmedIntent", "Transferring", "RemoteAccepted",
          "Activating", "CollectionComplete", "MediaIncomplete", "Complete",
          "Cancelled", "Failed", "NeedsDecision")
MEDIA_STAGES = ("AwaitingConsent", "Declined", "Running", "Cancelled", "Interrupted", "Failed", "Complete")


def keys(value, expected):
    if type(value) is not dict or set(value) != set(expected):
        raise ValueError("evidence schema")


def number(value, limit=U64):
    if type(value) is not int or not 0 <= value <= limit:
        raise ValueError("evidence number")
    return value


def flag(value):
    if type(value) is not bool:
        raise ValueError("evidence flag")
    return value


def digest(value):
    if type(value) is not str or re.fullmatch(r"[0-9a-f]{64}", value) is None:
        raise ValueError("evidence fingerprint")
    return value


def operation_id(value):
    if type(value) is not str or re.fullmatch(r"[0-9a-f]{16}-[0-9a-f]{16}", value) is None:
        raise ValueError("evidence operation")
    return value


def fingerprint(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(',', ':'),
                                     ensure_ascii=True).encode()).hexdigest()


def strict_json(data):
    def pairs(items):
        result = {}
        for key, value in items:
            if key in result:
                raise ValueError("duplicate evidence field")
            result[key] = value
        return result
    return json.loads(data, object_pairs_hook=pairs,
                      parse_constant=lambda _value: (_ for _ in ()).throw(ValueError("nonfinite evidence")))


def receipt(value):
    if value is not None:
        keys(value, ("completed_at", "next_eligible_at"))
        for instant in value.values():
            number(instant, (1 << 63) - 1)
        if value["next_eligible_at"] <= value["completed_at"]:
            raise ValueError("evidence deadline")
    return value


def counters(value):
    keys(value, NUMERIC_COUNTERS + ("resynced", "empty", "oversized"))
    for key in NUMERIC_COUNTERS:
        number(value[key])
    flag(value["resynced"])
    for key in ("empty", "oversized"):
        names = value[key]
        if type(names) is not list or len(names) > 3 or any(type(n) is not str or n not in MEDIA_NAMES for n in names) or len(set(names)) != len(names):
            raise ValueError("evidence reserved names")
    return value


def validate_native_state(value, context):
    keys(value, ("context", "operations", "files"))
    if digest(value["context"]) != digest(context):
        raise ValueError("stale evidence context")
    keys(value["operations"], ("Import", "Export"))
    for direction, record in value["operations"].items():
        if record is None:
            if direction != "Export":
                raise ValueError("missing import evidence")
            continue
        keys(record, ("id", "direction", "phase", "scope", "authority", "account",
                      "configured_endpoint", "resulting_publication", "collection_complete",
                      "receipt", "success_cooldown", "media"))
        operation_id(record["id"])
        if record["direction"] != direction or record["phase"] not in PHASES or record["scope"] != "CollectionAndMedia":
            raise ValueError("evidence operation state")
        for key in ("authority", "account", "configured_endpoint"):
            digest(record[key])
        if record["resulting_publication"] is not None:
            digest(record["resulting_publication"])
        flag(record["collection_complete"])
        receipt(record["receipt"])
        receipt(record["success_cooldown"])
        media = record["media"]
        keys(media, ("stage", "consent", "consent_bound", "counters", "remote_outcome_unknown", "exclusions_verified"))
        if media["stage"] not in MEDIA_STAGES:
            raise ValueError("evidence media state")
        for key in ("consent_bound", "remote_outcome_unknown", "exclusions_verified"):
            flag(media[key])
        counters(media["counters"])
        if media["consent"] is not None:
            keys(media["consent"], ("categories",))
            if type(media["consent"]["categories"]) is not int or media["consent"]["categories"] != 1:
                raise ValueError("evidence consent")
        elif media["consent_bound"]:
            raise ValueError("evidence absent consent")
    keys(value["files"], MEDIA_NAMES)
    for fact in value["files"].values():
        keys(fact, ("exists", "bytes", "sha256"))
        if flag(fact["exists"]):
            number(fact["bytes"], NATIVE_LIMIT)
            digest(fact["sha256"])
        elif fact["bytes"] is not None or fact["sha256"] is not None:
            raise ValueError("evidence absent file")
    return value


@contextmanager
def directory(path):
    # Walk with dirfd/O_NOFOLLOW: a replaced parent cannot redirect a later read.
    path = Path(path)
    if not path.is_absolute() or any(part in ("..", ".") for part in path.parts[1:]):
        raise ValueError("evidence root")
    fd = os.open('/', os.O_RDONLY | os.O_DIRECTORY)
    try:
        for part in path.parts[1:]:
            child = os.open(part, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=fd)
            os.close(fd)
            fd = child
        yield fd
    finally:
        os.close(fd)


def signature(info):
    return (info.st_dev, info.st_ino, info.st_size, info.st_mtime_ns, info.st_ctime_ns)


def bounded_read(parent, name, limit, missing=False):
    try:
        fd = os.open(name, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=parent)
    except FileNotFoundError:
        if missing:
            return None
        raise ValueError("missing evidence input")
    try:
        before = os.fstat(fd)
        if not stat.S_ISREG(before.st_mode) or before.st_nlink != 1 or before.st_size > limit:
            raise ValueError("evidence file bound")
        with os.fdopen(fd, 'rb', closefd=False) as stream:
            data = stream.read(limit + 1)
        if len(data) > limit or signature(before) != signature(os.fstat(fd)) or signature(before) != signature(os.stat(name, dir_fd=parent, follow_symlinks=False)):
            raise ValueError("changing evidence file")
        return data, signature(before)
    except FileNotFoundError:
        raise ValueError("changing evidence file")
    finally:
        os.close(fd)


def native_context(support):
    support = Path(support)
    if support.parts[-2:] != ("Library", "Application Support"):
        raise ValueError("evidence support root")
    root = support.parent.parent
    mac = str(root).endswith('/Library/Containers/com.antoinefink.meguri.dev/Data')
    sim = re.fullmatch(r"/.*/Devices/[0-9A-Fa-f-]{36}/data/Containers/Data/Application/[0-9A-Fa-f-]{36}", str(root)) is not None
    if not (mac or sim):
        raise ValueError("evidence development container")
    with directory(root) as fd:
        info = os.fstat(fd)
        return dict(root=str(root), device=info.st_dev, inode=info.st_ino)


def native_state(context):
    keys(context, ("root", "device", "inode"))
    number(context["device"]); number(context["inode"])
    support = Path(context["root"]) / "Library/Application Support"
    if native_context(support) != context:
        raise ValueError("evidence container changed")
    with directory(support / "ManualTransfers") as sidecar, directory(support / "Collection/collection.media") as media_dir:
        data, original = bounded_read(sidecar, "manual-transfers.json", SIDECAR_LIMIT)
        document = strict_json(data)
        keys(document, ("manualTransfers",))
        history = document["manualTransfers"]
        keys(history, ("version", "serial", "active", "operations", "cooldowns"))
        if type(history["version"]) is not int or history["version"] != 1:
            raise ValueError("evidence history version")
        number(history["serial"])
        operations, cooldowns = history["operations"], history["cooldowns"]
        if type(operations) is not dict or not 1 <= len(operations) <= 256 or type(cooldowns) is not dict or len(cooldowns) > 256:
            raise ValueError("evidence history bound")
        for account, cooldown in cooldowns.items():
            digest(account); receipt(cooldown)
            if cooldown is None:
                raise ValueError("evidence empty cooldown")
        if history["active"] is not None and operation_id(history["active"]) not in operations:
            raise ValueError("evidence active operation")
        selected = {"Import": None, "Export": None}
        serials = set()
        for identity, record in operations.items():
            operation_id(identity)
            serial = int(identity[:16], 16)
            if serial in serials or serial > history["serial"] or serial == 0:
                raise ValueError("ambiguous operation order")
            serials.add(serial)
            allowed = ("id account configured_endpoint target_endpoint direction scope phase acceptance boundary guard recovery_guard recovery_local local remote candidate activation_identity original safety_backups server_copy prepared_at actual_generation collection_complete resulting_publication media receipt revision").split()
            keys(record, allowed)
            if record["id"] != identity or record["direction"] not in selected:
                raise ValueError("evidence operation identity")
            if record["phase"] not in PHASES or record["scope"] not in ("CollectionOnly", "CollectionAndMedia") or record["acceptance"] not in ("NotSent", "PossiblyAccepted", "ExactOk", "Refused"):
                raise ValueError("evidence authority state")
            if record["boundary"] not in (None, "LocalChanged", "RemoteChanged", "IdentityChanged", "ArtifactChanged", "RemoteCompletionUnknown", "ActivationInterrupted", "MediaProviderUnavailable", "PreparationInterrupted", "Cancelled", "Interrupted", "Storage", "Server", "InvalidDownload"):
                raise ValueError("evidence boundary")
            flag(record["collection_complete"]); receipt(record["receipt"])
            number(record["prepared_at"], (1 << 63) - 1); number(record["revision"])
            if record["actual_generation"] is not None:
                number(record["actual_generation"])
            digest(record["account"])
            endpoint = record["configured_endpoint"]
            if type(endpoint) is not str or not 1 <= len(endpoint) <= 2048:
                raise ValueError("evidence endpoint")
            prior = selected[record["direction"]]
            if prior is None or identity > prior["id"]:
                selected[record["direction"]] = record
        reduced = {}
        for direction, record in selected.items():
            if record is None:
                reduced[direction] = None
                continue
            publication = record["resulting_publication"]
            if publication is not None:
                if type(publication) is not list or len(publication) != 2:
                    raise ValueError("evidence publication")
                for bits in publication:
                    number(bits)
            media = record["media"]
            keys(media, ("stage", "consent", "counters", "remote_outcome_unknown", "exclusions_verified", "failure_kind"))
            consent = media["consent"]
            bound = False
            if consent is not None:
                keys(consent, ("revision", "generation", "categories", "account", "configured_endpoint"))
                for key in ("revision", "generation", "categories"):
                    number(consent[key])
                digest(consent["account"])
                if type(consent["configured_endpoint"]) is not str or len(consent["configured_endpoint"]) > 2048:
                    raise ValueError("evidence consent endpoint")
                bound = (consent["generation"] == record["actual_generation"] and
                         consent["account"] == record["account"] and
                         consent["configured_endpoint"] == record["configured_endpoint"] and
                         consent["categories"] == 1 and consent["revision"] <= record["revision"])
            reduced[direction] = dict(
                id=record["id"], direction=direction, phase=record["phase"], scope=record["scope"],
                authority=fingerprint([record[k] for k in ("id", "account", "configured_endpoint", "direction", "scope")]),
                account=fingerprint(record["account"]), configured_endpoint=fingerprint(record["configured_endpoint"]),
                resulting_publication=None if publication is None else fingerprint(publication),
                collection_complete=record["collection_complete"], receipt=record["receipt"],
                success_cooldown=cooldowns.get(record["account"]), media=dict(
                    stage=media["stage"], consent=None if consent is None else dict(categories=consent["categories"]),
                    consent_bound=bound, counters=media["counters"],
                    remote_outcome_unknown=media["remote_outcome_unknown"], exclusions_verified=media["exclusions_verified"]))
        files = {}
        for name in MEDIA_NAMES:
            result = bounded_read(media_dir, name, NATIVE_LIMIT, missing=True)
            if result is None:
                files[name] = dict(exists=False, bytes=None, sha256=None)
            else:
                data, _ = result
                files[name] = dict(exists=True, bytes=len(data), sha256=hashlib.sha256(data).hexdigest())
        if signature(os.stat("manual-transfers.json", dir_fd=sidecar, follow_symlinks=False)) != original or native_context(support) != context:
            raise ValueError("changing native evidence")
        with directory(support / "ManualTransfers") as current_sidecar, directory(support / "Collection/collection.media") as current_media:
            for current, prior in ((current_sidecar, sidecar), (current_media, media_dir)):
                if signature(os.fstat(current)) != signature(os.fstat(prior)):
                    raise ValueError("changing evidence parent")
        result = dict(context=fingerprint(context), operations=reduced, files=files)
        return validate_native_state(result, fingerprint(context))


def native_proof(support):
    """Reduce app-owned evidence in the VM; never export rows or account records."""
    support = Path(support)
    with (support / "ManualTransfers/manual-transfers.json").open(encoding="utf-8") as stream:
        records = json.load(stream)["manualTransfers"]["operations"].values()
    stages = {}
    for direction in ("Import", "Export"):
        record = max((record for record in records if record["direction"] == direction),
                     key=lambda record: (record["prepared_at"], record["revision"]))
        media = record["media"]
        consent = media.get("consent") or {}
        counters = media["counters"]
        # Fail closed before emitting any unrecognized filenames/server content.
        for category in ("empty", "oversized"):
            if any(name not in MEDIA_NAMES for name in counters[category]):
                raise ValueError("unreserved media exclusion")
        numeric = ("uploaded_files", "uploaded_deletions", "downloaded_files",
                   "downloaded_deletions", "checked", "bytes_downloaded",
                   "bytes_uploaded", "awaiting")
        if any(type(counters[key]) is not int or counters[key] < 0 for key in numeric):
            raise ValueError("invalid media counters")
        safe_counters = {key: counters[key] for key in numeric}
        safe_counters.update(resynced=counters["resynced"], empty=counters["empty"],
                             oversized=counters["oversized"])
        if record["phase"] not in ("Complete", "MediaIncomplete", "CollectionComplete"):
            raise ValueError("unfinished collection")
        if record["scope"] != "CollectionAndMedia" or media["stage"] != "Complete":
            raise ValueError("unfinished selected media")
        receipt = record["receipt"]
        if any(type(value) is not bool for value in (
                counters["resynced"], record["collection_complete"],
                media["exclusions_verified"], media["remote_outcome_unknown"])):
            raise ValueError("invalid completion flags")
        if any(type(receipt[key]) is not int for key in ("completed_at", "next_eligible_at")):
            raise ValueError("invalid completion instants")
        stages[direction] = dict(
            phase=record["phase"], scope=record["scope"], media_stage=media["stage"],
            collection_complete=record["collection_complete"], counters=safe_counters,
            consent_bound=bool(consent and consent["generation"] == record["actual_generation"]
                               and consent["account"] == record["account"]
                               and consent["configured_endpoint"] == record["configured_endpoint"]
                               and consent["categories"] == 1),
            exclusions_verified=media["exclusions_verified"],
            remote_outcome_unknown=media["remote_outcome_unknown"],
            completed_at=receipt["completed_at"], next_eligible_at=receipt["next_eligible_at"])
    files = {}
    for name in MEDIA_NAMES:
        path = support / "Collection/collection.media" / name
        if path.is_symlink() or not path.is_file() or path.stat().st_size > 65536:
            raise ValueError("invalid synthetic native media")
        data = path.read_bytes()
        files[name] = dict(bytes=len(data), sha256=hashlib.sha256(data).hexdigest())
    return dict(stages=stages, files=files)


def accounts(path):
    entries = {}
    with open(path, encoding="utf-8") as stream:
        for line in stream:
            key, sep, value = line.strip().partition("=")
            if sep and key in ("SYNC_USER3", "SYNC_USER4"):
                if key in entries:
                    raise ValueError("duplicate synthetic account")
                entries[key] = value
    a, b = (entries[key].split(":", 1) for key in ("SYNC_USER3", "SYNC_USER4"))
    # This is a dedicated generated-data job, never a generic account loader.
    if a != ["meguri-c", "meguri-test-c"] or b != ["meguri-d", "meguri-test-d"]:
        raise ValueError("reserved synthetic examples changed")
    return a, b


def main():
    if sys.argv[1] == "archive-stale-fixture-state":
        if len(sys.argv) != 2:
            raise ValueError("repair arguments")
        try:
            source = strict_json(sys.stdin.buffer.read(4097))
            proof = archive_stale_fixture_state(source)
        except (ValueError, KeyError, OSError, TypeError, AttributeError, RecursionError,
                subprocess.SubprocessError):
            json.dump(dict(purpose="environment-repair-not-M8", archived=False,
                           status="blocked", source_run=FAILED_FIXTURE_RUN), sys.stdout)
            sys.exit(3)
        json.dump(proof, sys.stdout)
        sys.exit(0 if proof["archived"] else 4)
    if sys.argv[1] == "stage-media":
        if len(sys.argv) != 4:
            raise ValueError("fixture arguments")
        json.dump(stage_media(sys.argv[2], sys.argv[3]), sys.stdout)
        return
    if sys.argv[1] == "repair-source":
        if len(sys.argv) != 4:
            raise ValueError("repair arguments")
        json.dump(repair_source(sys.argv[2], sys.argv[3]), sys.stdout)
        return
    if sys.argv[1] in ("native-state", "native-context"):
        # VM-only bounded reader. No records or exceptions escape on failure.
        signal.signal(signal.SIGALRM, lambda *_args: (_ for _ in ()).throw(ValueError("reader deadline")))
        signal.alarm(6)
        if sys.argv[1] == "native-context":
            value = native_context(sys.argv[2])
        else:
            input_path = Path(sys.argv[2])
            with directory(input_path.parent) as parent:
                data, _ = bounded_read(parent, input_path.name, 4096)
            value = native_state(strict_json(data))
        json.dump(value, sys.stdout)
        return
    if sys.argv[1] == "reader-config":
        context = strict_json(sys.stdin.read(4097))
        keys(context, ("root", "device", "inode"))
        number(context["device"]); number(context["inode"])
        json.dump(dict(context=fingerprint(context), command=[sys.argv[2],
                       "-o", "BatchMode=yes", "-o", "ConnectTimeout=5", sys.argv[3]]), sys.stdout)
        return
    if sys.argv[1] == "wire-port":
        print(wire_config()[0])
        return
    if sys.argv[1] == "wire-proof":
        json.dump(wire_proof(), sys.stdout)
        return
    if sys.argv[1] == "check-bind":
        port = str(int(sys.argv[2]))
        inventory = json.load(sys.stdin)
        if len(inventory) != 1 or inventory[0]["NetworkSettings"]["Ports"]["8080/tcp"] != [
                dict(HostIp="127.0.0.1", HostPort=port)]:
            raise ValueError("owned rig bind mismatch")
        return
    mode, path = sys.argv[1:3]
    if mode == "native-proof":
        json.dump(native_proof(path), sys.stdout)
        return
    a, b = accounts(path)
    if mode == "check-users":
        inventory = json.load(sys.stdin)
        if len(inventory) != 1:
            raise ValueError("expected one rig container")
        env = inventory[0]["Config"]["Env"]
        for key, pair in (("SYNC_USER3", a), ("SYNC_USER4", b)):
            expected = key + "=" + ":".join(pair)
            if [entry for entry in env if entry.startswith(key + "=")] != [expected]:
                raise ValueError("rig synthetic account mismatch")
    elif mode == "manifest":
        config = json.load(sys.stdin)
        _, token = wire_config(config)
        context = digest(config["nativeContext"])
        port = int(sys.argv[3])
        if port != 18765:
            raise ValueError("unscoped VM port")
        fixture = Path(sys.argv[4])
        run = fixture.parent.name
        def synthetic_hash(label, size):
            return hashlib.sha256(recipe_bytes(run, label, size)).hexdigest()
        json.dump(dict(endpoint="http://127.0.0.1:%d/" % port,
                       username=a[0], password=a[1], deckID=1,
                       exportUsername=b[0], exportPassword=b[1],
                       supportPath=sys.argv[5],
                       wireToken=token, nativeContext=context,
                       serverImportSHA256=synthetic_hash("server-c", 4096),
                       serverExportSHA256=synthetic_hash("server-d", 8192),
                       deviceSHA256=synthetic_hash("device", 6144)), sys.stdout)
    else:
        raise ValueError("unknown operation")


if __name__ == "__main__":
    try:
        main()
    except (ValueError, KeyError, IndexError, OSError, TypeError, AttributeError, RecursionError,
            subprocess.SubprocessError, http.client.HTTPException):
        # Never include input text, exception reprs or an inspect inventory.
        sys.stderr.write("manual-transfer-data: synthetic configuration refused\n")
        sys.exit(1)
