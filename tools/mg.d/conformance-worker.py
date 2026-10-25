"""One trusted launch per pooled namespace; the driver never owns its lifetime.

MG invokes this directly with factory-substituted worker identity. No shell or
replacement-driver API; the image entrypoint supplies the driver's clock.
"""
import argparse
import importlib.util
import json
import os
import pathlib
import re
import signal
import subprocess
import sys
import time


def main():
    parser = argparse.ArgumentParser()
    for key in ("root", "project", "run", "owner", "worker", "handoff", "faults", "tz"):
        parser.add_argument("--" + key, required=True)
    args = parser.parse_args()
    if (not re.fullmatch("[0-9a-f]{32}", args.owner)
            or not re.fullmatch("[0-9a-f]{32}", args.worker) or args.owner == args.worker):
        raise ValueError("invalid trusted worker identity")
    root = str(pathlib.Path(args.root).resolve())
    name = "meguri-conf-worker-" + args.owner + "-" + args.worker
    labels = {"meguri.mg.checkout": root, "meguri.mg.project": args.project,
              "meguri.mg.run-id": args.run, "meguri.mg.purpose": "conformance-worker",
              "meguri.mg.edge-owner": args.owner, "meguri.mg.worker": args.worker}
    command = ["docker", "compose", "-p", args.project, "-f", root + "/docker/compose.yml",
               "run", "-d", "-T", "--no-deps", "--name", name]
    for key, value in labels.items():
        command += ["--label", key + "=" + value]
    command += ["-e", "TZ=" + args.tz, "-e", "ORACLE_SYNC_EDGE_OWNER=" + args.owner,
                "-e", "ORACLE_SYNC_WORKER=" + args.worker,
                "-e", "ORACLE_SYNC_FAULTS=" + args.faults,
                "-e", "ORACLE_HANDOFF=/handoff", "-v", args.handoff + ":/handoff",
                "-e", "ORACLE_FIXTURES=/fixtures", "-v", root + "/conformance/fixtures:/fixtures:ro",
                "-e", "LD_PRELOAD=", "--entrypoint", "python3", "oracle",
                "-u", "/opt/oracle/worker_holder.py"]
    # No --rm: holder exit, driver EOF, CLI death and launch failure all leave
    # the namespace owned for MG's edge-first sweep. Compose retains init/tmpfs.
    spec = importlib.util.spec_from_file_location("route_admission", root + "/tools/mg.d/conformance-admission.py")
    admission = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(admission)
    owner_admission = admission.OwnerAdmission(
        pathlib.Path(root) / "build/mg/conformance-routes" / args.owner)
    with owner_admission.hold(time.monotonic() + 25, publishing=True), admission.launch_admission(root):
        # Keep publication within owner admission, including inspect. A launch
        # paused behind infrastructure admission must recheck terminal closure.
        owner_admission.require_open()
        pending = owner_admission.begin_mutation()
        try:
            subprocess.run(command, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                           stderr=subprocess.DEVNULL, check=True, timeout=20,
                           env=dict(os.environ, BUILDX_NO_DEFAULT_ATTESTATIONS="1"))
        except subprocess.CalledProcessError:
            pending.unlink()
            raise
        except BaseException:
            owner_admission.mark("uncertain")
            raise
        pending.unlink()
        state = json.loads(subprocess.check_output(["docker", "inspect", name], timeout=5))[0]
        if (not state["State"]["Running"] or state["HostConfig"].get("AutoRemove")
                or any(state["Config"]["Labels"].get(k) != v for k, v in labels.items())):
            raise ValueError("worker holder is unavailable")
        owner_admission.require_open()
    # Exactly one attached driver. It MUST enter through the clock-setting
    # entrypoint; do not invoke Python directly or inherit holder LD_PRELOAD.
    os.execvp("docker", ["docker", "exec", "-i", name, "/opt/oracle/entrypoint.sh", "worker"])


if __name__ == "__main__":
    def interrupted(_signal, _frame):
        raise RuntimeError("worker launch interrupted")
    for sig in (signal.SIGTERM, signal.SIGINT, signal.SIGHUP):
        signal.signal(sig, interrupted)
    try:
        main()
    except Exception:
        print("conformance worker unavailable", file=sys.stderr)
        sys.exit(1)
