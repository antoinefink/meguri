import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

from source_view import permitted, prepare, seed, validate


CONFIG_STORE_PATHS = tuple(
    prefix + ".config/" + suffix
    for prefix in ("core/", "server/", "server/web/nested/")
    for suffix in ("gcloud/application_default_credentials.json", "gh/hosts.yml",
                   "glab-cli/config.yml", "rclone/rclone.conf", "future-tool/auth.json",
                   "future-tool/nextest.toml", "credentials", "nextest.toml.bak")
)
UNAUDITED_CONFIG_PATHS = (
    "server/.config/nextest.toml",
    "server/web/nested/.config/nextest.toml",
    "core/nested/.config/nextest.toml",
    "core/.config/nested/nextest.toml",
)


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
        for name in ("/core/a", "core/../oracle/a", "core/build/a", "app/.git/a", "core/refs/a",
                     "server/.config/nextest.toml/credentials",
                     "server/.config/.config/nextest.toml"):
            self.assertFalse(permitted(name), name)
        self.assertTrue(permitted("core/crates/example/src/lib.rs"))

    def test_server_source_keeps_nested_quarantine_and_credentials_out(self):
        allowed = ("server/src/main.rs", "server/web/src/Login.tsx", "server/web/src/Icon.jsx",
                   "core/.config/nextest.toml", "server/web/config/settings.json",
                   "server/web/vite.config.ts")
        for name in allowed:
            self.assertTrue(permitted(name))
            self.file(name)
        denied = ["server/" + part + "/sentinel" for part in (
            "oracle", "refs", "local-data", "build", ".git", ".codex", ".claude",
            ".aws", ".ssh", ".azure", ".docker", ".kube", ".agents", "web/node_modules",
        )] + ["server/web/" + part for part in (
            ".env", ".env.production", ".npmrc", ".netrc", ".gitmodules", ".git-credentials",
        )] + ["server/.config/gcloud/application_default_credentials.json",
              "server/.agents/skills/example/SKILL.md",
              "server/web/nested/.config/gcloud/credentials.db",
              "server/web/nested/.agents/settings.json"] + list(CONFIG_STORE_PATHS + UNAUDITED_CONFIG_PATHS)
        for name in denied:
            self.assertFalse(permitted(name))
            self.file(name)
        self.git("add", "-A")
        report = prepare(self.root, self.target)
        self.assertEqual(report["files"], len(allowed))
        self.assertFalse(report["real_history"])
        self.assertIn(".config", report["credential_trees"])
        self.assertEqual(report["source_config_files"], ["core/.config/nextest.toml"])
        self.assertEqual(sorted(p.relative_to(self.target).as_posix()
                                for p in self.target.rglob("*") if p.is_file()), sorted(allowed))
        validate(self.target)

    def test_export_refuses_nested_credentials_and_agent_configuration(self):
        self.target.mkdir()
        for name in CONFIG_STORE_PATHS + UNAUDITED_CONFIG_PATHS + ("server/.agents/skills/example/SKILL.md",
                     "server/web/nested/.config/gcloud/credentials.db",
                     "server/web/nested/.agents/settings.json"):
            with self.subTest(name=name):
                path = self.target / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text("synthetic excluded input")
                with self.assertRaises(ValueError):
                    validate(self.target)
                path.unlink()
        # Empty guard directories remain exportable; source files still work.
        (self.target / "server/main.rs").write_text("fn main() {}")
        for name in ("core/.config/nextest.toml", "server/web/config/settings.json"):
            config = self.target / name
            config.parent.mkdir(parents=True, exist_ok=True)
            config.write_text("synthetic source configuration\n")
        validate(self.target)

    def test_server_links_hardlinks_and_submodules_fail_before_copy(self):
        path = self.file("server/src/main.rs")
        self.git("add", "-A")
        os.link(path, self.root / "alias")
        with self.assertRaises(ValueError):
            prepare(self.root, self.target)
        (self.root / "alias").unlink()
        path.unlink()
        path.symlink_to(self.root / "outside")
        with self.assertRaises(ValueError):
            prepare(self.root, self.target)
        path.unlink()
        path.write_text("own code")
        tree = self.git("write-tree").decode().strip()
        commit = self.git("-c", "user.name=test", "-c", "user.email=test@localhost",
                          "commit-tree", tree, "-m", "synthetic submodule").decode().strip()
        self.git("update-index", "--add", "--cacheinfo", "160000," + commit + ",server/module")
        with self.assertRaises(ValueError):
            prepare(self.root, self.target)
        self.assertFalse(self.target.exists())

    def test_server_seed_and_output_use_the_same_boundary(self):
        self.apply_seed("server/web/src/Screen.tsx")
        self.assertTrue((self.target / "server/web/src/Screen.tsx").is_file())
        self.file("server/refs/blocked.rs")
        blocked = self.target / "server/.env"
        blocked.write_text("synthetic")
        with self.assertRaises(ValueError):
            validate(self.target)

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

    def apply_seed(self, name, mode="100644", value="own code"):
        self.target.mkdir()
        env = dict(os.environ, GIT_DIR=str(self.root.parent / "seed-git"), GIT_WORK_TREE=str(self.target))
        subprocess.run(["git", "init", "-q"], env=env, cwd=self.target, check=True)
        source_patch = self.root.parent / "seed.patch"
        source_patch.write_text(
            f"diff --git a/{name} b/{name}\nnew file mode {mode}\n"
            f"--- /dev/null\n+++ b/{name}\n@@ -0,0 +1 @@\n+{value}\n"
        )
        with patch.dict(os.environ, env, clear=True):
            seed(self.target, source_patch)

    def test_own_patch_is_available_without_real_history(self):
        self.apply_seed("app/example.swift")
        self.assertEqual((self.target / "app/example.swift").read_text(), "own code\n")
        self.assertFalse((self.target / ".git").exists())

    def test_seed_refuses_forbidden_output_before_model_access(self):
        with self.assertRaises(ValueError):
            self.apply_seed("app/refs/example.txt")

    def test_seed_refuses_unknown_config_store_before_model_access(self):
        with self.assertRaises(ValueError):
            self.apply_seed("server/web/.config/future-tool/auth.json")

    def test_seed_accepts_audited_config(self):
        self.apply_seed("core/.config/nextest.toml")
        self.assertEqual((self.target / "core/.config/nextest.toml").read_text(), "own code\n")

    def test_seed_accepts_ordinary_config_outside_config_store(self):
        self.apply_seed("server/web/config/settings.json")
        self.assertEqual((self.target / "server/web/config/settings.json").read_text(), "own code\n")

    def test_seed_refuses_unaudited_config_before_model_access(self):
        for name in UNAUDITED_CONFIG_PATHS:
            with self.subTest(name=name), tempfile.TemporaryDirectory() as temp:
                self.target = Path(temp) / "view"
                with self.assertRaises(ValueError):
                    self.apply_seed(name)

    def test_seed_refuses_links_before_model_access(self):
        with self.assertRaises(ValueError):
            self.apply_seed("app/link", mode="120000", value="/outside/synthetic")

    def test_seed_refuses_path_traversal(self):
        with self.assertRaises(subprocess.CalledProcessError):
            self.apply_seed("../outside.txt")
        self.assertFalse((self.root.parent / "outside.txt").exists())


if __name__ == "__main__":
    unittest.main()
