"""Tests for the review gallery and its server (U10, R1988, R1989)."""

import datetime
import hashlib
import json
import os
import shutil
import sys
import tempfile
import threading
import time
import unittest
import urllib.error
import urllib.request

from PIL import Image

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.dirname(HERE))

import gallery  # noqa: E402
import server  # noqa: E402

BASELINE = "app/Snapshots/iPhone18,1-24A434/home.deckList/typical/light-standard-iphoneLarge.png"


class GalleryTest(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.mkdtemp()
        self.checkout = os.path.join(self.dir, "checkout")
        os.makedirs(os.path.join(self.checkout, "docs", "design"))
        with open(os.path.join(self.checkout, "docs", "design", "decisions.md"), "w") as handle:
            handle.write("| Date | Screen | Decision | Reason | Source |\n|---|---|---|---|---|\n")
        self.new = self.png(os.path.join(self.checkout, BASELINE), (0, 122, 255))
        self.old = self.png(os.path.join(self.dir, "old.png"), (255, 59, 48))
        self.same = self.png(os.path.join(self.dir, "same.png"), (0, 122, 255))
        self.out = os.path.join(self.dir, "site")

    def tearDown(self):
        shutil.rmtree(self.dir, ignore_errors=True)

    def png(self, path, colour):
        os.makedirs(os.path.dirname(path), exist_ok=True)
        image = Image.new("RGB", (60, 40), (255, 255, 255))
        image.paste(colour, (10, 10, 50, 30))
        image.save(path)
        return path

    def item(self, **changes):
        item = {"id": "home.deckList/typical/light-standard-iphoneLarge", "screen": "home.deckList", "state": "typical",
                "variant": "light-standard-iphoneLarge", "platform": "iphone", "baseline": BASELINE, "new": self.new,
                "old": self.old, "checkout": self.checkout, "branch": "feat/meguri-v1", "pending": False, "signature": True}
        item.update(changes)
        # What tools/mg gallery finds in git: the blob HEAD holds at the
        # baseline's path, when the file there is that blob.
        if "committed_blob" not in changes:
            with open(item["new"], "rb") as handle:
                item["committed_blob"] = gallery.git_blob_id(handle.read())
        return item

    def build(self, items=None, picks=None):
        return gallery.build(items or [self.item()], self.out, picks)

    def test_the_page_shows_old_new_and_diff_with_open_issues(self):
        ledger = os.path.join(self.dir, "status.yaml")
        with open(ledger, "w") as handle:
            handle.write("version: 1\nscreens:\n  home.deckList:\n    status: built\n    issues:\n    - {severity: high, rule: text-truncation, variant: dark-ax5-iphoneSmall, summary: Name cut off}\n")
        manifest = gallery.build([self.item(), self.item(id="b", variant="dark-standard-iphoneLarge", old=self.same)], self.out, [], [ledger])
        changed, unchanged = manifest["items"]
        self.assertEqual(changed["change"], "changed")
        self.assertTrue(os.path.exists(os.path.join(self.out, changed["diff"])))
        self.assertEqual(unchanged["change"], "unchanged")
        page = open(os.path.join(self.out, "index.html")).read()
        for text in ("old", "new", "diff", "text-truncation", "Name cut off", "data-decision=approve"):
            self.assertIn(text, page)
        self.assertNotIn("<script src", page)

    def test_every_image_opens_at_full_size(self):
        manifest = self.build()
        page = open(os.path.join(self.out, "index.html")).read()
        [item] = manifest["items"]
        for side in ("old", "new", "diff"):
            src = item[side]
            self.assertIn("<a href='%s' target=_blank rel=noopener><img loading=lazy src='%s'" % (src, src), page)

    def test_an_image_not_committed_as_shown_cannot_be_approved(self):
        self.build([self.item(committed_blob=None)])
        with self.assertRaises(gallery.GalleryError) as caught:
            gallery.decide(self.out, self.item()["id"], "approve", "")
        self.assertIn("not committed", str(caught.exception))
        self.build([self.item(committed_blob="0" * 40)])
        with self.assertRaises(gallery.GalleryError):
            gallery.decide(self.out, self.item()["id"], "approve", "")
        self.assertFalse(os.path.exists(os.path.join(self.checkout, gallery.APPROVED)))
        # A reject needs no committed image.
        gallery.decide(self.out, self.item()["id"], "reject", "Too faint")

    def test_approving_an_os_builds_successor_retires_the_entry_it_replaces(self):
        old = "app/Snapshots/iPhone18,1-24A434/home.deckList/typical/light-standard-iphoneLarge.png"
        other = "app/Snapshots/iPhone18,5-24A434/home.deckList/typical/light-standard-iphoneLarge.png"
        new = "app/Snapshots/iPhone18,1-24B100/home.deckList/typical/light-standard-iphoneLarge.png"
        gallery.write_json(os.path.join(self.checkout, gallery.APPROVED), {"version": 1, "baselines": [
            {"path": old, "screen": "home.deckList", "signature": True, "sha256": "a", "blob": "b"},
            {"path": other, "screen": "home.deckList", "signature": True, "sha256": "c", "blob": "d"},
        ]})
        os.makedirs(os.path.dirname(os.path.join(self.checkout, new)))
        shutil.copyfile(self.new, os.path.join(self.checkout, new))
        self.build([self.item(baseline=new)])
        written = gallery.decide(self.out, self.item()["id"], "approve", "")
        approved = json.load(open(os.path.join(self.checkout, gallery.APPROVED)))
        self.assertEqual(sorted(b["path"] for b in approved["baselines"]), sorted([new, other]))
        entry = next(b for b in approved["baselines"] if b["path"] == new)
        self.assertEqual(entry["supersedes"], [old])
        review = json.load(open(os.path.join(self.checkout, written["files"][0])))
        self.assertEqual(review["decisions"][-1]["supersedes"], [old])

    def test_concurrent_approvals_all_land(self):
        # Eight images approved at once through the server: every approval and
        # every decision is kept. Reads are slowed down so that unserialized
        # read-modify-writes would overwrite each other.
        items = []
        for n in range(8):
            path = "app/Snapshots/iPhone18,1-24A434/home.deckList/typical/cell-%d.png" % n
            image = self.png(os.path.join(self.checkout, path), (n * 30, 122, 255))
            items.append(self.item(id="cell-%d" % n, variant="cell-%d" % n, baseline=path, new=image))
        self.build(items)
        read_json = gallery.read_json

        def slow_read(path, default):
            data = read_json(path, default)
            time.sleep(0.05)
            return data

        gallery.read_json = slow_read
        httpd = server.ThreadingHTTPServer(("127.0.0.1", 0), server.make_handler(self.out))
        threading.Thread(target=httpd.serve_forever, daemon=True).start()
        base = "http://127.0.0.1:%d" % httpd.server_address[1]
        answers = []

        def approve(item_id):
            request = urllib.request.Request(base + "/api/decision", data=json.dumps({"item": item_id, "decision": "approve"}).encode(), method="POST")
            request.add_header("Content-Type", "application/json")
            answers.append(json.loads(urllib.request.urlopen(request).read()))

        try:
            threads = [threading.Thread(target=approve, args=(item["id"],)) for item in items]
            for thread in threads:
                thread.start()
            for thread in threads:
                thread.join()
        finally:
            gallery.read_json = read_json
            httpd.shutdown()
            httpd.server_close()
        self.assertEqual(len(answers), 8)
        approved = json.load(open(os.path.join(self.checkout, gallery.APPROVED)))
        self.assertEqual(len(approved["baselines"]), 8)
        review_dir = os.path.join(self.checkout, "docs", "design", "reviews")
        [review] = os.listdir(review_dir)
        self.assertEqual(len(json.load(open(os.path.join(review_dir, review)))["decisions"]), 8)
        self.assertEqual([f for f in os.listdir(os.path.dirname(os.path.join(self.checkout, gallery.APPROVED))) if f.endswith(".tmp")], [])

    def test_approving_an_image_locks_it_and_records_the_decision(self):
        self.build()
        written = gallery.decide(self.out, self.item()["id"], "approve", "")
        approved = json.load(open(os.path.join(self.checkout, gallery.APPROVED)))
        [entry] = approved["baselines"]
        data = open(self.new, "rb").read()
        self.assertEqual(entry["path"], BASELINE)
        self.assertEqual(entry["sha256"], hashlib.sha256(data).hexdigest())
        self.assertEqual(entry["approved_by"], "antoine")
        self.assertTrue(entry["signature"])
        # The id git gives the file: sha1 of "blob <size>\0" and the bytes.
        self.assertEqual(entry["blob"], hashlib.sha1(b"blob %d\0" % len(data) + data).hexdigest())
        review_path = os.path.join(self.checkout, "docs", "design", "reviews", datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%d") + ".json")
        review = json.load(open(review_path))
        self.assertEqual(review["decisions"][0]["decision"], "approve")
        self.assertEqual(review["decisions"][0]["baseline"], BASELINE)
        self.assertEqual(entry["review"], os.path.relpath(review_path, self.checkout))
        self.assertEqual(sorted(written["files"]), sorted([entry["review"], gallery.APPROVED]))
        # Approving again replaces the entry instead of adding one.
        gallery.decide(self.out, self.item()["id"], "approve", "again")
        self.assertEqual(len(json.load(open(os.path.join(self.checkout, gallery.APPROVED)))["baselines"]), 1)

    def test_reject_and_note_need_a_note_and_never_lock_a_baseline(self):
        self.build()
        with self.assertRaises(gallery.GalleryError):
            gallery.decide(self.out, self.item()["id"], "reject", " ")
        gallery.decide(self.out, self.item()["id"], "reject", "Counts too faint")
        self.assertFalse(os.path.exists(os.path.join(self.checkout, gallery.APPROVED)))
        with self.assertRaises(gallery.GalleryError):
            gallery.decide(self.out, "unknown", "note", "x")

    def test_an_image_changed_since_the_build_cannot_be_approved(self):
        self.build()
        self.png(os.path.join(self.checkout, BASELINE), (52, 199, 89))
        with self.assertRaises(gallery.GalleryError):
            gallery.decide(self.out, self.item()["id"], "approve", "")

    def test_a_pending_branch_approval_is_queued_for_its_commit(self):
        self.build([self.item(pending=True, branch="unit/U152")])
        gallery.decide(self.out, self.item()["id"], "approve", "")
        [line] = open(os.path.join(self.out, "pending-commits.jsonl")).read().splitlines()
        self.assertEqual(json.loads(line)["branch"], "unit/U152")

    def test_a_pick_is_logged_with_its_reason(self):
        picks = [{"screen": "home.deckList", "checkout": self.checkout, "variants": [{"name": "A", "image": self.new}, {"name": "B", "image": self.old}]}]
        self.build(picks=picks)
        with self.assertRaises(gallery.GalleryError):
            gallery.pick(self.out, "home.deckList", "B", "")
        gallery.pick(self.out, "home.deckList", "B", "Calmer | quieter counts")
        lines = open(os.path.join(self.checkout, "docs", "design", "decisions.md")).read().splitlines()
        self.assertRegex(lines[-1], r"^\| \d{4}-\d\d-\d\d \| home\.deckList \| Picked B \| Calmer / quieter counts \| gallery, reviews/")

    def test_the_server_records_decisions_from_its_own_page_only(self):
        self.build()
        handler = server.make_handler(self.out)
        httpd = server.ThreadingHTTPServer(("127.0.0.1", 0), handler)
        thread = threading.Thread(target=httpd.serve_forever, daemon=True)
        thread.start()
        base = "http://127.0.0.1:%d" % httpd.server_address[1]
        try:
            page = urllib.request.urlopen(base + "/").read().decode()
            self.assertIn("Meguri review gallery", page)
            with self.assertRaises(urllib.error.HTTPError) as caught:
                urllib.request.urlopen(base + "/../../etc/passwd")
            self.assertEqual(caught.exception.code, 404)

            def post(body, origin=None, content_type="application/json"):
                request = urllib.request.Request(base + "/api/decision", data=json.dumps(body).encode(), method="POST")
                request.add_header("Content-Type", content_type)
                if origin:
                    request.add_header("Origin", origin)
                return urllib.request.urlopen(request)

            with self.assertRaises(urllib.error.HTTPError) as caught:
                post({"item": self.item()["id"], "decision": "approve"}, origin="https://example.com")
            self.assertEqual(caught.exception.code, 403)
            with self.assertRaises(urllib.error.HTTPError) as caught:
                post({"item": self.item()["id"], "decision": "approve"}, content_type="text/plain")
            self.assertEqual(caught.exception.code, 415)
            answer = json.loads(post({"item": self.item()["id"], "decision": "approve"}, origin=base).read())
            self.assertEqual(answer["decision"], "approve")
            self.assertTrue(os.path.exists(os.path.join(self.checkout, gallery.APPROVED)))
        finally:
            httpd.shutdown()
            httpd.server_close()


if __name__ == "__main__":
    unittest.main()
