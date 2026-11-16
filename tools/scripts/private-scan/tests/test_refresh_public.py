"""Synthetic-only tests; run through the existing MG scanner discovery."""
import contextlib
import io
import json
import os
from pathlib import Path
import stat
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import private_scan as scan


def git(repo, *args):
    return subprocess.check_output(["git", "-C", str(repo), "-c", "user.name=Synthetic",
                                    "-c", "user.email=synthetic@localhost", *args], stderr=subprocess.PIPE)


class RefreshPublic(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.repo = Path(os.path.realpath(self.tmp.name)) / "repo"
        self.repo.mkdir()
        git(self.repo, "init", "-q")
        (self.repo / ".gitignore").write_text("build/\n")
        (self.repo / "public.txt").write_text("publicword\n")
        self.commit()
        self.parent = self.repo / "build/private/tokens"
        self.parent.mkdir(parents=True)
        (self.repo / "build/private").chmod(0o700)
        self.tokens = self.parent / "tokens.txt"
        self.tokens.write_bytes(b"publicword\nunknownword\n")
        self.tokens.chmod(0o600)

    def commit(self):
        git(self.repo, "add", "-A")
        git(self.repo, "commit", "-qm", "synthetic public snapshot")
        self.head = git(self.repo, "rev-parse", "HEAD").decode().strip()

    def refresh(self):
        return scan.refresh_public(str(self.repo), self.head)

    def test_head_only_private_results_masking_and_unknown_retention(self):
        fact = scan.PUBLIC_SWIFT_IMPORT
        module = fact.split(" ", 1)[1].casefold()
        (self.repo / "fixed.swift").write_text(fact + "\n")
        results = self.repo / "conformance/reports/private/result.txt"
        results.parent.mkdir(parents=True)
        results.write_text("resultword\n")
        self.commit()
        (self.repo / "public.txt").write_text("publicword dirtyword\n")
        (self.repo / "staged.txt").write_text("stagedword\n")
        git(self.repo, "add", "staged.txt")
        (self.repo / "untracked.txt").write_text("untrackedword\n")
        kept = sorted([module, "dirtyword", "stagedword", "untrackedword", "resultword", "unknownword"])
        self.tokens.write_text("publicword\n" + "\n".join(kept) + "\n")
        self.refresh()
        self.assertEqual(self.tokens.read_text().splitlines(), kept)

    def test_private_exact_backup_count_hash_only_output_and_untouched_metadata(self):
        before = self.tokens.read_bytes()
        metadata = self.parent / "meta.json"
        metadata.write_bytes(b"historical-build-metadata\xff\n")
        stdout = io.StringIO()
        with contextlib.redirect_stdout(stdout):
            rc = scan.main(["refresh-public", "--repo", str(self.repo), "--head", self.head])
        self.assertEqual(rc, 0)
        result = json.loads(stdout.getvalue())
        self.assertEqual((result["before"], result["removed"], result["after"]), (2, 1, 1))
        self.assertEqual(result["head"], self.head)
        self.assertEqual(result["before_sha256"], scan.hashlib.sha256(before).hexdigest())
        backup, = self.parent.glob("tokens.before-*")
        self.assertEqual(backup.read_bytes(), before)
        for path in (backup, self.tokens):
            self.assertEqual(stat.S_IMODE(path.stat().st_mode), 0o600)
        self.assertEqual(metadata.read_bytes(), b"historical-build-metadata\xff\n")
        self.assertNotIn("publicword", stdout.getvalue())
        self.assertNotIn("unknownword", stdout.getvalue())
        self.assertEqual(set(result), {"head", "before", "removed", "after", "before_sha256", "after_sha256"})

    def test_stale_short_or_moving_head_refuses_and_unknown_only_is_noop(self):
        before = self.tokens.read_bytes()
        for head in (self.head[:12], "0" * len(self.head)):
            with self.assertRaises(ValueError):
                scan.refresh_public(str(self.repo), head)
        original = scan.public_tokens

        def advance(*args):
            result = original(*args)
            (self.repo / "later.txt").write_text("unknownword\n")
            self.commit()
            return result

        with mock.patch.object(scan, "public_tokens", side_effect=advance):
            with self.assertRaises(ValueError):
                self.refresh()
        self.assertEqual(self.tokens.read_bytes(), before)
        self.assertEqual(list(self.parent.glob("tokens.before-*")), [])
        self.tokens.write_bytes(b"neverpublicword\n")
        self.assertEqual(self.refresh()["removed"], 0)
        self.assertEqual(list(self.parent.glob("tokens.before-*")), [])

    def test_shared_main_requires_opt_in_and_uses_linked_committed_head(self):
        linked = self.repo.parent / "linked"
        git(self.repo, "worktree", "add", "-q", "-b", "synthetic-linked", str(linked))
        (linked / "linked.txt").write_text("linkedpublicword\n")
        git(linked, "add", "linked.txt")
        git(linked, "commit", "-qm", "linked committed text")
        head = git(linked, "rev-parse", "HEAD").decode().strip()
        (self.repo / "public.txt").write_text("dirtymainword\n")
        (linked / "untracked.txt").write_text("dirtylinkedword\n")
        self.tokens.write_bytes(b"linkedpublicword\ndirtymainword\ndirtylinkedword\nunknownword\n")
        with self.assertRaises(ValueError):
            scan.refresh_public(str(linked), head)
        scan.refresh_public(str(linked), head, shared_main=True)
        self.assertEqual(self.tokens.read_bytes(), b"dirtylinkedword\ndirtymainword\nunknownword\n")
        self.assertEqual((self.repo / "public.txt").read_text(), "dirtymainword\n")
        self.assertFalse((linked / "build").exists())

    def test_unsafe_source_file_links_types_modes_and_owner_refuse(self):
        outside = self.repo.parent / "outside"
        outside.write_bytes(self.tokens.read_bytes())
        for kind in ("symlink", "hardlink", "fifo", "writable"):
            with self.subTest(kind=kind):
                self.tokens.unlink()
                if kind == "symlink":
                    self.tokens.symlink_to(outside)
                elif kind == "hardlink":
                    os.link(outside, self.tokens)
                elif kind == "fifo":
                    os.mkfifo(self.tokens)
                else:
                    self.tokens.write_bytes(outside.read_bytes())
                    self.tokens.chmod(0o666)
                with self.assertRaises((OSError, ValueError)):
                    self.refresh()
        self.tokens.unlink()
        self.tokens.write_bytes(outside.read_bytes())
        self.tokens.chmod(0o600)
        original = scan.os.getuid
        with mock.patch.object(scan.os, "getuid", return_value=original() + 1):
            with self.assertRaises(ValueError):
                self.refresh()
        self.assertEqual(list(self.parent.glob("tokens.before-*")), [])
        self.assertEqual(outside.read_bytes(), b"publicword\nunknownword\n")

    def test_unsafe_target_directory_and_existing_backup_symlink_refuse(self):
        private = self.repo / "build/private"
        private.chmod(0o755)
        with self.assertRaises(ValueError):
            self.refresh()
        private.chmod(0o700)
        moved = self.parent.with_name("moved")
        self.parent.rename(moved)
        self.parent.symlink_to(moved, target_is_directory=True)
        with self.assertRaises(OSError):
            self.refresh()
        self.parent.unlink()
        moved.rename(self.parent)
        outside = self.repo.parent / "outside"
        outside.write_bytes(b"untouched")
        (self.parent / ("tokens.before-" + "0" * 32)).symlink_to(outside)
        with mock.patch.object(scan.secrets, "token_hex", return_value="0" * 32):
            with self.assertRaises(FileExistsError):
                self.refresh()
        self.assertEqual(outside.read_bytes(), b"untouched")
        self.assertEqual(self.tokens.read_bytes(), b"publicword\nunknownword\n")

    def test_changed_source_or_failed_replace_never_overwrites_original(self):
        original = scan.public_tokens

        def mutate(*args):
            result = original(*args)
            self.tokens.write_bytes(b"changedunknownword\n")
            return result

        with mock.patch.object(scan, "public_tokens", side_effect=mutate):
            with self.assertRaises(ValueError):
                self.refresh()
        self.assertEqual(self.tokens.read_bytes(), b"changedunknownword\n")
        self.assertEqual(list(self.parent.glob("tokens.before-*")), [])
        self.tokens.write_bytes(b"publicword\nunknownword\n")
        with mock.patch.object(scan.os, "replace", side_effect=OSError("synthetic")):
            with self.assertRaises(OSError):
                self.refresh()
        self.assertEqual(self.tokens.read_bytes(), b"publicword\nunknownword\n")
        backup, = self.parent.glob("tokens.before-*")
        self.assertEqual(backup.read_bytes(), self.tokens.read_bytes())

    def test_invalid_token_encoding_never_reaches_error_output(self):
        self.tokens.write_bytes(b"\xffprivate-error-sentinel\n")
        stdout, stderr = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
            status = scan.main(["refresh-public", "--repo", str(self.repo), "--head", self.head])
        self.assertEqual(status, 2)
        self.assertEqual(stdout.getvalue(), "")
        self.assertNotIn("private-error-sentinel", stderr.getvalue())
