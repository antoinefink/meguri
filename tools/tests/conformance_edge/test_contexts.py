"""Launch authority survives the fixture process and scopes parent cleanup."""
import importlib.util
import json
import os
import pathlib
import shutil
import subprocess
import sys
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[3]
spec = importlib.util.spec_from_file_location("owner_contexts", ROOT / "tools/mg.d/conformance-owners.py")
contexts = importlib.util.module_from_spec(spec)
spec.loader.exec_module(contexts)


class ParentContexts(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = pathlib.Path(self.temporary.name)
        self.run_dir = self.root / "build/mg/conformance-route-test/current-run"
        self.run_dir.mkdir(parents=True)
        helper_dir = self.root / "tools/mg.d"
        helper_dir.mkdir(parents=True)
        shutil.copy2(ROOT / "tools/mg.d/conformance-owners.py", helper_dir)
        self.owners = [c * 32 for c in "abcd"]

    def register(self, owner, **changes):
        fields = dict(root=self.root, project="current-project", run="current-run",
                      run_dir=self.run_dir, owner=owner, image="test-image", faults="sync-faults:8474")
        fields.update(changes)
        contexts.register(**fields)

    def cleanup(self, failed=""):
        # Exercise the production shell consumer with the edge operation as
        # its sole fake. The registry parser and its scope checks remain real.
        script = r'''
mg_job() { :; }
. "$1"
mg_compose_project() { echo current-project; }
mg_failure() { echo "$1" >>"$MG_RUN_DIR/failures"; }
mg_status() { echo "$1" >"$MG_RUN_DIR/status"; }
mg__remove_unreserved_run_containers() { echo generic >>"$MG_RUN_DIR/calls"; }
python3() {
  if [ "$3" = "$MG_ROOT/tools/mg.d/conformance-edge.py" ]; then
    while [ "$#" -gt 0 ] && [ "$1" != --owner ]; do shift; done
    shift
    echo "$1" >>"$MG_RUN_DIR/calls"
    [ "$1" != "$FAILED_OWNER" ]
  else
    "$PYTHON_BIN" "$@"
  fi
}
mg_conf__cleanup
'''
        env = dict(os.environ, MG_ROOT=str(self.root), MG_RUN_ID="current-run", MG_RUN_DIR=str(self.run_dir),
                   FAILED_OWNER=failed, PYTHON_BIN=sys.executable)
        result = subprocess.run(["bash", "-u", "-c", script, "context-test", str(ROOT / "tools/mg.d/conformance.sh")],
                                env=env, capture_output=True, timeout=5)
        path = self.run_dir / "calls"
        return result, path.read_text().splitlines() if path.exists() else []

    def test_parent_consumes_additional_owners_after_registration_process_exits(self):
        for owner in self.owners[:2]:
            self.register(owner)
        # Abrupt process exit cannot erase registrations for the extra owners.
        code = r'''
import importlib.util,os,sys
spec=importlib.util.spec_from_file_location('contexts',sys.argv[1])
m=importlib.util.module_from_spec(spec); spec.loader.exec_module(m)
for owner in sys.argv[4:]:
 m.register(sys.argv[2],'current-project','current-run',sys.argv[3],owner,'test-image','sync-faults:8474')
os._exit(7)
'''
        result = subprocess.run([sys.executable, "-I", "-B", "-c", code,
                                 str(ROOT / "tools/mg.d/conformance-owners.py"), str(self.root), str(self.run_dir),
                                 *self.owners[2:]], capture_output=True, timeout=5)
        self.assertEqual(result.returncode, 7)
        result, calls = self.cleanup()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(calls, self.owners + ["generic"])

    def test_failed_extra_owner_keeps_context_and_blocks_generic_cleanup(self):
        for owner in self.owners:
            self.register(owner)
        before = {p.name: p.read_bytes() for p in (self.run_dir / "edge-owners").glob("*.json")}
        result, calls = self.cleanup(failed=self.owners[2])
        self.assertEqual(result.returncode, 1)
        self.assertEqual(calls, self.owners)
        self.assertEqual((self.run_dir / "status").read_text().strip(), "error")
        self.assertEqual((self.run_dir / "failures").read_text().splitlines(), ["live/edge-cleanup"])
        self.assertEqual(before, {p.name: p.read_bytes() for p in (self.run_dir / "edge-owners").glob("*.json")})

    def test_foreign_scope_cannot_become_parent_authority(self):
        for field, replacement in (("root", "/another-checkout"), ("project", "another-project"),
                                   ("run", "another-run"), ("owner", "f" * 32)):
            with self.subTest(field=field):
                self.register(self.owners[0])
                path = self.run_dir / "edge-owners" / (self.owners[0] + ".json")
                value = json.loads(path.read_text())
                value[field] = replacement
                path.write_text(json.dumps(value))
                result, calls = self.cleanup()
                self.assertEqual(result.returncode, 1)
                self.assertEqual(calls, [])
                self.assertEqual(json.loads(path.read_text()), value)
                path.unlink()

    def test_owner_registration_is_immutable_and_not_a_report(self):
        self.register(self.owners[0])
        self.register(self.owners[0])
        with self.assertRaises(contexts.ContextError):
            self.register(self.owners[0], project="another-project")
        records = contexts.records(self.root, "current-project", "current-run", self.run_dir)
        self.assertEqual([record["owner"] for record in records], self.owners[:1])
        self.assertEqual(list(self.run_dir.iterdir()), [self.run_dir / "edge-owners"])

    def test_foreign_project_resources_survive_current_owner_cleanup(self):
        spec = importlib.util.spec_from_file_location("edge_model", ROOT / "tools/tests/conformance_edge/test_edge.py")
        model = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(model)
        current = model.FakeEdges()
        current.allocate()
        foreign = model.FakeEdges(run="separate-run", owner="e" * 32)
        foreign.project = "separate-project"
        foreign.labels["meguri.mg.project"] = foreign.project
        for resource in foreign.resources.values():
            resource["Config"]["Labels"]["meguri.mg.project"] = foreign.project
            networks = resource["NetworkSettings"]["Networks"]
            networks[foreign.project + "_oracle-internal"] = networks.pop("test-project_oracle-internal")
        current.resources.update(foreign.resources)
        current.proxies.update(foreign.proxies)
        current.sweep()
        self.assertEqual(current.resources, foreign.resources)
        self.assertEqual(current.proxies, foreign.proxies)
