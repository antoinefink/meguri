# SPDX-License-Identifier: AGPL-3.0-or-later
"""add_media_from_url with http_fixtures and url_files
(specs/conformance/editor-ops.md E1-E5) through real workers. Expected names
come from the rules of specs/editor/paste-sources.md S5, S6 and S6.1 (page
editor column) and media-files M3, and expected bytes from the fixtures,
never from the driver."""

from __future__ import annotations

import hashlib
import time
import unittest

from driver.harness import WorkerProcess
from driver.tests.test_worker import run_ok

START = "2030-01-15T10:00:00"


def scenario(steps, fixtures=None, files=None, media_files=None, sid="t/url-media"):
    sc = {"id": sid, "clock": {"tz": "UTC", "start": START}, "steps": steps}
    setup = {}
    if fixtures is not None:
        setup["http_fixtures"] = fixtures
    if files is not None:
        setup["url_files"] = files
    if media_files is not None:
        setup["media_files"] = media_files
    if setup:
        sc["setup"] = setup
    return sc


def fetch(url):
    return {"add_media_from_url": {"url": url}}


def entry(data: bytes) -> dict:
    return {"sha1": hashlib.sha1(data).hexdigest(), "size": len(data)}


def names(out):
    return [step["add_media_from_url"] for step in out]


class Names(unittest.TestCase):
    def test_the_call_returns_the_name_it_derived_and_stores_the_transformed_one(self):
        fixtures = [
            {"path": "/dir/Pic.PNG", "content_type": "image/png", "text": "pic"},
            {"path": "/img", "content_type": "image/png", "text": "img"},
            {"path": "/", "content_type": "image/jpeg; charset=binary", "text": "root"},
            {"path": "/x", "content_type": "Image/PNG", "text": "x"},
            {"path": "/dir/", "content_type": "image/png", "text": "dir"},
            {"path": "/dir/a%2Fb.png", "content_type": "image/png", "text": "slash"},
            {"path": "/%FF.png", "content_type": "image/png", "text": "ff"},
            {"path": "/w/play", "content_type": "audio/webm", "text": "weba"},
            {"path": "/none", "text": "untyped"},
        ]
        steps = [
            fetch("{fixture}/dir/Pic.PNG?size=2"),
            fetch("{fixture}/img?id=3"),
            fetch("{fixture}/"),
            fetch("{fixture}/x"),
            fetch("{fixture}/dir/"),
            fetch("{fixture}/dir/a%2Fb.png"),
            fetch("{fixture}/%FF.png"),
            fetch("{fixture}/w/play?f=a.mp3"),
            fetch("{fixture}/none"),
            {"observe": ["media_dir"]},
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario(steps, fixtures))
        self.assertEqual(names(out[:9]), [
            {"name": "Pic.PNG"},
            {"name": "img.png"},
            {"name": "paste"},
            {"name": "x"},
            {"name": "paste.png"},
            {"name": "a/b.png"},
            {"error": "fetch_failed"},
            {"name": "play.weba"},
            {"name": "none"},
        ])
        self.assertEqual(out[9]["media_dir"], {
            "pic.png": entry(b"pic"),
            "img.png": entry(b"img"),
            "paste": entry(b"root"),
            "x": entry(b"x"),
            "paste.png": entry(b"dir"),
            "ab.png": entry(b"slash"),
            "play.weba": entry(b"weba"),
            "none": entry(b"untyped"),
        })


class Responses(unittest.TestCase):
    def test_statuses_redirects_and_unknown_paths(self):
        fixtures = [
            {"path": "/ok", "content_type": "image/png", "text": "ok"},
            {"path": "/moved", "redirect_to": "/ok"},
            {"path": "/gone", "status": 404, "content_type": "image/png", "text": "not here"},
            {"path": "/broken", "status": 500, "text": "failed"},
            {"path": "/empty", "content_type": "image/png"},
            {"path": "/late", "delay_ms": 300, "byte_delay_ms": 100, "content_type": "image/png", "text": "abc"},
            # A redirect status without Location is not followed: the fetch keeps that response.
            {"path": "/bare", "status": 302, "content_type": "image/png", "text": "bare"},
        ]
        steps = [
            fetch("{fixture}/ok"),
            fetch("{fixture}/moved"),
            fetch("{fixture}/gone"),
            fetch("{fixture}/broken"),
            fetch("{fixture}/nowhere.png"),
            fetch("{fixture}/empty"),
            fetch("{fixture}/late"),
            fetch("{fixture}/bare"),
            {"observe": ["media_dir"]},
        ]
        with WorkerProcess("UTC") as w:
            began = time.monotonic()
            out = run_ok(self, w, scenario(steps, fixtures))
            took = time.monotonic() - began
        self.assertEqual(names(out[:8]), [
            {"name": "ok.png"},
            # The name comes from the URL asked for, the type from the final response.
            {"name": "moved.png"},
            {"error": "fetch_failed"},
            {"error": "fetch_failed"},
            {"error": "fetch_failed"},
            {"name": "empty.png"},
            {"name": "late.png"},
            {"name": "bare.png"},
        ])
        self.assertEqual(out[8]["media_dir"], {
            "ok.png": entry(b"ok"),
            "moved.png": entry(b"ok"),
            "empty.png": entry(b""),
            "late.png": entry(b"abc"),
            "bare.png": entry(b"bare"),
        })
        self.assertGreaterEqual(took, 0.6, "delay_ms and byte_delay_ms wait before the bytes")


class Files(unittest.TestCase):
    def test_file_urls_read_the_url_files_folder(self):
        files = [{"name": "b c.JPG", "text": "jpeg"}, {"name": "plain.png", "fill": {"size": 3, "byte": 9}}]
        steps = [
            fetch("{files}/b%20c.JPG"),
            fetch("{files}/plain.png"),
            fetch("{files}/missing.png"),
            {"observe": ["media_dir"]},
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario(steps, files=files))
        self.assertEqual(names(out[:3]), [{"name": "b c.JPG"}, {"name": "plain.png"}, {"error": "fetch_failed"}])
        self.assertEqual(out[3]["media_dir"], {"b c.jpg": entry(b"jpeg"), "plain.png": entry(bytes([9]) * 3)})


class Invalid(unittest.TestCase):
    def test_bad_setup_is_refused_before_the_steps(self):
        bad = [
            {"http_fixtures": "x"},
            {"http_fixtures": [{"text": "x"}]},
            {"http_fixtures": [{"path": "a"}]},
            {"http_fixtures": [{"path": "/a?b"}]},
            {"http_fixtures": [{"path": "/a b"}]},
            {"http_fixtures": [{"path": "/é"}]},
            {"http_fixtures": [{"path": "/a"}, {"path": "/a"}]},
            {"http_fixtures": [{"path": "/a", "status": 199}]},
            {"http_fixtures": [{"path": "/a", "status": True}]},
            {"http_fixtures": [{"path": "/a", "redirect_to": "/b"}]},
            {"http_fixtures": [{"path": "/a", "redirect_to": "/a", "status": 200}]},
            {"http_fixtures": [{"path": "/a", "status": 204, "text": "x"}]},
            {"http_fixtures": [{"path": "/a", "text": "x", "fill": {"size": 1}}]},
            {"http_fixtures": [{"path": "/a", "content_type": ""}]},
            {"http_fixtures": [{"path": "/a", "content_type": "image/png\r\nX: y"}]},
            {"http_fixtures": [{"path": "/a", "content_type": " image/png"}]},
            {"http_fixtures": [{"path": "/a", "delay_ms": 35_001}]},
            {"http_fixtures": [{"path": "/a", "byte_delay_ms": -1}]},
            {"http_fixtures": [{"path": "/a", "other": 1}]},
            {"url_files": [{"name": "a/b", "text": "x"}]},
            {"url_files": [{"name": "a"}]},
            {"url_files": [{"name": "a", "text": "x"}, {"name": "a", "text": "y"}]},
        ]
        with WorkerProcess("UTC") as w:
            for setup in bad:
                sc = {"id": "t/url-media", "clock": {"tz": "UTC", "start": START}, "setup": setup,
                      "steps": [{"observe": ["media_dir"]}]}
                resp = w.request({"cmd": "run", "scenario": sc})
                with self.subTest(setup=setup):
                    self.assertFalse(resp["ok"], resp)
                    self.assertEqual(resp["error"]["kind"], "invalid_scenario", resp)
                    self.assertNotIn("step", resp["error"], resp)

    def test_urls_name_no_host(self):
        bad = [
            {"url": "http://127.0.0.1:1/a.png"},
            {"url": "https://example.com/a.png"},
            {"url": "file:///etc/hostname"},
            {"url": "{fixture}a.png"},
            {"url": 1},
            {"url": "{fixture}/a.png", "other": 1},
            {},
        ]
        fixtures = [{"path": "/a.png", "text": "a"}]
        with WorkerProcess("UTC") as w:
            for args in bad:
                resp = w.request({"cmd": "run", "scenario": scenario([{"add_media_from_url": args}], fixtures)})
                with self.subTest(args=args):
                    self.assertFalse(resp["ok"], resp)
                    self.assertEqual(resp["error"]["kind"], "invalid_scenario", resp)
            resp = w.request({"cmd": "run", "scenario": scenario([fetch("{fixture}/a.png")])})
            self.assertEqual(resp["error"]["kind"], "invalid_scenario", resp)


if __name__ == "__main__":
    unittest.main()
