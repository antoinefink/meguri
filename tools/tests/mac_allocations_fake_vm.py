#!/usr/bin/env python3
"""VM wrapper fake; process safety has separate checks."""
import json
import os
from pathlib import Path
import sys

helper, action, directory, *extra = sys.argv[1:]
root = Path(directory)
vm = Path(os.environ["FAKE_VM"])
lease = Path(os.environ["MEGURI_HOST_LEASE_DIR"]).parent / ("mac-vm." + vm.name + ".lock/holder.json")
assert lease.is_file(), "allocation control ran outside VM lease"
assert Path(helper).is_file(), "helper was not copied into VM"
with open(os.environ["STUB_LOG"], "a") as log:
    log.write("vm-allocations " + action + " lease-held\n")
mode = os.environ.get("FAKE_ALLOCATIONS", "pass")
if action == "--start":
    binary, runid = extra
    assert binary.endswith("/Build/Products/Profile/Meguri.app/Contents/MacOS/Meguri")
    assert root.parent.name == runid
    (root / "started").touch()
    # Some control failures occur after the remote start. Host must still stop.
    if mode == "start-ssh-loss":
        sys.exit(255)
elif action == "--stop":
    assert (root / "started").exists()
    data = {"diagnostic_mode": "allocations", "ordinary_performance_receipt": False,
            "coverage": "uncertified/partial", "start_seen": mode != "missing-notify",
            "tracing_started_notification_at": "2026-10-20T10:20:30+00:00",
            "finalized": mode not in ("cleanup-failure", "stop-ssh-loss"),
            "trace_exit": 0 if mode == "pass" else 64,
            "cleanup_confirmed": mode != "stop-ssh-loss",
            "errors": [] if mode == "pass" else [mode], "status": "pass" if mode == "pass" else "error"}
    (root / "metadata.json").write_text(json.dumps(data))
    (root / "xctrace.log").write_text("fake trace finalization\n")
    if mode != "missing-trace":
        trace = root / "allocations.trace"
        trace.mkdir()
        (trace / "data").write_text("fake allocations")
    if mode == "stop-ssh-loss":
        sys.exit(255)
    sys.exit(0 if mode == "pass" else 1)
else:
    raise AssertionError("unexpected action")
