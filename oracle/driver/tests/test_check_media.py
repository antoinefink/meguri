# SPDX-License-Identifier: AGPL-3.0-or-later
"""Check Media and its actions (specs/conformance/media-ops.md F14) and the
check_db rejection failed (specs/conformance/collection-check.md K10),
through real workers. Expected values come from the scenario's names and
bytes and the rules of maintenance/check-media.md, never from the driver."""

from __future__ import annotations

import hashlib
import unittest

from driver.harness import WorkerProcess
from driver.tests.test_worker import run_ok

START = "2030-01-15T10:00:00"
NOTETYPE = {
    "name": "Media",
    "fields": ["Front", "Back"],
    "templates": [
        {"name": "One", "front": "{{Front}}", "back": "<img src=\"{{Back}}\">"},
        {"name": "Two", "front": "[sound:{{Front}}]", "back": "{{Back}}"},
    ],
}


def scenario(steps, notes=(), media_files=(), tags=None, sid="t/check-media"):
    setup = {"notetypes": [NOTETYPE], "notes": [{"notetype": "Media", "fields": list(f)} for f in notes]}
    if media_files:
        setup["media_files"] = list(media_files)
    sc = {"id": sid, "clock": {"tz": "UTC", "start": START}, "setup": setup, "steps": steps}
    if tags is not None:
        sc["tags"] = tags
    return sc


def sha1(text: str) -> str:
    return hashlib.sha1(text.encode("utf-8")).hexdigest()


class CheckMedia(unittest.TestCase):
    def test_the_report_members_come_back_as_values(self):
        sc = scenario(
            [{"check_media": {}}, {"check_media": {}}],
            notes=[("<img src=\"foo[.jpg\"> <img src=\"gone.png\">", ""), ("[sound:b.mp3]", "")],
            media_files=[
                {"name": "foo[.jpg", "text": "renamed"},
                {"name": "b.mp3", "text": "used"},
                {"name": "z.png", "text": "unused"},
                {"name": "a.png", "text": "unused too"},
                {"name": "sub", "dir": True},
            ],
        )
        with WorkerProcess("UTC") as w:
            first, second = run_ok(self, w, sc)
        templates = [
            {"notetype": "Media", "template": "One", "side": "back"},
            {"notetype": "Media", "template": "Two", "side": "front"},
        ]
        self.assertEqual(first["check_media"], {
            "unused": ["a.png", "z.png"],
            "missing": ["gone.png"],
            "missing_notes": ["#n1"],
            "renamed": {"foo[.jpg": "foo.jpg"},
            "dirs": ["sub"],
            "oversize": [],
            "extracted": 0,
            "trash_count": 0,
            "field_templates": templates,
        })
        self.assertEqual(second["check_media"]["renamed"], {})
        self.assertEqual(second["check_media"]["field_templates"], templates)

    def test_a_renamed_old_name_holding_the_separator(self):
        # Anki removes '>' (a disallowed character) from a name, so the old
        # name may hold the renamed line's separator and the new one cannot.
        sc = scenario(
            [{"check_media": {}}, {"observe": ["media_dir"]},
             {"db_exec": {"sql": "SELECT flds FROM notes ORDER BY id"}}],
            notes=[("[sound:a -> b.jpg]", "")],
            media_files=[{"name": "a -> b.jpg", "text": "arrow"}],
        )
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, sc)
        self.assertEqual(out[0]["check_media"]["renamed"], {"a -> b.jpg": "a - b.jpg"})
        self.assertEqual(out[0]["check_media"]["unused"], [])
        self.assertEqual(out[0]["check_media"]["missing"], [])
        self.assertEqual(out[1]["media_dir"], {"a - b.jpg": {"sha1": sha1("arrow"), "size": 5}})
        self.assertEqual(out[2]["db_exec"]["rows"], [["[sound:a - b.jpg]\x1f"]])

    def test_delete_unused_and_tag_missing_act_on_the_latest_run(self):
        sc = scenario(
            [{"check_media": {}}, {"tag_missing_media": {}}, {"advance": "1m"}, {"delete_unused_media": {}},
             {"tag_missing_media": {}}, {"observe": ["media_trash"]},
             {"db_exec": {"sql": "SELECT tags FROM notes ORDER BY id"}}],
            notes=[("<img src=\"gone.png\">", ""), ("<img src=\"a.jpg\">", "")],
            media_files=[{"name": "a.jpg", "text": "used"}, {"name": "b.jpg", "text": "unused"}],
        )
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, sc)
        self.assertEqual(out[1], {"tag_missing_media": {"count": 1}})
        self.assertEqual(out[3], {"delete_unused_media": {"count": 1}})
        # A note that has the tag already is not written again.
        self.assertEqual(out[4], {"tag_missing_media": {"count": 0}})
        self.assertEqual(out[5]["media_trash"], [{"name": "b.jpg", "sha1": sha1("unused"), "size": 6}])
        self.assertEqual(out[6]["db_exec"]["rows"], [[" missing-media "], [""]])

    def test_actions_need_a_check_media_step_and_only_f14_2_steps_between(self):
        bad = [
            [{"delete_unused_media": {}}],
            [{"tag_missing_media": {}}],
            [{"check_media": {}}, {"handoff": "oracle"}, {"tag_missing_media": {}}],
            [{"check_media": {}}, {"db_exec": {"sql": "SELECT 1"}}, {"delete_unused_media": {}}],
            [{"check_media": {}}, {"media_outside": [{"name": "n.jpg", "text": "x"}]}, {"delete_unused_media": {}}],
            [{"check_media": {"x": 1}}],
            [{"check_media": {}}, {"delete_unused_media": []}],
        ]
        with WorkerProcess("UTC") as w:
            for steps in bad:
                resp = w.request({"cmd": "run", "scenario": scenario(steps)})
                with self.subTest(steps=steps):
                    self.assertFalse(resp["ok"], resp)
                    self.assertEqual(resp["error"]["kind"], "invalid_scenario", resp)
            ok = scenario([{"check_media": {}}, {"observe": ["media_dir"]}, {"advance": "1m"},
                           {"tag_missing_media": {}}, {"delete_unused_media": {}}])
            run_ok(self, w, ok)

    def test_case_variants_only_with_the_tag(self):
        files = [{"name": "Cat.jpg", "text": "upper"}, {"name": "cat.jpg", "text": "lower"}]
        with WorkerProcess("UTC") as w:
            resp = w.request({"cmd": "run", "scenario": scenario([{"observe": ["media_dir"]}], media_files=files)})
            self.assertFalse(resp["ok"], resp)
            self.assertEqual(resp["error"]["kind"], "invalid_scenario", resp)
            out = run_ok(self, w, scenario([{"observe": ["media_dir"]}], media_files=files, tags=["media-case-variants"]))
            self.assertEqual(sorted(out[0]["media_dir"]), ["Cat.jpg", "cat.jpg"])
            # Names equal after NFC are refused with the tag too.
            nfc = [{"name": "é.jpg", "text": "a"}, {"name": "é.jpg", "text": "b"}]
            resp = w.request({"cmd": "run", "scenario": scenario([{"observe": ["media_dir"]}], media_files=nfc,
                                                                 tags=["media-case-variants"])})
            self.assertFalse(resp["ok"], resp)
            self.assertEqual(resp["error"]["kind"], "invalid_scenario", resp)


class CheckDbFailed(unittest.TestCase):
    def test_a_check_database_error_is_the_rejection_failed_and_changes_nothing(self):
        # collection-check.md K10: a review-log ivl outside i32 on a reviewed
        # card stops Anki's Check Database before it commits anything.
        sc = scenario(
            [{"answer": {"card": "#c1", "rating": "easy"}},
             {"db_exec": {"sql": "UPDATE revlog SET ivl = 3000000000"}},
             {"db_exec": {"sql": "UPDATE cards SET due = 2.5"}},
             {"check_db": {}},
             {"db_exec": {"sql": "SELECT ivl FROM revlog"}},
             {"db_exec": {"sql": "SELECT typeof(due), due FROM cards ORDER BY id"}}],
            notes=[("a", "b")],
        )
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, sc)
        self.assertEqual(out[3], {"check_db": {"error": "failed"}})
        self.assertEqual(out[4]["db_exec"]["rows"], [[3000000000]])
        self.assertEqual(out[5]["db_exec"]["rows"], [["real", 2.5], ["real", 2.5]])


if __name__ == "__main__":
    unittest.main()
