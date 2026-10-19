import os
from pathlib import Path
import subprocess
import tempfile
import unittest

from source_view import permitted, prepare, validate


class SourceViewTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name) / "repo"
        self.root.mkdir()
        self.target = Path(self.temp.name) / "view"
        self.git("init", "-q")

    def git(self, *args):
        return subprocess.check_output(["git", "-C", str(self.root), *args])

    def file(self, name, value="synthetic"):
        path = self.root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(value)
        return path

    def test_only_tracked_permitted_files_are_copied_without_history(self):
        self.file("core/lib.rs", "independent implementation")
        self.file("specs/example.md", "neutral facts")
        self.file("core/untracked.rs")
        for folder in ("oracle", "refs", "local-data", "build", ".codex", ".claude"):
            self.file(folder + "/sentinel", "not for implementation")
        self.git("add", "-A")
        self.git("reset", "--", "core/untracked.rs")
        report = prepare(self.root, self.target)
        self.assertEqual(report["files"], 2)
        self.assertEqual((self.target / "core/lib.rs").read_text(), "independent implementation")
        self.assertFalse((self.target / ".git").exists())
        self.assertEqual(sorted(p.name for p in self.target.iterdir()), ["core", "specs"])
        validate(self.target)

    def test_links_and_submodule_entries_fail_closed(self):
        self.file("core/allowed.rs")
        (self.root / "core/link.rs").symlink_to("allowed.rs")
        self.git("add", "-A")
        with self.assertRaises(ValueError):
            prepare(self.root, self.target)
        self.assertFalse(self.target.exists())

    def test_symlink_parent_is_rejected_before_reading(self):
        self.file("core/nested/lib.rs")
        self.git("add", "-A")
        (self.root / "core/nested/lib.rs").unlink()
        (self.root / "core/nested").rmdir()
        (self.root / "core/nested").symlink_to(self.root / "outside", target_is_directory=True)
        with self.assertRaises(ValueError):
            prepare(self.root, self.target)

    def test_hardlinks_are_rejected(self):
        path = self.file("core/lib.rs")
        os.link(path, self.root / "linked")
        self.git("add", "-A")
        with self.assertRaises(ValueError):
            prepare(self.root, self.target)

    def test_forbidden_components_and_traversal_are_rejected(self):
        for name in ("/core/a", "core/../oracle/a", "core/build/a", "app/.git/a", "core/refs/a"):
            self.assertFalse(permitted(name), name)
        self.assertTrue(permitted("core/crates/example/src/lib.rs"))

    def test_forbidden_or_linked_worker_output_is_rejected(self):
        self.target.mkdir()
        (self.target / "core").mkdir()
        (self.target / "core/lib.rs").write_text("allowed")
        (self.target / "core/link.rs").symlink_to("lib.rs")
        with self.assertRaises(ValueError):
            validate(self.target)
        (self.target / "core/link.rs").unlink()
        (self.target / "oracle").mkdir()
        (self.target / "oracle/forbidden.txt").write_text("not permitted")
        with self.assertRaises(ValueError):
            validate(self.target)

    def test_empty_sandbox_guards_have_no_patch_content(self):
        self.target.mkdir()
        for name in (".git", ".codex", ".agents", ".aws"):
            (self.target / name).mkdir()
        validate(self.target)
        # Empty directories have no Git patch representation. Files inside any
        # excluded directory must still fail, including nested empty parents.
        (self.target / ".aws/nested").mkdir()
        validate(self.target)
        env = dict(os.environ, GIT_DIR=str(self.root.parent / "shadow-git"), GIT_WORK_TREE=str(self.target))
        subprocess.run(["git", "init", "-q"], env=env, cwd=self.target, check=True)
        subprocess.run(["git", "add", "-f", "-A"], env=env, cwd=self.target, check=True)
        self.assertEqual(subprocess.check_output(["git", "ls-files", "-z"], env=env, cwd=self.target), b"")
        (self.target / ".aws/nested/credentials").write_text("synthetic")
        with self.assertRaises(ValueError):
            validate(self.target)


if __name__ == "__main__":
    unittest.main()
