#!/usr/bin/env python3
"""Prove allowed writes, excluded host paths, denied credentials and networking."""

from pathlib import Path
import socket
import subprocess
import sys


def child(port: int) -> None:
    for path in (
        "/work/oracle", "/work/refs", "/work/local-data", "/work/build",
        "/Users/antoine/code/meguri", "/Applications/Anki.app", "/var/run/docker.sock",
    ):
        assert not Path(path).exists(), "forbidden source/host path is exposed: " + path
    # Codex creates an empty, read-only .git guard when the workspace has none.
    guard = Path("/work/.git")
    assert not guard.is_symlink()
    assert not guard.exists() or (guard.is_dir() and not any(guard.iterdir())), "real Git history exposed"
    try:
        Path("/codex-run/guard-marker").read_bytes()
    except PermissionError:
        pass
    else:
        raise AssertionError("sandbox allowed credential-directory reads")
    marker = Path("/work/cleanroom-probe")
    marker.write_text("allowed write")
    marker.unlink()
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=2):
            pass
    except OSError:
        pass
    else:
        raise AssertionError("sandbox allowed command networking")
    print("cleanroom: allowed write PASS; forbidden paths absent; credentials and network denied")


if __name__ == "__main__":
    if len(sys.argv) == 3 and sys.argv[1] == "child":
        child(int(sys.argv[2]))
    else:
        Path("/codex-run/guard-marker").write_text("synthetic nonsecret guard marker")
        with socket.socket() as server:
            server.bind(("127.0.0.1", 0))
            server.listen(2)
            port = server.getsockname()[1]
            # Confirm the listener answers before checking the sandbox.
            with socket.create_connection(("127.0.0.1", port), timeout=2):
                connection, _ = server.accept()
                connection.close()
            subprocess.run([
                "/opt/codex/bin/codex", "sandbox", "--",
                "python3", "/runner/probe.py", "child", str(port),
            ], check=True)
