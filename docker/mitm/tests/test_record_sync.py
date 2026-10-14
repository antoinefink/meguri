"""Unit tests of record_sync.py's actor and step attribution (sync-ops.md S9).

They run inside the mitmproxy image, which has mitmproxy's test helpers and
zstandard: `tools/mg selftest-host` starts them in the `sync-recorder` service
with `python3 -B -m unittest discover -s /opt/mitm/tests`.
"""

import json
import logging
import os
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from mitmproxy.test import taddons, tflow  # noqa: E402

import record_sync  # noqa: E402


class ActorSteps(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.mkdtemp()
        self.health = os.path.join(self.dir, "health.json")

    def run_flows(self, paths, actor_steps):
        """Passes one flow per request path through the addon; returns the
        paths it forwarded, the recorded lines and the health report."""
        addon = record_sync.RecordSync()
        forwarded = []
        # Each test context attaches a log handler bound to its own event
        # loop and leaves it on the root logger; the next context's logging
        # would reach the closed loop. Restore the handlers afterwards.
        root = logging.getLogger()
        handlers = list(root.handlers)
        try:
            with taddons.context(addon) as tctx:
                tctx.configure(addon, record_dir=self.dir, record_health=self.health, record_actor_steps=actor_steps)
                addon.running()
                for path in paths:
                    flow = tflow.tflow(resp=True)
                    flow.request.path = path
                    addon.request(flow)
                    forwarded.append(flow.request.path)
                    addon.response(flow)
        finally:
            for handler in root.handlers[:]:
                if handler not in handlers:
                    root.removeHandler(handler)
        with open(os.path.join(self.dir, "exchanges.jsonl"), encoding="utf-8") as lines:
            recorded = [json.loads(line) for line in lines]
        with open(self.health, encoding="utf-8") as health:
            return forwarded, recorded, json.load(health)

    def test_the_prefix_is_removed_and_recorded_as_actor_and_step(self):
        forwarded, recorded, health = self.run_flows(
            ["/client_a/7/sync/meta", "/meguri_2/12/sync/upload", "/client_z/0/sync/hostKey"], True
        )
        self.assertEqual(forwarded, ["/sync/meta", "/sync/upload", "/sync/hostKey"])
        self.assertEqual(
            [(line["seq"], line["actor"], line["step"], line["path"]) for line in recorded],
            [(1, "client_a", 7, "/sync/meta"), (2, "meguri_2", 12, "/sync/upload"), (3, "client_z", 0, "/sync/hostKey")],
        )
        self.assertEqual((health["ok"], health["written"], health["errors"]), (True, 3, 0))

    def test_a_request_without_the_prefix_is_forwarded_but_fails_the_recording(self):
        for path in ["/sync/meta", "/client_aa/1/sync/meta", "/meguri_1/1/sync/meta", "/client_a/01/sync/meta"]:
            with self.subTest(path=path):
                self.setUp()
                forwarded, recorded, health = self.run_flows([path], True)
                self.assertEqual(forwarded, [path])
                self.assertEqual(recorded, [])
                self.assertEqual((health["ok"], health["written"], health["errors"]), (False, 0, 1))
                self.assertIn("prefix", health["last_error"])

    def test_without_the_option_paths_and_lines_stay_as_they_were(self):
        forwarded, recorded, health = self.run_flows(["/client_a/7/sync/meta"], False)
        self.assertEqual(forwarded, ["/client_a/7/sync/meta"])
        self.assertEqual(recorded[0]["path"], "/client_a/7/sync/meta")
        self.assertNotIn("actor", recorded[0])
        self.assertNotIn("step", recorded[0])
        self.assertTrue(health["ok"])


if __name__ == "__main__":
    unittest.main()
