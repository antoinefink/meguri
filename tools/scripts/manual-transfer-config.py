#!/usr/bin/env python3
"""RF4 synthetic account derivation. Secrets travel only over stdin/stdout pipes."""
import json
import hashlib
import sys
import http.client
from pathlib import Path


MEDIA_NAMES = ("rf4-server.bin", "rf4-device.bin", "rf4-zero.bin")


def wire_config():
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
        _, token = wire_config()
        port = int(sys.argv[3])
        if port != 18765:
            raise ValueError("unscoped VM port")
        fixture = Path(sys.argv[4])
        run = fixture.parent.name
        def synthetic_hash(label, size):
            data = b"".join(hashlib.sha256(("RF4:%s:%s:%d" % (run, label, block)).encode()).digest()
                            for block in range((size + 31) // 32))[:size]
            return hashlib.sha256(data).hexdigest()
        json.dump(dict(endpoint="http://127.0.0.1:%d/" % port,
                       username=a[0], password=a[1], deckID=1,
                       exportUsername=b[0], exportPassword=b[1],
                       fixturePath=str(fixture), supportPath=sys.argv[5],
                       wireToken=token,
                       serverImportSHA256=synthetic_hash("server-c", 4096),
                       serverExportSHA256=synthetic_hash("server-d", 8192),
                       deviceSHA256=synthetic_hash("device", 6144)), sys.stdout)
    else:
        raise ValueError("unknown operation")


if __name__ == "__main__":
    try:
        main()
    except (ValueError, KeyError, IndexError, OSError, TypeError, http.client.HTTPException):
        # Never include input text, exception reprs or an inspect inventory.
        sys.stderr.write("manual-transfer-data: synthetic configuration refused\n")
        sys.exit(1)
