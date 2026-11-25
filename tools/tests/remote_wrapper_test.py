"""Synthetic transport tests; no SSH connection, VM or host input."""
import importlib.util
import io
import json
import os
from pathlib import Path
import shlex
import subprocess
import sys
import tarfile
import tempfile
import time
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location(
    "remote_wrapper", Path(__file__).resolve().parents[1] / "vm/remote-wrapper.py")
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)


class RemoteWrapperTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix="remote kit '$; ")
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        (self.root / "bin").mkdir()
        for name in ("tart", "vm-ssh"):
            self.script(name, "import sys\nsys.stdout.buffer.write(sys.stdin.buffer.read())\n"
                        "print(__import__('json').dumps(sys.argv[1:]), file=sys.stderr)\n"
                        "sys.exit(37)\n")

    def script(self, name, body):
        path = self.root / "bin" / name
        path.write_text("#!" + sys.executable + "\n" + body)
        path.chmod(0o700)
        return path

    def remote(self, mode, args, data=b""):
        # Execute exactly the command the host SSH shell receives, locally.
        return subprocess.run(["/bin/sh", "-c", m.remote_command(mode, args, str(self.root))],
                              input=data, capture_output=True, timeout=5)

    def test_argument_boundaries_binary_streams_and_status(self):
        args = ["", "two words", "'\"\\", "$(exit 19); *", "line\nbreak", "é"]
        for mode in ("tart", "vm-ssh"):
            result = self.remote(mode, args, bytes(range(256)) * 100)
            self.assertEqual(result.returncode, 37)
            self.assertEqual(result.stdout, bytes(range(256)) * 100)
            self.assertEqual(json.loads(result.stderr), args)

    def test_tar_in_both_directions(self):
        self.script("vm-ssh", "import os, sys\nos.execv('/bin/sh', ['sh', '-c', sys.argv[1]])\n")
        payload = bytes(range(256)) * 100
        archive = io.BytesIO()
        with tarfile.open(fileobj=archive, mode="w") as tar:
            entry = tarfile.TarInfo("payload with spaces")
            entry.size = len(payload)
            tar.addfile(entry, io.BytesIO(payload))
        target = self.root / "out"
        target.mkdir()
        incoming = self.remote("vm-ssh", [shlex.join(["tar", "-xf", "-", "-C", str(target)])],
                               archive.getvalue())
        self.assertEqual(incoming.returncode, 0, incoming.stderr)
        self.assertEqual((target / entry.name).read_bytes(), payload)
        outgoing = self.remote("vm-ssh", [shlex.join(["tar", "-cf", "-", "-C", str(target), entry.name])])
        self.assertEqual(outgoing.returncode, 0, outgoing.stderr)
        with tarfile.open(fileobj=io.BytesIO(outgoing.stdout)) as tar:
            self.assertEqual(tar.extractfile(entry.name).read(), payload)

    def test_missing_kit_or_either_wrapper_is_unavailable(self):
        for name in ("tart", "vm-ssh"):
            path = self.root / "bin" / name
            path.chmod(0o600)
            for mode in ("tart", "vm-ssh"):
                self.assertEqual(self.remote(mode, ["true"]).returncode, 255)
            path.chmod(0o700)
        self.assertEqual(subprocess.run(["/bin/sh", "-c", m.remote_command(
            "vm-ssh", ["true"], str(self.root / "absent SSD"))],
            capture_output=True).returncode, 255)

    def test_only_plain_mg_start_detaches(self):
        for args in (["run", "--no-graphics", "--vnc-experimental", "guest"],
                     ["run", "guest"], ["run", "--no-graphics", "--help"],
                     ["run", "--no-graphics", "guest", "--other"]):
            result = self.remote("tart", args)
            self.assertEqual(result.returncode, 37)
            self.assertEqual(json.loads(result.stderr), args)

    def test_detached_start_has_own_session_closes_streams_and_keeps_log(self):
        proof = self.root / "finished"
        release = self.root / "release"
        self.addCleanup(release.touch)
        self.script("tart", "import os, time\nprint('startup diagnostic', flush=True)\n"
                    "assert os.getsid(0) == os.getpid()\n"
                    "assert os.read(0, 1) == b''\n"
                    + "while not os.path.exists(" + repr(str(release)) + "): time.sleep(0.05)\n"
                    + "open(" + repr(str(proof)) + ", 'w').write('survived')\n")
        # The pipe readers must see EOF before the detached process finishes.
        result = self.remote("tart", ["run", "--no-graphics", "guest"])
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(proof.exists())
        release.touch()
        deadline = time.monotonic() + 4
        while not proof.exists() and time.monotonic() < deadline:
            time.sleep(0.05)
        self.assertEqual(proof.read_text(), "survived")
        log, = self.root.glob("tart-run-*.log")
        self.assertIn(b"startup diagnostic", log.read_bytes())
        self.assertEqual(log.stat().st_mode & 0o777, 0o600)
        self.assertIn(os.fsencode(log), result.stderr)

    def test_immediate_start_failure_is_diagnosable(self):
        self.script("tart", "import sys\nprint('start refused', file=sys.stderr)\nsys.exit(23)\n")
        result = self.remote("tart", ["run", "--no-graphics", "guest"])
        self.assertEqual(result.returncode, 23)
        self.assertIn(b"start refused", result.stderr)

    def test_ssh_options_and_config_validation(self):
        installed = self.root / "vm-ssh"
        installed.write_text("")
        config = self.root / "remote.json"
        config.write_text(json.dumps({"host": "test-alias", "runtime": str(self.root)}))
        with patch.object(sys, "argv", [str(installed), "two words", ""]), \
                patch.object(os, "execv") as execute:
            m.main()
        program, argv = execute.call_args.args
        self.assertEqual(program, "/usr/bin/ssh")
        self.assertEqual(argv[:-1], [program, "-T", "-o", "BatchMode=yes", "-o",
                         "StrictHostKeyChecking=yes", "-o", "ConnectTimeout=8", "-o",
                         "ServerAliveInterval=15", "-o", "ServerAliveCountMax=3", "-o",
                         "ForwardAgent=no", "-o", "RequestTTY=no", "--", "test-alias"])
        dedicated = m.ssh_arguments("test-alias", dedicated=True)
        self.assertEqual(dedicated, [*argv[:-3], "-o", "ControlMaster=no",
                                    "-o", "ControlPath=none", "--", "test-alias"])
        self.assertNotIn("ControlMaster=no", argv)
        self.assertNotIn("ControlPath=none", argv)
        for value in ({}, {"host": "-bad", "runtime": "/kit"},
                      {"host": "bad\nhost", "runtime": "/kit"},
                      {"host": "alias", "runtime": "relative"}, []):
            config.write_text(json.dumps(value))
            with patch.object(sys, "argv", [str(installed)]), patch.object(os, "execv") as execute:
                self.assertEqual(m.main(), 255)
                execute.assert_not_called()


if __name__ == "__main__":
    unittest.main()
