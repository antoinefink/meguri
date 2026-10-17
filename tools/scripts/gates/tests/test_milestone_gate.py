"""Tests for tools/scripts/gates/milestone_gate.py (U240), through its command
line: a throwaway git repository holds the units' commits and the user-step
evidence, a results directory holds tools/mg job results, and the manifest and
tiers config are the JSON that `milestone_gate.py load` and `gate.py load`
produce. Expected outcomes come from the plan's U240 test scenarios.

Run: python3 -m unittest discover -s tools/scripts/gates/tests
"""

import copy
import importlib.util
import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
GATE = os.path.join(os.path.dirname(HERE), "milestone_gate.py")
TIERS_GATE = os.path.join(os.path.dirname(os.path.dirname(os.path.dirname(HERE))), "gate", "gate.py")
NOW = "2026-11-20T12:00:00Z"
MS = ("M0", "M1R", "M1", "M2", "M3", "M4", "M5")  # the manifest's order, as the plan's table has it
FOLDABLE = {"job": "sim-setup", "args": [], "detail": "foldable",
            "equals": {"status": "unavailable", "reason": "toolchain"}}
ENV = dict(os.environ, GIT_AUTHOR_NAME="t", GIT_AUTHOR_EMAIL="t@example.invalid", GIT_COMMITTER_NAME="t",
           GIT_COMMITTER_EMAIL="t@example.invalid", GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull)


def tiers_quarantine():
    """gate.py's own quarantine loader, so invalid entries are judged as U19 judges them."""
    spec = importlib.util.spec_from_file_location("tiers_gate", TIERS_GATE)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod.load_quarantine


def run_def(rid, job=None, args=None, tier="T0", cls="unit", **extra):
    r = {"id": rid, "job": job or rid, "args": list(args or []), "tier": tier, "class": cls, "require": [],
         "pending": None, "disabled": None, "release_only": False, "user_step": None}
    r.update(extra)
    return r


def manifest(**per_milestone):
    """A valid manifest: every milestone empty but for what the caller gives
    (a dict of milestone fields per milestone id). Its plan texts are empty,
    so no suite or user step needs a plan excerpt."""
    out = {"version": 1, "plan": "plan.md", "evidence_paths": ["docs/process/milestone-records/"],
           "budget_devices": {"R1822": ["ipad", "mac"]},
           "global": [{"id": "quarantine", "kind": "quarantine"}], "milestones": []}
    for m in MS:
        entry = {"id": m, "name": "Milestone " + m, "plan": {"gate": "", "user_steps": ""}, "units": [],
                 "suites": [], "user_steps": [], "deferred": []}
        entry.update(per_milestone.get(m, {}))
        out["milestones"].append(entry)
    return out


def milestone(man, mid):
    return [m for m in man["milestones"] if m["id"] == mid][0]


class Sandbox(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.mkdtemp(prefix="milestone-gate-")
        self.addCleanup(shutil.rmtree, self.tmp)
        self.root = os.path.join(self.tmp, "repo")
        self.results = os.path.join(self.tmp, "results")
        os.makedirs(self.root)
        self.git("init", "-q", "-b", "main")
        self.base = self.commit("Base", {"README.md": "meguri\n"})
        self.seq = 0
        self.config = {"runs": [], "quarantine": {"entries": [], "invalid": []}}

    def git(self, *args):
        return subprocess.run(["git", "-C", self.root] + list(args), check=True, env=ENV,
                              stdout=subprocess.PIPE, stderr=subprocess.PIPE).stdout.decode().strip()

    def commit(self, message, files=None, trailers=""):
        for path, content in (files or {}).items():
            full = os.path.join(self.root, path)
            os.makedirs(os.path.dirname(full), exist_ok=True)
            with open(full, "w", encoding="utf-8") as f:
                f.write(content if isinstance(content, str) else json.dumps(content))
        self.git("add", "-A")
        self.git("commit", "-q", "--allow-empty", "-m", message + ("\n\n" + trailers if trailers else ""))
        return self.git("rev-parse", "HEAD")

    def merge_units(self, *units, role="implementer"):
        for u in units:
            self.commit("Add %s" % u, trailers="Unit: %s\nRole: %s" % (u, role))
        return self.git("rev-parse", "HEAD")

    def run_(self, rid, **kw):
        self.config["runs"].append(run_def(rid, **kw))

    def result(self, job, args=(), status="pass", sha=None, dirty=False, details=None, summary=None,
               failures=None, dry_run=False, artifacts=()):
        """Writes a result; `artifacts` are file names written into its run
        directory and listed as its artifacts."""
        self.seq += 1
        run_id = "20261120T1000%02dZ-%d" % (self.seq, 100 + self.seq)
        path = os.path.join(self.results, job, run_id, "result.json")
        os.makedirs(os.path.dirname(path))
        listed = []
        for name in artifacts:
            listed.append({"kind": "report", "path": os.path.join(os.path.dirname(path), name)})
            with open(listed[-1]["path"], "w", encoding="utf-8") as f:
                f.write("{}\n")
        with open(path, "w", encoding="utf-8") as f:
            json.dump({"schema_version": 1, "job": job, "run_id": run_id, "status": status,
                       "exit_code": {"pass": 0, "warn": 0, "fail": 1, "error": 2}[status], "placement": "host",
                       "service": None, "dry_run": dry_run, "args": list(args), "root": self.root,
                       "started_at": "2026-11-20T10:00:%02dZ" % self.seq, "finished_at": "2026-11-20T10:01:00Z",
                       "duration_ms": 1000, "commit": {"sha": sha or self.git("rev-parse", "HEAD"), "dirty": dirty},
                       "message": "%s %s" % (job, status),
                       "summary": summary or {"total": 3, "passed": 3, "failed": 0, "skipped": 0},
                       "failures": failures or [], "artifacts": listed, "user_steps": [], "details": details or {}}, f)
        return path

    def evaluate(self, man, milestone, doctor=None, extra=()):
        paths = {}
        for name, value in (("manifest", man), ("config", self.config)):
            paths[name] = os.path.join(self.tmp, name + ".json")
            with open(paths[name], "w", encoding="utf-8") as f:
                json.dump(value, f)
        out = os.path.join(self.tmp, "evaluation.json")
        if os.path.exists(out):
            os.remove(out)
        self.record = os.path.join(self.root, "docs/process/milestone-records/%s.json" % milestone)
        cmd = [sys.executable, "-B", GATE, "evaluate", "--manifest", paths["manifest"], "--config", paths["config"],
               "--milestone", milestone, "--root", self.root, "--results", self.results, "--now", NOW,
               "--record", self.record, "--out", out] + list(extra)
        if doctor is not None:
            dpath = os.path.join(self.tmp, "doctor.json")
            with open(dpath, "w", encoding="utf-8") as f:
                json.dump(doctor, f)
            cmd += ["--doctor", dpath]
        p = subprocess.run(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=ENV)
        if p.returncode == 2:
            return 2, json.loads(p.stdout.decode() or "{}")
        self.assertIn(p.returncode, (0, 1, 3), p.stderr.decode())
        with open(out, encoding="utf-8") as f:
            return p.returncode, json.load(f)

    def check(self, man, plan_text):
        with open(os.path.join(self.root, "plan.md"), "w", encoding="utf-8") as f:
            f.write(plan_text)
        paths = {}
        for name, value in (("manifest", man), ("config", self.config)):
            paths[name] = os.path.join(self.tmp, name + ".json")
            with open(paths[name], "w", encoding="utf-8") as f:
                json.dump(value, f)
        p = subprocess.run([sys.executable, "-B", GATE, "check", "--manifest", paths["manifest"], "--config",
                            paths["config"], "--root", self.root], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        return p.returncode, json.loads(p.stdout.decode())

    def failure_ids(self, ev):
        return [f["id"] for f in ev["failures"]]

    def failure(self, ev, fid):
        hits = [f["message"] for f in ev["failures"] if f["id"] == fid]
        self.assertTrue(hits, "no failure %s in %s" % (fid, json.dumps(ev["failures"], indent=1)))
        return hits[0]


def device_run(milestone, device, build, budgets, fail=()):
    return {"milestone": milestone, "device": device, "build_commit": build,
            "budgets": [{"id": b, "status": "fail" if b in fail else "pass", "p95_ms": 100} for b in budgets]}


M1_BUDGETS = ["R1818", "R1822", "R1823"]


class SuiteTests(Sandbox):
    def test_missing_suite_result_reports_missing_and_exits_non_zero(self):
        self.run_("u246-conformance", job="conformance-aggregate", tier="T3", cls="conformance")
        man = manifest(M1={"suites": [{"id": "u246-conformance", "run": "u246-conformance"}]})
        rc, ev = self.evaluate(man, "M1")
        self.assertEqual(rc, 1)
        self.assertEqual(ev["outcome"], "fail")
        suite = [s for s in ev["suites"] if s["id"] == "u246-conformance"][0]
        self.assertEqual(suite["outcome"], "missing")
        self.assertIn("missing", self.failure(ev, "suite:u246-conformance"))
        self.assertFalse(os.path.exists(self.record))

    def test_all_pass_writes_the_record(self):
        self.run_("unit", job="core-test")
        self.run_("static", job="static-checks", args=["--base", "{base}"], cls="static")
        sha = self.git("rev-parse", "HEAD")
        self.result("core-test")
        self.result("static-checks", ["--base", sha])
        man = manifest(M0={"suites": [{"id": "unit", "run": "unit"}, {"id": "static", "run": "static"}]})
        rc, ev = self.evaluate(man, "M0")
        self.assertEqual((rc, ev["outcome"]), (0, "pass"), ev["failures"])
        self.assertEqual(ev["gate_commit"], sha)
        with open(self.record, encoding="utf-8") as f:
            record = json.load(f)
        self.assertEqual(record["gate_commit"], sha)
        self.assertEqual(record["closed_at"], NOW)
        self.assertEqual([s["id"] for s in record["suites"]], ["unit", "static"])
        self.assertEqual(record["suites"][0]["summary"]["passed"], 3)

    def test_outside_the_main_checkout_a_milestone_never_closes(self):
        # A unit branch carries its own `Unit:` trailers, so its units look merged there.
        self.run_("unit", job="core-test")
        self.merge_units("U240")
        self.result("core-test")
        man = manifest(M0={"suites": [{"id": "unit", "run": "unit"}], "units": ["U240"]})
        rc, ev = self.evaluate(man, "M0", extra=["--outside-main"])
        self.assertEqual((rc, ev["outcome"]), (1, "fail"))
        self.assertEqual(self.failure_ids(ev), ["checkout"])
        self.assertIn("main checkout", self.failure(ev, "checkout"))
        self.assertFalse(os.path.exists(self.record))
        rc, ev = self.evaluate(man, "M0")
        self.assertEqual(rc, 0, ev["failures"])

    def test_placeholders_match_the_results_own_commit(self):
        self.run_("static", job="static-checks", args=["--base", "{base}"], cls="static")
        self.commit("Later")
        # A merge gate's run measured from an older base, not the milestone's.
        self.result("static-checks", ["--base", self.base])
        man = manifest(M0={"suites": [{"id": "static", "run": "static"}]})
        rc, ev = self.evaluate(man, "M0")
        self.assertEqual(rc, 1)
        self.assertEqual(ev["suites"][0]["outcome"], "missing")

    def test_dry_runs_and_other_args_are_not_results(self):
        self.run_("conf", job="conformance", args=["--lane", "all"], cls="conformance")
        self.result("conformance", ["--lane", "all"], dry_run=True)
        self.result("conformance", ["--lane", "all", "sched"])
        man = manifest(M0={"suites": [{"id": "conf", "run": "conf"}]})
        rc, ev = self.evaluate(man, "M0")
        self.assertEqual(ev["suites"][0]["outcome"], "missing")

    def test_latest_result_counts(self):
        self.run_("unit", job="core-test")
        self.result("core-test")
        self.result("core-test", status="fail", failures=[{"id": "t1", "message": "boom", "repro": "x"}])
        man = manifest(M0={"suites": [{"id": "unit", "run": "unit"}]})
        rc, ev = self.evaluate(man, "M0")
        self.assertEqual(rc, 1)
        self.assertIn("t1", self.failure(ev, "suite:unit"))

    def test_results_from_two_commits_fail(self):
        self.run_("a", job="job-a")
        self.run_("b", job="job-b")
        a = self.git("rev-parse", "HEAD")
        self.result("job-a")
        b = self.commit("Change code", {"core/x.rs": "fn x() {}\n"})
        self.result("job-b")
        man = manifest(M0={"suites": [{"id": "a", "run": "a"}, {"id": "b", "run": "b"}]})
        rc, ev = self.evaluate(man, "M0")
        self.assertEqual(rc, 1)
        message = self.failure(ev, "results-commit")
        self.assertIn("results not from one commit", message)
        self.assertIn(a[:12], message)
        self.assertIn(b[:12], message)

    def test_dirty_result_fails(self):
        self.run_("unit", job="core-test")
        self.result("core-test", dirty=True)
        man = manifest(M0={"suites": [{"id": "unit", "run": "unit"}]})
        rc, ev = self.evaluate(man, "M0")
        self.assertEqual(rc, 1)
        self.assertEqual(ev["suites"][0]["outcome"], "dirty")
        self.assertIn("dirty", self.failure(ev, "suite:unit"))

    def test_code_commit_after_the_gate_commit_fails(self):
        self.run_("unit", job="core-test")
        self.result("core-test")
        self.commit("Change code", {"core/x.rs": "fn x() {}\n"})
        man = manifest(M0={"suites": [{"id": "unit", "run": "unit"}]})
        rc, ev = self.evaluate(man, "M0")
        self.assertEqual(rc, 1)
        self.assertIn("core/x.rs", self.failure(ev, "after-gate"))

    def test_queued_result_fails(self):
        self.run_("ui", job="xcode-check")
        self.result("xcode-check", status="warn", details={"queued": True})
        man = manifest(M0={"suites": [{"id": "ui", "run": "ui"}]})
        rc, ev = self.evaluate(man, "M0")
        self.assertEqual(rc, 1)
        self.assertEqual(ev["suites"][0]["outcome"], "queued")

    def test_suite_awaiting_user_counts_as_awaiting(self):
        self.run_("release-gate", tier="T3", cls="static")
        self.result("release-gate", status="warn", details={"outcome": "awaiting_user"})
        man = manifest(M5={"suites": [{"id": "release-gate", "run": "release-gate"}]})
        rc, ev = self.evaluate(man, "M5")
        self.assertEqual((rc, ev["outcome"]), (3, "awaiting_user"), ev["failures"])
        self.assertEqual([a["id"] for a in ev["awaiting"]], ["suite:release-gate"])

    def test_a_warn_that_is_neither_queued_nor_awaiting_fails(self):
        # A simulator job that cannot get its simulator ends warn with the lane
        # unavailable: nothing ran.
        self.run_("perf-smoke", tier="T3", cls="perf")
        self.result("perf-smoke", status="warn",
                    details={"lane": {"kind": "iphone", "status": "unavailable", "reason": "toolchain"}})
        man = manifest(M0={"suites": [{"id": "perf-smoke", "run": "perf-smoke"}]})
        rc, ev = self.evaluate(man, "M0")
        self.assertEqual((rc, ev["outcome"]), (1, "fail"))
        self.assertEqual(ev["suites"][0]["outcome"], "unavailable")
        self.assertIn("unavailable", self.failure(ev, "suite:perf-smoke"))
        # A pass with notes is not a pass either.
        self.result("perf-smoke", status="warn", details={})
        rc, ev = self.evaluate(man, "M0")
        self.assertEqual(rc, 1)
        self.assertEqual(ev["suites"][0]["outcome"], "warn")
        self.assertIn("warn", self.failure(ev, "suite:perf-smoke"))
        self.result("perf-smoke")
        rc, ev = self.evaluate(man, "M0")
        self.assertEqual(rc, 0, ev["failures"])

    def harvest_runs(self):
        self.run_("live", job="live-oracle", tier="T1", cls="sync")
        self.run_("harvest", job="harvest-report", args=["--live-report", "{artifact:live:t1-live.json}"],
                  tier="T3", cls="conformance")
        return manifest(M0={"suites": [{"id": "live", "run": "live"}, {"id": "harvest", "run": "harvest"}]})

    def test_an_artifact_argument_is_the_file_of_the_result_the_gate_reads(self):
        man = self.harvest_runs()
        live = os.path.join(os.path.dirname(self.result("live-oracle", artifacts=["t1-live.json"])), "t1-live.json")
        elsewhere = os.path.join(self.tmp, "copy", "t1-live.json")
        self.result("harvest-report", ["--live-report", elsewhere])
        rc, ev = self.evaluate(man, "M0")
        self.assertEqual(rc, 1)
        self.assertEqual([s["outcome"] for s in ev["suites"]], ["pass", "missing"])
        self.assertIn("t1-live.json of the live result", self.failure(ev, "suite:harvest"))
        self.result("harvest-report", ["--live-report", live])
        rc, ev = self.evaluate(man, "M0")
        self.assertEqual(rc, 0, ev["failures"])

    def test_an_artifact_from_another_commit_does_not_count(self):
        man = self.harvest_runs()
        live_a = os.path.join(os.path.dirname(self.result("live-oracle", artifacts=["t1-live.json"])), "t1-live.json")
        self.commit("Change code", {"core/x.rs": "fn x() {}\n"})
        # A harvest report made on commit B from commit A's live report.
        self.result("harvest-report", ["--live-report", live_a])
        rc, ev = self.evaluate(man, "M0")
        self.assertEqual(rc, 1)
        self.assertEqual(ev["suites"][1]["outcome"], "missing")
        # B's own live run is now the one the gate reads; A's report still does not count.
        live_b = os.path.join(os.path.dirname(self.result("live-oracle", artifacts=["t1-live.json"])), "t1-live.json")
        self.result("harvest-report", ["--live-report", live_a])
        rc, ev = self.evaluate(man, "M0")
        self.assertEqual(ev["suites"][1]["outcome"], "missing")
        self.result("harvest-report", ["--live-report", live_b])
        rc, ev = self.evaluate(man, "M0")
        self.assertEqual(rc, 0, ev["failures"])

    def test_pending_suite_fails_and_says_who_lands_it(self):
        man = manifest(M1={"suites": [{"id": "u243", "run": "u243-review-loop", "pending": "U243"}]})
        rc, ev = self.evaluate(man, "M1")
        self.assertEqual(rc, 1)
        self.assertEqual(ev["suites"][0]["outcome"], "pending")
        self.assertIn("U243", self.failure(ev, "suite:u243"))

    def test_pending_tiers_run_keeps_the_suite_pending(self):
        self.run_("perf-gate", cls="perf", pending="U241")
        man = manifest(M0={"suites": [{"id": "perf-gate", "run": "perf-gate"}]})
        rc, ev = self.evaluate(man, "M0")
        self.assertEqual(ev["suites"][0]["outcome"], "pending")
        self.assertIn("U241", self.failure(ev, "suite:perf-gate"))

    def test_scope_items_must_each_pass(self):
        self.run_("agg", job="conformance-aggregate", tier="T3", cls="conformance")
        man = manifest(M1={"suites": [{"id": "agg", "run": "agg",
                                       "scope": {"detail": "areas", "items": ["storage", "sched"]}}]})
        self.result("conformance-aggregate", details={"areas": {"storage": {"status": "pass"}, "sched": "fail"}})
        rc, ev = self.evaluate(man, "M1")
        self.assertEqual(rc, 1)
        self.assertIn("sched: fail", self.failure(ev, "suite:agg"))
        self.result("conformance-aggregate", details={})
        rc, ev = self.evaluate(man, "M1")
        self.assertIn("details.areas is missing", self.failure(ev, "suite:agg"))
        self.result("conformance-aggregate", details={"areas": {"storage": "pass", "sched": {"status": "pass"},
                                                                "search": "fail"}})
        rc, ev = self.evaluate(man, "M1")
        self.assertEqual(rc, 0, ev["failures"])

    def test_require_rules_of_the_run_and_the_suite_apply(self):
        self.run_("harvest", job="harvest-report", tier="T3", cls="conformance",
                  require=[{"id": "R1880-unclassified", "summary": "unclassified", "max": 0},
                           {"id": "R1880-in-scope", "detail": "harvest.in", "only_keys": ["pass"], "release": True}])
        self.result("harvest-report", summary={"total": 1, "passed": 1, "failed": 0, "skipped": 0, "unclassified": 2})
        man = manifest(M0={"suites": [{"id": "harvest", "run": "harvest"}]})
        rc, ev = self.evaluate(man, "M0")
        self.assertIn("R1880-unclassified", self.failure(ev, "suite:harvest"))
        # The release-only rule does not apply at a milestone; the M5 suite adds it.
        self.result("harvest-report", summary={"total": 1, "passed": 1, "failed": 0, "skipped": 0, "unclassified": 0},
                    details={"harvest": {"in": {"pass": 5, "fail": 1}}})
        rc, ev = self.evaluate(man, "M4")
        self.assertEqual(rc, 0, ev["failures"])
        man["milestones"][5]["suites"] = [{"id": "harvest", "run": "harvest", "require": [
            {"id": "R1880-in-scope", "detail": "harvest.in", "only_keys": ["pass"]}]}]
        rc, ev = self.evaluate(man, "M5")
        self.assertIn("1 fail", self.failure(ev, "suite:harvest"))

    def test_gates_are_cumulative(self):
        self.run_("a", job="job-a")
        self.run_("b", job="job-b")
        man = manifest(M1={"suites": [{"id": "a", "run": "a"}]}, M2={"suites": [{"id": "b", "run": "b"}]})
        self.result("job-b")
        rc, ev = self.evaluate(man, "M2")
        self.assertEqual(rc, 1)
        self.assertEqual([s["id"] for s in ev["suites"]], ["a", "b"])
        self.assertEqual(ev["suites"][0]["since"], "M1")
        self.assertIn("suite:a", self.failure_ids(ev))
        self.result("job-a")
        rc, ev = self.evaluate(man, "M2")
        self.assertEqual(rc, 0, ev["failures"])

    def test_a_later_milestone_widens_a_suites_scope(self):
        self.run_("agg", job="aggregate", tier="T3")
        man = manifest(M1={"suites": [{"id": "agg", "run": "agg", "scope": {"detail": "areas", "items": ["sync"]}}]},
                       M2={"suites": [{"id": "agg", "run": "agg", "scope": {"detail": "areas", "items": ["search"]}}]})
        self.result("aggregate", details={"areas": {"sync": "pass"}})
        rc, ev = self.evaluate(man, "M1")
        self.assertEqual(rc, 0, ev["failures"])
        rc, ev = self.evaluate(man, "M2")
        self.assertIn("search: not reported", self.failure(ev, "suite:agg"))


class ManifestTests(Sandbox):
    def test_a_suite_that_requires_its_own_gate_is_rejected(self):
        self.run_("m5-gate", job="milestone-gate", args=["M5"], tier="T3", cls="static")
        man = manifest(M5={"suites": [{"id": "m5-gate", "run": "m5-gate"}]})
        rc, out = self.evaluate(man, "M5")
        self.assertEqual(rc, 2)
        self.assertTrue(any("requires the M5 gate" in e for e in out["errors"]), out)
        self.config["runs"][0]["args"] = ["{milestone}"]
        rc, out = self.evaluate(man, "M5")
        self.assertEqual(rc, 2)
        # An earlier milestone's gate is not this milestone's.
        self.config["runs"][0]["args"] = ["M3"]
        rc, out = self.evaluate(man, "M5")
        self.assertNotEqual(rc, 2, out)

    def test_suites_must_be_runs_tools_mg_milestone_executes(self):
        self.run_("soak", tier="T2", cls="sync")
        self.run_("audit", tier="T3", release_only=True, cls="static")
        man = manifest(M1={"suites": [{"id": "soak", "run": "soak"}, {"id": "audit", "run": "audit"},
                                      {"id": "nope", "run": "nope"}]})
        rc, out = self.evaluate(man, "M1")
        self.assertEqual(rc, 2)
        text = "\n".join(out["errors"])
        self.assertIn("T2 run", text)
        self.assertIn("release runs", text)
        self.assertIn("run nope is not in tools/gate/tiers.yaml", text)

    def test_a_unit_in_two_milestones_is_rejected(self):
        rc, out = self.evaluate(manifest(M0={"units": ["U1"]}, M1={"units": ["U1"]}), "M1")
        self.assertEqual(rc, 2)
        self.assertTrue(any("U1 is already in M0" in e for e in out["errors"]), out)

    def test_check_compares_the_manifest_with_the_plan(self):
        plan = "\n".join([
            "### Delivery Milestones", "", "| Milestone | Units | Gate | User steps |", "|---|---|---|---|",
        ] + ["| **%s Milestone %s** | %s | g | u |" % (m, m, "U1, U2" if m == "M0" else "U%d" % (i + 2))
             for i, m in enumerate(MS)] + [
            "", "### Unit Index", "", "| U-ID | Title |", "|---|---|",
        ] + ["| U%d | t |" % i for i in range(1, len(MS) + 2)] + ["", "### U2b. A sub-unit", ""])
        man = manifest(**{m: {
            "units": ["U1", "U2"] if m == "M0" else ["U%d" % (i + 2)], "plan": {"gate": "g", "user_steps": "u"},
            "suites": [{"id": "g", "run": "g", "pending": "U9", "plan": ["g"]}],
            "user_steps": [{"id": "u", "description": "u", "evidence": "docs/u.json", "validator": {"type": "report"},
                            "plan": ["u"]}]} for i, m in enumerate(MS)})
        rc, out = self.check(man, plan)
        self.assertEqual((rc, out["errors"]), (0, []))
        milestone(man, "M0")["units"] = ["U1"]
        milestone(man, "M2")["plan"]["gate"] = "changed"
        milestone(man, "M3")["name"] = "Other"
        rc, out = self.check(man, plan)
        self.assertEqual(rc, 1)
        text = "\n".join(out["errors"])
        self.assertIn("M0: units differ from the plan's table (missing U2", text)
        self.assertIn("unit U2 of the Unit Index is in no milestone", text)
        self.assertIn("M2: plan.gate is not the table's closing gate text", text)
        self.assertIn("M3: name 'Other'", text)

    @staticmethod
    def plan_for(man):
        """A plan whose Delivery Milestones table and Unit Index are the manifest's."""
        lines = ["### Delivery Milestones", "", "| Milestone | Units | Gate | User steps |", "|---|---|---|---|"]
        lines += ["| **%s %s** | %s | %s | %s |" % (m["id"], m["name"], ", ".join(m["units"]), m["plan"]["gate"],
                                                 m["plan"]["user_steps"]) for m in man["milestones"]]
        lines += ["", "### Unit Index", "", "| U-ID | Title |", "|---|---|"]
        lines += ["| %s | t |" % u for m in man["milestones"] for u in m["units"]]
        return "\n".join(lines) + "\n"

    def bound_manifest(self):
        self.run_("native", job="fp-probe", tier="T3", cls="conformance")
        screens = {"detail": "screens", "items": ["home", "review"]}
        home_review = {"text": "for Home and review", "items": ["home", "review"]}
        per = {m: {"units": ["U%d" % (i + 1)]} for i, m in enumerate(MS)}
        per["M1"] = {
            "units": ["U246", "U252", "U253", "U254"],
            "plan": {"gate": "U246: areas storage and sync (R1926–R1933), plus the native oracle check (KTD24). "
                             "U252–U254 for Home and review.",
                     "user_steps": "Device run on iPhone and iPad (AE81). A 7-day trial, logged"},
            "suites": [
                {"id": "u246", "run": "u246", "pending": "U246", "scope": {"detail": "areas", "items": ["storage", "sync"]},
                 "plan": [{"text": "U246: areas storage and sync (R1926–R1933)", "items": ["storage", "sync"]}]},
                {"id": "u252", "run": "u252", "pending": "U252", "scope": screens, "plan": ["U252", home_review]},
                {"id": "u253", "run": "u253", "pending": "U253", "scope": screens, "plan": ["U253", home_review]},
                {"id": "u254", "run": "u254", "pending": "U254", "scope": screens, "plan": ["U254", home_review]}],
            "user_steps": [
                {"id": "device-run-%s" % d, "description": "run", "evidence": "docs/%s.json" % d,
                 "validator": {"type": "device-run", "device": d}, "plan": ["Device run on iPhone and iPad (AE81)"]}
                for d in ("iphone", "ipad")] + [
                {"id": "trial", "description": "trial", "evidence": "docs/log.jsonl",
                 "validator": {"type": "trial", "days": 7}, "plan": ["A 7-day trial, logged"]}]}
        man = manifest(**per)
        man["global"].append({"id": "native-oracle", "kind": "suite", "run": "native", "from": "M1",
                              "plan": ["the native oracle check (KTD24)"]})
        return man

    def test_check_binds_the_plan_text_to_the_suites_and_user_steps_that_run(self):
        man = self.bound_manifest()
        rc, out = self.check(man, self.plan_for(man))
        self.assertEqual((rc, out["errors"]), (0, []))

        def m1(m):
            return milestone(m, "M1")

        def drop(key, ident):
            return lambda m: m1(m).__setitem__(key, [x for x in m1(m)[key] if x["id"] != ident])

        def set_in(get, value):
            return lambda m: get(m).update(value)

        cases = [
            ("a required suite removed", drop("suites", "u246"), "U246: areas storage and sync (R1926–R1933)"),
            ("a scope item removed", set_in(lambda m: m1(m)["suites"][0]["scope"], {"items": ["storage"]}),
             "binds sync, which its scope lacks"),
            ("the suite of a unit inside a range removed", drop("suites", "u253"), "names U253"),
            ("the suite of a range's last unit removed", drop("suites", "u254"), "names U254"),
            ("a device run removed", drop("user_steps", "device-run-ipad"), "no device-run user step on ipad"),
            ("the trial shortened", set_in(lambda m: m1(m)["user_steps"][2]["validator"], {"days": 5}),
             "trial of 5 days, but its plan excerpts do not say 5-day"),
            ("a global condition unbound", lambda m: m["global"][1].pop("plan"), "the native oracle check (KTD24)"),
            ("an excerpt that is not in the text",
             set_in(lambda m: m1(m)["suites"][0], {"plan": ["U246: areas storage and search"]}),
             "'U246: areas storage and search' is not in the closing gate text"),
            ("an excerpt that spans a unit range", set_in(lambda m: m1(m)["suites"][1], {"plan": ["U252–U254"]}),
             "spans the unit range U252–U254"),
            ("an excerpt that is not unique", set_in(lambda m: m1(m)["suites"][0], {"plan": [
                "U246: areas", {"text": "storage", "items": ["storage"]}, {"text": "and", "items": []},
                {"text": "sync (R1926–R1933)", "items": ["sync"]}]}), "'and' occurs 2 times"),
            ("items on a step that takes none",
             set_in(lambda m: m1(m)["user_steps"][2], {"plan": [{"text": "A 7-day trial, logged", "items": ["x"]}]}),
             "a trial step binds no items"),
            # Text a suite binds to scope items is claimed with those items or
            # not at all: an itemless claim would keep the text covered once
            # the suite that holds the items is removed.
            ("a supporting suite quotes text another binds to items",
             lambda m: m1(m)["suites"].append({"id": "bench", "run": "bench", "pending": "U246",
                                               "plan": ["U246: areas storage and sync (R1926–R1933)"]}),
             "must bind the same items"),
            ("a co-claim that binds fewer items",
             lambda m: [m1(m)["suites"][2].update({"scope": {"detail": "screens", "items": ["home"]}}),
                        m1(m)["suites"][2].update({"plan": ["U253", {"text": "for Home and review", "items": ["home"]}]})],
             "must bind the same items"),
            ("a suite whose plan text another quotes alike",
             lambda m: m1(m)["suites"].append(dict(copy.deepcopy(m1(m)["suites"][0]), id="u246b", run="u246b")),
             "removing it would go unnoticed"),
            ("an excerpt that overlaps another definition's",
             set_in(lambda m: m1(m)["suites"][1], {"plan": ["U252", {"text": "for Home", "items": ["home"]}]}),
             "overlaps"),
            # The plan's wording names the validator a user step needs.
            ("the trial swapped for a report", set_in(lambda m: m1(m)["user_steps"][2], {"validator": {"type": "report"}}),
             "'7-day', which asks for a trial step of 7 days"),
            ("a device run swapped for a report",
             set_in(lambda m: m1(m)["user_steps"][1], {"validator": {"type": "report"}}),
             "'Device run', which asks for a device-run step"),
            ("a step whose excerpts do not name its kind",
             set_in(lambda m: m1(m)["user_steps"][2], {"validator": {"type": "setup", "step": "xcode-license"}}),
             "a setup step, but none of its plan excerpts says R1969"),
        ]
        for name, mutate, expected in cases:
            with self.subTest(name):
                broken = copy.deepcopy(man)
                mutate(broken)
                rc, out = self.check(broken, self.plan_for(broken))
                self.assertEqual(rc, 1, out)
                self.assertTrue(any(expected in e for e in out["errors"]), "%r not in %s" % (expected, out["errors"]))
                # evaluate refuses the same manifest: a gate never runs without a required entry.
                rc, out = self.evaluate(broken, "M1")
                self.assertEqual(rc, 2, out)

    def test_device_runs_the_plan_does_not_name_cover_every_device(self):
        man = self.bound_manifest()
        m1 = milestone(man, "M1")
        m1["plan"]["user_steps"] = "Device run. A 7-day trial, logged"
        for st in m1["user_steps"][:2]:
            st["plan"] = ["Device run"]
        rc, out = self.check(man, self.plan_for(man))
        self.assertEqual(rc, 1)
        self.assertTrue(any("no device-run user step on mac" in e for e in out["errors"]), out["errors"])

    def test_check_rejects_an_unapproved_deferral(self):
        self.run_("snap", job="snapshots", cls="ui")
        man = manifest(M5={"deferred": [{"id": "foldable", "dependency": "xcode-27.1-foldable-simulator",
                                         "condition": FOLDABLE, "cells": {"run": "snap"}}]})
        plan = "### Delivery Milestones\n" + "".join("| **%s Milestone %s** |  | g | u |\n" % (m, m) for m in MS)
        rc, out = self.check(man, plan)
        self.assertEqual(rc, 1)
        self.assertTrue(any("approved_by: antoine" in e for e in out["errors"]), out)


class UnitTests(Sandbox):
    def test_an_escalated_unmerged_unit_fails_and_is_named(self):
        self.merge_units("U30")
        ledger = {"id": "E-20261101T000000Z-ab12", "kind": "blocked-red", "state": "open", "unit": "U31",
                  "summary": "no green gate after six rounds", "created_at": "2026-11-01T00:00:00Z"}
        self.commit("Record escalation", {"docs/process/provenance/escalations.jsonl": json.dumps(ledger) + "\n"})
        man = manifest(M1={"units": ["U30", "U31"]})
        rc, ev = self.evaluate(man, "M1")
        self.assertEqual(rc, 1)
        self.assertIn("U31", self.failure(ev, "unit:U31"))
        self.assertIn("E-20261101T000000Z-ab12", self.failure(ev, "escalation:U31:E-20261101T000000Z-ab12"))
        self.assertNotIn("unit:U30", self.failure_ids(ev))

    def test_a_resolved_escalation_does_not_block(self):
        self.merge_units("U31")
        lines = [{"id": "E-1", "kind": "approval", "state": "open", "unit": "U31", "summary": "s"},
                 {"id": "E-1", "state": "resolved", "resolved_by": "antoine", "resolution": "ok"}]
        self.commit("Ledger", {"docs/process/provenance/escalations.jsonl": "".join(json.dumps(x) + "\n" for x in lines)})
        rc, ev = self.evaluate(manifest(M1={"units": ["U31"]}), "M1")
        self.assertEqual(rc, 0, ev["failures"])

    def test_spec_writer_commits_do_not_merge_a_unit(self):
        self.merge_units("U26", role="spec-writer")
        rc, ev = self.evaluate(manifest(M1={"units": ["U26"]}), "M1")
        self.assertIn("unit:U26", self.failure_ids(ev))

    def test_units_are_checked_at_the_gate_commit(self):
        self.run_("unit", job="core-test")
        self.result("core-test")
        self.merge_units("U27")  # after the gate commit
        rc, ev = self.evaluate(manifest(M0={"suites": [{"id": "unit", "run": "unit"}]}, M1={"units": ["U27"]}), "M1")
        self.assertIn("unit:U27", self.failure_ids(ev))

    def test_earlier_milestones_units_count(self):
        rc, ev = self.evaluate(manifest(M0={"units": ["U1"]}), "M1")
        self.assertIn("unit:U1", self.failure_ids(ev))

    def test_an_unreadable_ledger_fails(self):
        self.commit("Ledger", {"docs/process/provenance/escalations.jsonl": "<<<<<<< conflict\n"})
        rc, ev = self.evaluate(manifest(), "M0")
        self.assertIn("escalations", self.failure_ids(ev))


class QuarantineTests(Sandbox):
    def test_a_quarantined_sync_test_fails_every_milestone_gate(self):
        self.config["quarantine"]["entries"] = [{
            "test": "sync::two_clients", "run": "sync-smoke", "class": "sync", "owner": "U107",
            "reason": "flaky port", "deadline": "2026-12-31", "approved_by": "antoine",
            "evidence": {"commit": "a" * 40, "failed": "20261101T000000Z-1", "passed": "20261101T000001Z-2"},
            "escalation": None, "label": "sync-smoke:sync::two_clients"}]
        rc, ev = self.evaluate(manifest(), "M0")
        self.assertEqual(rc, 1)
        self.assertIn("sync", self.failure(ev, "quarantine-protected:sync-smoke:sync::two_clients"))
        self.assertEqual(ev["quarantine"][0]["owner"], "U107")

    def test_a_quarantine_entry_without_owner_or_deadline_fails(self):
        load_quarantine = tiers_quarantine()
        runs = [{"id": "ui", "class": "ui"}]
        self.config["quarantine"] = load_quarantine({"entries": [{
            "test": "Snap/flaky", "run": "ui", "class": "ui", "reason": "timing", "approved_by": "antoine",
            "evidence": {"commit": "a" * 40, "failed": "20261101T000000Z-1", "passed": "20261101T000001Z-2"}}]}, runs)
        self.assertTrue(self.config["quarantine"]["invalid"])
        rc, ev = self.evaluate(manifest(), "M0")
        self.assertEqual(rc, 1)
        message = self.failure(ev, "quarantine-invalid:ui:Snap/flaky")
        self.assertIn("owner is required", message)
        self.assertIn("deadline", message)

    def test_an_open_flaky_ui_quarantine_is_recorded_not_failed(self):
        self.config["quarantine"]["entries"] = [{
            "test": "Snap/flaky", "run": "ui", "class": "ui", "owner": "U252", "reason": "timing",
            "deadline": "2026-12-31", "approved_by": "antoine", "evidence": None, "escalation": None, "label": "x"}]
        rc, ev = self.evaluate(manifest(), "M0")
        self.assertEqual(rc, 0, ev["failures"])
        with open(self.record, encoding="utf-8") as f:
            self.assertEqual(json.load(f)["quarantine"][0], {"run": "ui", "test": "Snap/flaky", "class": "ui",
                                                             "owner": "U252", "deadline": "2026-12-31"})

    def test_an_expired_quarantine_fails(self):
        self.config["quarantine"]["entries"] = [{
            "test": "t", "run": "ui", "class": "ui", "owner": "U252", "reason": "r", "deadline": "2026-11-01",
            "approved_by": "antoine", "evidence": None, "escalation": None, "label": "x"}]
        rc, ev = self.evaluate(manifest(), "M0")
        self.assertIn("quarantine-expired:ui:t", self.failure_ids(ev))


class GlobalTests(Sandbox):
    def test_native_oracle_applies_from_m1_and_is_attached(self):
        self.run_("native-oracle", job="fp-probe", tier="T3", cls="conformance")
        man = manifest()
        man["global"].append({"id": "native-oracle", "kind": "suite", "run": "native-oracle", "from": "M1"})
        rc, ev = self.evaluate(man, "M0")
        self.assertEqual(rc, 0, ev["failures"])
        rc, ev = self.evaluate(man, "M1")
        self.assertIn("global:native-oracle", self.failure_ids(ev))
        path = self.result("fp-probe")
        rc, ev = self.evaluate(man, "M1")
        self.assertEqual(rc, 0, ev["failures"])
        with open(self.record, encoding="utf-8") as f:
            attached = [g for g in json.load(f)["global"] if g["id"] == "native-oracle"][0]["attached"]
        self.assertEqual(attached["result"], path)

    def test_a_pending_global_condition_fails_once_it_applies(self):
        man = manifest()
        man["global"].append({"id": "nightly-divergences", "pending": "U247", "from": "M2"})
        rc, ev = self.evaluate(man, "M1")
        self.assertEqual(rc, 0, ev["failures"])
        rc, ev = self.evaluate(man, "M2")
        self.assertIn("U247", self.failure(ev, "global:nightly-divergences"))


class DeferralTests(Sandbox):
    def setUp(self):
        super().setUp()
        self.run_("snap", job="snapshots", cls="ui")
        self.run_("unit", job="core-test")

    def man(self, **entry):
        d = {"id": "foldable", "dependency": "xcode-27.1-foldable-simulator", "condition": FOLDABLE,
             "cells": {"run": "snap", "scope": {"detail": "devices", "items": ["foldable"]}},
             "approved_by": "antoine", "why": "no foldable simulator in Xcode 27.0"}
        d.update(entry)
        return manifest(M0={"suites": [{"id": "unit", "run": "unit"}]}, M5={"deferred": [d]})

    def test_an_approved_deferral_whose_condition_holds_passes_and_is_recorded(self):
        self.result("core-test")
        self.result("sim-setup", details={"foldable": {"status": "unavailable", "reason": "toolchain"}})
        rc, ev = self.evaluate(self.man(), "M5")
        self.assertEqual(rc, 0, ev["failures"])
        with open(self.record, encoding="utf-8") as f:
            deferred = json.load(f)["deferred"]
        self.assertEqual([(d["id"], d["outcome"], d["approved_by"]) for d in deferred],
                         [("foldable", "deferred", "antoine")])

    def test_an_unapproved_deferral_fails(self):
        self.result("core-test")
        self.result("sim-setup", details={"foldable": {"status": "unavailable", "reason": "toolchain"}})
        rc, ev = self.evaluate(self.man(approved_by="agent"), "M5")
        self.assertEqual(rc, 1)
        self.assertIn("approved_by: antoine", self.failure(ev, "deferred:foldable"))

    def test_a_deferral_outside_the_fixed_list_fails(self):
        self.result("core-test")
        rc, ev = self.evaluate(self.man(dependency="ankiweb-permission"), "M5")
        self.assertIn("dependency must be one of", self.failure(ev, "deferred:foldable"))
        rc, ev = self.evaluate(self.man(condition=dict(FOLDABLE, equals={})), "M5")
        self.assertIn("the condition of", self.failure(ev, "deferred:foldable"))

    def test_a_deferral_whose_condition_no_longer_holds_fails_until_its_cells_pass(self):
        self.result("core-test")
        self.result("sim-setup", details={"foldable": {"status": "available", "device_type": "iPhone Fold"}})
        rc, ev = self.evaluate(self.man(), "M5")
        self.assertEqual(rc, 1)
        self.assertIn("no longer holds", self.failure(ev, "deferred:foldable"))
        self.result("snapshots", details={"devices": {"foldable": "pass"}})
        rc, ev = self.evaluate(self.man(), "M5")
        self.assertEqual(rc, 0, ev["failures"])
        self.assertEqual(ev["deferred"][0]["outcome"], "resolved")

    def test_a_deferral_needs_a_condition_result(self):
        self.result("core-test")
        rc, ev = self.evaluate(self.man(), "M5")
        self.assertIn("sim-setup", self.failure(ev, "deferred:foldable"))

    UNAVAILABLE = {"lane": {"kind": "foldable", "status": "unavailable", "reason": "toolchain"}}

    def holding(self):
        self.result("core-test")
        self.result("sim-setup", details={"foldable": {"status": "unavailable", "reason": "toolchain"}})

    def with_snap(self, scope=None):
        man = self.man()
        man["milestones"][1]["suites"] = [dict({"id": "snap", "run": "snap"}, **({"scope": scope} if scope else {}))]
        return man

    def snap(self, ev):
        return [s for s in ev["suites"] if s["id"] == "snap"][0]

    def test_an_approved_deferral_covers_its_cells_runs_unavailable_lane(self):
        self.holding()
        self.result("snapshots", status="warn", details=dict(self.UNAVAILABLE, devices={
            "iphone": "pass", "ipad": {"status": "pass"}, "foldable": "unavailable"}))
        man = self.with_snap()
        rc, ev = self.evaluate(man, "M5")
        self.assertEqual(rc, 0, ev["failures"])
        self.assertEqual(self.snap(ev)["outcome"], "deferred")
        self.assertIn("foldable", self.snap(ev)["message"])
        with open(self.record, encoding="utf-8") as f:
            self.assertEqual([(s["id"], s["status"], s["outcome"]) for s in json.load(f)["suites"]],
                             [("unit", "pass", "pass"), ("snap", "warn", "deferred")])
        os.remove(self.record)
        # Without the deferral, the same result fails.
        rc, ev = self.evaluate(man, "M1")
        self.assertEqual(rc, 1)
        self.assertIn("unavailable", self.failure(ev, "suite:snap"))

    def test_a_deferral_waives_only_its_cells_of_a_scoped_suite(self):
        # The suite needs the iPhone and foldable cells; Antoine deferred the
        # foldable ones only. A warn of the foldable lane with no iPhone result
        # is no pass.
        self.holding()
        man = self.with_snap({"detail": "devices", "items": ["iphone", "foldable"]})
        cases = [
            (dict(self.UNAVAILABLE, devices={"foldable": "unavailable"}), "iphone: not reported"),
            (self.UNAVAILABLE, "details.devices is missing"),
            (dict(self.UNAVAILABLE, devices={"iphone": "fail", "foldable": "unavailable"}), "iphone: fail"),
        ]
        for details, expected in cases:
            with self.subTest(expected):
                self.result("snapshots", status="warn", details=details)
                rc, ev = self.evaluate(man, "M5")
                self.assertEqual((rc, ev["outcome"]), (1, "fail"))
                self.assertEqual(self.snap(ev)["outcome"], "fail")
                self.assertIn(expected, self.failure(ev, "suite:snap"))
                self.assertFalse(os.path.exists(self.record))
        self.result("snapshots", status="warn", details=dict(self.UNAVAILABLE, devices={
            "iphone": "pass", "foldable": "unavailable"}))
        rc, ev = self.evaluate(man, "M5")
        self.assertEqual(rc, 0, ev["failures"])
        self.assertEqual(self.snap(ev)["outcome"], "deferred")
        self.assertIn("foldable", self.snap(ev)["message"])
        # A pass that leaves the deferred cell out is deferred too.
        os.remove(self.record)
        self.result("snapshots", details={"devices": {"iphone": "pass"}})
        rc, ev = self.evaluate(man, "M5")
        self.assertEqual(rc, 0, ev["failures"])
        self.assertEqual(self.snap(ev)["outcome"], "deferred")

    def test_a_deferral_leaves_the_rest_of_an_unscoped_suites_run_to_pass(self):
        # Without a scope the suite needs its whole run: the result must show
        # every cell but the deferred ones passing.
        self.holding()
        man = self.with_snap()
        cases = [
            (self.UNAVAILABLE, "details.devices is missing"),
            (dict(self.UNAVAILABLE, devices={"ipad": "unavailable", "foldable": "unavailable"}), "ipad: unavailable"),
        ]
        for details, expected in cases:
            with self.subTest(expected):
                self.result("snapshots", status="warn", details=details)
                rc, ev = self.evaluate(man, "M5")
                self.assertEqual(rc, 1)
                self.assertIn(expected, self.failure(ev, "suite:snap"))

    def test_a_deferral_keeps_the_pass_rules_and_failures(self):
        self.holding()
        self.config["runs"][0]["require"] = [{"id": "nothing-failed", "summary": "failed", "max": 0}]
        man = self.with_snap({"detail": "devices", "items": ["iphone", "foldable"]})
        devices = dict(self.UNAVAILABLE, devices={"iphone": "pass", "foldable": "unavailable"})
        self.result("snapshots", status="warn", details=devices,
                    summary={"total": 4, "passed": 2, "failed": 2, "skipped": 0})
        rc, ev = self.evaluate(man, "M5")
        self.assertEqual(rc, 1)
        self.assertIn("nothing-failed", self.failure(ev, "suite:snap"))
        self.result("snapshots", status="warn", details=devices,
                    failures=[{"id": "Snap/iphone.home", "message": "differs", "repro": "x"}])
        rc, ev = self.evaluate(man, "M5")
        self.assertEqual(rc, 1)
        self.assertIn("Snap/iphone.home", self.failure(ev, "suite:snap"))

    def test_a_deferral_must_name_its_cells(self):
        self.holding()
        rc, ev = self.evaluate(self.man(cells={"run": "snap"}), "M5")
        self.assertEqual(rc, 1)
        self.assertIn("cells must be {run", self.failure(ev, "deferred:foldable"))

    def test_a_deferral_covers_only_its_dependencys_lane_while_it_holds(self):
        self.result("core-test")
        self.result("sim-setup", details={"foldable": {"status": "unavailable", "reason": "toolchain"}})
        self.result("snapshots", status="warn",
                    details={"lane": {"kind": "ipad", "status": "unavailable", "reason": "toolchain"}})
        man = self.man()
        man["milestones"][1]["suites"] = [{"id": "snap", "run": "snap"}]
        rc, ev = self.evaluate(man, "M5")
        self.assertEqual(rc, 1)
        self.assertIn("suite:snap", self.failure_ids(ev))
        # The condition no longer holds: nothing covers the lane.
        self.result("snapshots", status="warn", details=self.UNAVAILABLE)
        self.result("sim-setup", details={"foldable": {"status": "available", "device_type": "iPhone Fold"}})
        rc, ev = self.evaluate(man, "M5")
        self.assertEqual(rc, 1)
        self.assertIn("suite:snap", self.failure_ids(ev))
        self.assertIn("deferred:foldable", self.failure_ids(ev))


class UserStepTests(Sandbox):
    def setUp(self):
        super().setUp()
        self.run_("unit", job="core-test")
        self.result("core-test")
        self.gate = self.git("rev-parse", "HEAD")

    def man(self, steps, milestone="M1"):
        per = {"M0": {"suites": [{"id": "unit", "run": "unit"}]},
               "M1": {"budgets": {"launch": ["R1818", "R1822"], "open": ["R1823"]}}}
        per.setdefault(milestone, {})["user_steps"] = steps
        return manifest(**per)

    def device_steps(self, milestone="M1"):
        return [{"id": "device-run-%s" % d, "description": "device run",
                 "evidence": "docs/process/milestone-records/device-runs/%s/%s.json" % (milestone, d),
                 "validator": {"type": "device-run", "device": d}} for d in ("iphone", "ipad")]

    def test_a_missing_device_run_awaits_antoine(self):
        rc, ev = self.evaluate(self.man(self.device_steps()), "M1")
        self.assertEqual((rc, ev["outcome"]), (3, "awaiting_user"), ev["failures"])
        self.assertIn("user-step:device-run-iphone", [x["id"] for x in ev["awaiting"]])
        self.assertIn("device-runs/M1/iphone.json", ev["awaiting"][0]["message"])
        self.assertFalse(os.path.exists(self.record))

    def test_a_device_run_that_records_a_budget_failure_fails(self):
        self.commit("Record device runs", {
            "docs/process/milestone-records/device-runs/M1/iphone.json": device_run("M1", "iphone", self.gate, ["R1818", "R1823"]),
            "docs/process/milestone-records/device-runs/M1/ipad.json":
                device_run("M1", "ipad", self.gate, M1_BUDGETS, fail=("R1818",))})
        rc, ev = self.evaluate(self.man(self.device_steps()), "M1")
        self.assertEqual(rc, 1)
        message = self.failure(ev, "user-step:device-run-ipad")
        self.assertIn("R1818", message)
        self.assertIn("device-run-ipad", message)

    def test_device_runs_committed_after_the_gate_commit_are_accepted(self):
        build = self.gate
        later = self.commit("Record device runs", {
            "docs/process/milestone-records/device-runs/M1/iphone.json": device_run("M1", "iphone", build, ["R1818", "R1823"]),
            "docs/process/milestone-records/device-runs/M1/ipad.json": device_run("M1", "ipad", build, M1_BUDGETS)})
        rc, ev = self.evaluate(self.man(self.device_steps()), "M1")
        # The evidence commit is accepted; the runs stay pending on U241's report contract.
        self.assertEqual(self.failure_ids(ev), ["user-step:device-run-iphone", "user-step:device-run-ipad"])
        self.assertEqual(ev["gate_commit"], self.gate)
        self.assertEqual(ev["evidence_commits"], [later])
        step = ev["user_steps"][0]
        self.assertEqual(step["details"]["build_commit"], build)
        self.assertEqual(step["details"]["budgets"], ["R1818", "R1823"])  # R1822 is iPad and Mac only

    def test_a_device_run_stays_pending_until_u241s_report_contract(self):
        # Rows that only say pass: nothing checks U241's schema, dimensions,
        # fixture or configuration yet, so they must not close the step.
        self.commit("Record device runs", {
            "docs/process/milestone-records/device-runs/M1/iphone.json":
                {"milestone": "M1", "device": "iphone", "build_commit": self.gate,
                 "budgets": [{"id": "R1818", "status": "pass"}, {"id": "R1823", "status": "pass"}]}})
        rc, ev = self.evaluate(self.man(self.device_steps()[:1]), "M1")
        self.assertEqual((rc, ev["outcome"]), (1, "fail"))
        self.assertEqual(ev["user_steps"][0]["outcome"], "pending")
        self.assertIn("U241", self.failure(ev, "user-step:device-run-iphone"))
        self.assertFalse(os.path.exists(self.record))

    def test_a_device_run_copies_the_app_and_core_changes_since_its_build(self):
        build = self.gate
        self.commit("Change app", {"app/Meguri/A.swift": "// a\n", "docs/x.md": "x\n"})
        self.result("core-test")
        self.gate = self.git("rev-parse", "HEAD")
        self.commit("Record device runs", {
            "docs/process/milestone-records/device-runs/M1/iphone.json": device_run("M1", "iphone", build, ["R1818", "R1823"]),
            "docs/process/milestone-records/device-runs/M1/ipad.json": device_run("M1", "ipad", build, M1_BUDGETS)})
        rc, ev = self.evaluate(self.man(self.device_steps()), "M1")
        self.assertEqual(ev["gate_commit"], self.gate)
        self.assertEqual(ev["user_steps"][0]["details"]["changed_since_build"], ["app/Meguri/A.swift"])

    def test_invalid_device_runs_fail(self):
        cases = [
            (device_run("M0", "iphone", self.gate, ["R1818", "R1823"]), "names milestone"),
            (device_run("M1", "ipad", self.gate, ["R1818", "R1823"]), "names device"),
            (device_run("M1", "iphone", self.gate, ["R1818"]), "no result for R1823"),
            (device_run("M1", "iphone", "f" * 40, ["R1818", "R1823"]), "build_commit"),
            ({"milestone": "M1"}, "expected"),
        ]
        for data, expected in cases:
            self.commit("Device run", {"docs/process/milestone-records/device-runs/M1/iphone.json": data})
            rc, ev = self.evaluate(self.man(self.device_steps()[:1]), "M1")
            self.assertEqual(rc, 1, expected)
            self.assertIn(expected, self.failure(ev, "user-step:device-run-iphone"))

    def test_variant_pick_without_an_entry_for_a_screen_awaits_and_names_it(self):
        decisions = "\n".join([
            "# Design decisions", "", "| Date | Screen | Decision | Reason | Source |", "|---|---|---|---|---|",
            "| 2026-11-10 | home.deckList | Picked B | calmer | gallery, reviews/2026-11-10.json |",
            "| 2026-11-11 | review.question | Vetoed A | too loud | chat |",
            "| 2026-09-01 | review.question | Picked C | old pick | chat |", ""])
        # The window starts when the milestone before M1 in the manifest's
        # order (M1R) closed, not M0.
        self.commit("Picks", {"docs/design/decisions.md": decisions,
                              "docs/process/milestone-records/M0.json": {"closed_at": "2026-08-01T00:00:00Z"},
                              "docs/process/milestone-records/M1R.json": {"closed_at": "2026-10-01T00:00:00Z"}})
        steps = [{"id": "variant-picks", "description": "picks", "evidence": "docs/design/decisions.md",
                  "validator": {"type": "variant-pick", "screens": ["home.deckList", "home.deckPage", "review.question"]}}]
        rc, ev = self.evaluate(self.man(steps), "M1")
        self.assertEqual((rc, ev["outcome"]), (3, "awaiting_user"), ev["failures"])
        message = ev["awaiting"][0]["message"]
        self.assertIn("home.deckPage", message)
        self.assertIn("review.question", message)  # vetoed, and picked before M1R closed
        self.assertNotIn("home.deckList", message)
        self.assertEqual(ev["window_start"], "2026-10-01")

    def trial(self, entries, days=7):
        log = "".join(json.dumps(e) + "\n" for e in entries)
        self.commit("Log", {"docs/process/daily-driver-log.jsonl": log})
        steps = [{"id": "trial", "description": "trial", "evidence": "docs/process/daily-driver-log.jsonl",
                  "validator": {"type": "trial", "days": days}}]
        return self.evaluate(self.man(steps), "M1")

    def day(self, n, fallback=None, incident=None):
        return {"date": "2026-11-%02d" % n, "devices": ["iphone"], "fallback": fallback, "data_incident": incident}

    def test_a_trial_log_with_a_data_incident_fails(self):
        entries = [self.day(n) for n in range(10, 15)] + [self.day(15, incident="cards lost after sync")] + \
                  [self.day(n) for n in range(16, 19)]
        rc, ev = self.trial(entries)
        self.assertEqual(rc, 1)
        message = self.failure(ev, "user-step:trial")
        self.assertIn("2026-11-15", message)
        self.assertIn("cards lost after sync", message)

    def test_a_complete_trial_passes(self):
        out_of_scope = {"task": "added cards in desktop Anki", "in_scope": False}
        rc, ev = self.trial([self.day(n, fallback=out_of_scope if n == 12 else "none") for n in range(10, 17)])
        self.assertEqual(rc, 0, ev["failures"])
        self.assertEqual(ev["user_steps"][0]["details"]["streak"], 7)

    def test_trial_entries_must_record_devices_fallback_and_incident(self):
        # Seven entries holding only a date are no evidence of seven clean days.
        rc, ev = self.trial([{"date": "2026-11-%02d" % n} for n in range(10, 17)])
        self.assertEqual(rc, 1)
        self.assertIn("lacks devices, fallback, data_incident", self.failure(ev, "user-step:trial"))
        partial = [self.day(n) for n in range(10, 17)]
        del partial[3]["data_incident"]
        rc, ev = self.trial(partial)
        self.assertEqual(rc, 1)
        self.assertIn("line 4 lacks data_incident", self.failure(ev, "user-step:trial"))
        for devices in ([], ["watch"], "iphone", ["iphone", "iphone"]):
            with self.subTest(devices=devices):
                entries = [self.day(n) for n in range(10, 17)]
                entries[0]["devices"] = devices
                rc, ev = self.trial(entries)
                self.assertEqual(rc, 1)
                self.assertIn("devices must list", self.failure(ev, "user-step:trial"))

    def test_a_short_or_gapped_trial_awaits(self):
        rc, ev = self.trial([self.day(n) for n in (5, 6, 7, 8, 10, 11, 12)])
        self.assertEqual(rc, 3)
        self.assertIn("3 of 7", ev["awaiting"][0]["message"])

    def test_an_in_scope_fallback_breaks_the_trial(self):
        rc, ev = self.trial([self.day(n) for n in range(5, 12)] +
                            [self.day(12, fallback={"task": "reviewed in AnkiMobile", "in_scope": True})])
        self.assertEqual(rc, 1)
        self.assertIn("reviewed in AnkiMobile", self.failure(ev, "user-step:trial"))

    def test_device_session_within_45_days(self):
        steps = [{"id": "device-session", "description": "session",
                  "evidence": "docs/process/milestone-records/device-sessions/",
                  "validator": {"type": "device-session", "days": 45}}]
        self.commit("Old session", {"docs/process/milestone-records/device-sessions/2026-09.json":
                                    {"date": "2026-09-20", "reported_by": "antoine", "findings": []}})
        rc, ev = self.evaluate(self.man(steps, "M2"), "M2")
        self.assertEqual(rc, 3)
        self.assertIn("latest: 2026-09-20", ev["awaiting"][0]["message"])
        self.commit("Session", {"docs/process/milestone-records/device-sessions/2026-11.json":
                                {"date": "2026-11-03", "reported_by": "antoine", "findings": []}})
        rc, ev = self.evaluate(self.man(steps, "M2"), "M2")
        self.assertEqual(rc, 0, ev["failures"])
        self.commit("Agent's own session", {"docs/process/milestone-records/device-sessions/2026-10.json":
                                            {"date": "2026-10-03"}})
        rc, ev = self.evaluate(self.man(steps, "M2"), "M2")
        self.assertIn("reported_by: antoine", self.failure(ev, "user-step:device-session"))

    def test_capture_index_must_list_every_case(self):
        steps = [{"id": "r1913-capture", "description": "capture", "evidence": "conformance/references/render/",
                  "validator": {"type": "capture", "index": "conformance/references/render/index.json",
                                "cases_file": "conformance/references/render/cases.json"}}]
        rc, ev = self.evaluate(self.man(steps), "M1")
        self.assertIn("case list", self.failure(ev, "user-step:r1913-capture"))
        self.commit("Cases", {"conformance/references/render/cases.json": {"cases": [{"id": "U7"}, {"id": "S11"}]}})
        rc, ev = self.evaluate(self.man(steps), "M1")
        self.assertEqual(rc, 3)
        self.commit("Index", {"conformance/references/render/index.json": {"cases": [{"id": "U7"}]}})
        rc, ev = self.evaluate(self.man(steps), "M1")
        self.assertEqual(rc, 3)
        self.assertIn("lacks S11", ev["awaiting"][0]["message"])
        self.commit("Index", {"conformance/references/render/index.json": {"cases": [{"id": "U7"}, {"id": "S11"}]}})
        rc, ev = self.evaluate(self.man(steps), "M1")
        self.assertEqual(rc, 0, ev["failures"])

    def test_report_records_antoines_result(self):
        steps = [{"id": "firewall-check", "description": "firewall",
                  "evidence": "docs/process/milestone-records/M4/firewall-check.json", "validator": {"type": "report"}}]
        rc, ev = self.evaluate(self.man(steps, "M4"), "M4")
        self.assertEqual(rc, 3)
        self.commit("Report", {"docs/process/milestone-records/M4/firewall-check.json":
                               {"date": "2026-11-12", "result": "fail", "reported_by": "antoine"}})
        rc, ev = self.evaluate(self.man(steps, "M4"), "M4")
        self.assertIn("reported a failure", self.failure(ev, "user-step:firewall-check"))

    def test_setup_steps_read_doctor(self):
        steps = [{"id": s, "description": s, "evidence": "build/mg/doctor/", "validator": {"type": "setup", "step": s}}
                 for s in ("xcode-license", "ios-runtime", "xcode-external-agents")]
        man = manifest(M0={"suites": [{"id": "unit", "run": "unit"}], "user_steps": steps})
        doctor = {"status": "warn", "user_steps": [
            {"id": "ios-runtime", "state": "missing", "required": True, "title": "Install the iOS 27 simulator runtime"},
            {"id": "xcode-external-agents", "state": "unverified", "required": False, "title": "Allow agents"}]}
        rc, ev = self.evaluate(man, "M0", doctor=doctor)
        self.assertEqual(rc, 3)
        self.assertEqual([a["id"] for a in ev["awaiting"]], ["user-step:ios-runtime"])
        doctor["user_steps"] = doctor["user_steps"][1:]
        rc, ev = self.evaluate(man, "M0", doctor=doctor)
        self.assertEqual(rc, 0, ev["failures"])
        self.assertEqual(ev["user_steps"][2]["details"]["state"], "unverified")
        os.remove(self.record)
        rc, ev = self.evaluate(man, "M0")
        self.assertIn("doctor", self.failure(ev, "user-step:xcode-license"))

    def test_uncommitted_evidence_does_not_count(self):
        path = os.path.join(self.root, "docs/process/milestone-records/device-runs/M1/iphone.json")
        os.makedirs(os.path.dirname(path))
        with open(path, "w", encoding="utf-8") as f:
            json.dump(device_run("M1", "iphone", self.gate, ["R1818", "R1823"]), f)
        rc, ev = self.evaluate(self.man(self.device_steps()[:1]), "M1")
        self.assertEqual(rc, 3)
        self.assertIn("uncommitted", ev["awaiting"][0]["message"])


class MilestoneOrderTests(Sandbox):
    """U240r: the milestones and their order are the manifest's one list
    (M0, M1R, M1, ...), which --check keeps equal to the plan's table."""

    def reviews_first(self):
        self.run_("unit", job="core-test")
        self.run_("review-loop", job="u243", cls="perf")
        self.run_("sync-soak", job="u248", tier="T3", cls="sync")
        self.run_("browser", job="u244", cls="perf")
        return manifest(M0={"suites": [{"id": "unit", "run": "unit"}]},
                        M1R={"suites": [{"id": "review-loop", "run": "review-loop"},
                                        {"id": "sync-soak", "run": "sync-soak"}]},
                        M1={"suites": [{"id": "browser", "run": "browser"}]})

    def test_m1r_with_every_suite_result_missing_reports_each_missing(self):
        rc, ev = self.evaluate(self.reviews_first(), "M1R")
        self.assertEqual((rc, ev["outcome"]), (1, "fail"))
        self.assertEqual([(s["id"], s["since"], s["outcome"]) for s in ev["suites"]],
                         [("unit", "M0", "missing"), ("review-loop", "M1R", "missing"),
                          ("sync-soak", "M1R", "missing")])
        for sid in ("unit", "review-loop", "sync-soak"):
            self.assertIn("no result", self.failure(ev, "suite:" + sid))

    def test_m1_includes_m1rs_suites(self):
        man = self.reviews_first()
        for job in ("core-test", "u243", "u248", "u244"):
            self.result(job)
        rc, ev = self.evaluate(man, "M1")
        self.assertEqual(rc, 0, ev["failures"])
        self.assertEqual([(s["id"], s["since"]) for s in ev["suites"]],
                         [("unit", "M0"), ("review-loop", "M1R"), ("sync-soak", "M1R"), ("browser", "M1")])
        # M1R itself does not run M1's suites.
        os.remove(self.record)
        rc, ev = self.evaluate(man, "M1R")
        self.assertEqual([s["id"] for s in ev["suites"]], ["unit", "review-loop", "sync-soak"])

    def test_an_unknown_milestone_is_rejected_with_the_manifests_names(self):
        rc, out = self.evaluate(manifest(), "M1X")
        self.assertEqual(rc, 2)
        self.assertIn("unknown milestone 'M1X'", out["errors"][0])
        self.assertIn("M0, M1R, M1, M2, M3, M4, M5", out["errors"][0])

    def test_the_names_are_the_manifests_own(self):
        # Nothing matches M<digit>: any ordered list of words is the milestones.
        self.run_("unit", job="core-test")
        self.result("core-test")
        man = {"version": 1, "plan": "plan.md", "evidence_paths": [], "budget_devices": {},
               "global": [{"id": "quarantine", "kind": "quarantine"}],
               "milestones": [{"id": i, "name": i, "plan": {"gate": "", "user_steps": ""}, "units": [],
                               "suites": s, "user_steps": [], "deferred": []}
                              for i, s in (("Alpha", [{"id": "unit", "run": "unit"}]), ("Beta", []))]}
        rc, ev = self.evaluate(man, "Beta")
        self.assertEqual(rc, 0, ev)
        self.assertEqual([(s["id"], s["since"]) for s in ev["suites"]], [("unit", "Alpha")])
        rc, out = self.evaluate(man, "M0")
        self.assertEqual(rc, 2)
        self.assertIn("Alpha, Beta", out["errors"][0])

    def test_milestone_ids_are_unique_words_and_never_release(self):
        for ids in (["M0", "M1R", "M1R"], ["M0", "release"], ["M0", "M 1"], []):
            with self.subTest(ids=ids):
                man = manifest()
                man["milestones"] = [dict(copy.deepcopy(man["milestones"][0]), id=i) for i in ids]
                rc, out = self.evaluate(man, "M0")
                self.assertEqual(rc, 2)
                self.assertIn("each with its own id", out["errors"][0])

    def test_a_suite_may_not_require_its_own_milestones_gate(self):
        self.run_("m1r-gate", job="milestone-gate", args=["M1R"], tier="T3", cls="static")
        rc, out = self.evaluate(manifest(M1R={"suites": [{"id": "g", "run": "m1r-gate"}]}), "M1R")
        self.assertEqual(rc, 2)
        self.assertTrue(any("requires the M1R gate" in e for e in out["errors"]), out)
        rc, out = self.evaluate(manifest(M1={"suites": [{"id": "g", "run": "m1r-gate"}]}), "M1")
        self.assertNotEqual(rc, 2, out)  # an earlier milestone's gate

    def test_the_quarantine_rule_holds_at_m1r_and_every_later_gate(self):
        self.config["quarantine"]["entries"] = [{
            "test": "sync::first_download", "run": "sync-smoke", "class": "sync", "owner": "U107",
            "reason": "flaky port", "deadline": "2026-12-31", "approved_by": "antoine", "evidence": None,
            "escalation": None, "label": "sync-smoke:sync::first_download"}]
        for m in MS[MS.index("M1R"):]:
            with self.subTest(milestone=m):
                rc, ev = self.evaluate(manifest(), m)
                self.assertEqual(rc, 1)
                self.assertIn("U19", self.failure(ev, "quarantine-protected:sync-smoke:sync::first_download"))

    def test_the_native_oracle_is_attached_from_m1r(self):
        self.run_("native-oracle", job="fp-probe", tier="T3", cls="conformance")
        man = manifest()
        man["global"].append({"id": "native-oracle", "kind": "suite", "run": "native-oracle", "from": "M1R"})
        rc, ev = self.evaluate(man, "M0")
        self.assertEqual(rc, 0, ev["failures"])
        for m in ("M1R", "M1"):
            rc, ev = self.evaluate(man, m)
            self.assertIn("global:native-oracle", self.failure_ids(ev))
        man["global"][1]["from"] = "M9"
        rc, out = self.evaluate(man, "M1")
        self.assertEqual(rc, 2)
        self.assertTrue(any("from must be a milestone (M0, M1R, M1" in e for e in out["errors"]), out)

    def test_check_reads_every_row_of_the_table_in_order(self):
        man = manifest(**{m: {"units": ["U%d" % (i + 1)]} for i, m in enumerate(MS)})
        plan = ManifestTests.plan_for(man)
        self.assertIn("| **M1R Milestone M1R** |", plan)
        rc, out = self.check(man, plan)
        self.assertEqual((rc, out["errors"]), (0, []))
        lacking = copy.deepcopy(man)
        lacking["milestones"] = [m for m in lacking["milestones"] if m["id"] != "M1R"]
        milestone(lacking, "M1")["units"] += ["U2"]
        rc, out = self.check(lacking, plan)
        self.assertEqual(rc, 1)
        self.assertIn("the plan's table has a row M1R that the manifest lacks", "\n".join(out["errors"]))
        swapped = copy.deepcopy(man)
        swapped["milestones"][1], swapped["milestones"][2] = swapped["milestones"][2], swapped["milestones"][1]
        rc, out = self.check(swapped, plan)
        self.assertEqual(rc, 1)
        self.assertIn("the manifest lists the milestones as M0, M1, M1R, M2, M3, M4, M5; the plan's table orders "
                      "them M0, M1R, M1, M2, M3, M4, M5", "\n".join(out["errors"]))
        rc, out = self.check(man, plan.replace("| **M1R Milestone M1R** |", "| **M1R** |"))
        self.assertEqual(rc, 1)
        self.assertIn("a row that is not | **<id> <name>** |", "\n".join(out["errors"]))


class ReviewsFirstCoverageTests(Sandbox):
    """U240r: the M1R row binds to its definitions like every other row. Its
    first-sync step says "trial" (the sync starts the trial); M1 repeats M1R's
    clauses and states the cumulative rule; a global condition from M1R claims
    the clause M1 repeats."""

    SYNC = ("The first real sync with AnkiWeb, after passing results (U246 sync area, U248) and a backup; "
            "it starts the M1R trial")

    def bound(self):
        self.run_("native", job="fp-probe", tier="T3", cls="conformance")
        per = {m: {"units": ["U%d" % (i + 1)]} for i, m in enumerate(MS)}
        per["M1R"] = {
            "units": ["U246", "U248", "U256"],
            "plan": {"gate": "U246: areas storage and sync, plus the native oracle check (KTD24).",
                     "user_steps": "U256 device run on iPhone, iPad and Mac (AE81). " + self.SYNC},
            "suites": [{"id": "u246", "run": "u246", "pending": "U246",
                        "scope": {"detail": "areas", "items": ["storage", "sync"]},
                        "plan": [{"text": "U246: areas storage and sync", "items": ["storage", "sync"]}]}],
            "user_steps": [
                {"id": "device-run-%s" % d, "description": "run", "evidence": "docs/%s.json" % d,
                 "validator": {"type": "device-run", "device": d}, "plan": ["U256 device run on iPhone, iPad and Mac (AE81)"]}
                for d in ("iphone", "ipad", "mac")] + [
                {"id": "first-sync", "description": "first sync", "evidence": "docs/first-sync.json",
                 "validator": {"type": "first-sync", "results": ["u246-sync-area", "u248-soak"]},
                 "plan": [{"text": self.SYNC, "items": ["u246-sync-area", "u248-soak"]}]}]}
        per["M1"] = {
            "units": ["U47"],
            "plan": {"gate": "Cumulative over M1R: the full gate. U246: areas storage, optimizer and sync, plus the "
                             "native oracle check (KTD24).", "user_steps": ""},
            "cumulative": ["Cumulative over M1R: the full gate"],
            "suites": [{"id": "u246", "run": "u246", "pending": "U246",
                        "scope": {"detail": "areas", "items": ["storage", "optimizer", "sync"]},
                        "plan": [{"text": "U246: areas storage, optimizer and sync",
                                  "items": ["storage", "optimizer", "sync"]}]}]}
        man = manifest(**per)
        man["global"].append({"id": "native-oracle", "kind": "suite", "run": "native", "from": "M1R",
                              "plan": ["plus the native oracle check (KTD24)"]})
        return man

    def test_the_reviews_first_rows_bind_to_their_definitions(self):
        man = self.bound()
        rc, out = self.check(man, ManifestTests.plan_for(man))
        self.assertEqual((rc, out["errors"]), (0, []))

        def step(m, sid):
            return [s for s in milestone(m, "M1R")["user_steps"] if s["id"] == sid][0]

        cases = [
            ("the cumulative rule unclaimed", lambda m: milestone(m, "M1").pop("cumulative"),
             "M1: the closing gate text 'Cumulative over M1R: the full gate'"),
            ("a cumulative excerpt that names no earlier milestone",
             lambda m: milestone(m, "M1").update({"cumulative": ["the full gate"]}), "names no earlier milestone"),
            ("the global condition unbound", lambda m: m["global"][1].pop("plan"),
             "M1: the closing gate text 'plus the native oracle check (KTD24)'"),
            ("the first sync swapped for a report", lambda m: step(m, "first-sync").update(
                {"validator": {"type": "report"}, "plan": [self.SYNC]}), "which asks for a first-sync step"),
            ("the first sync swapped for a trial", lambda m: step(m, "first-sync").update(
                {"validator": {"type": "trial", "days": 7}, "plan": [self.SYNC]}), "which asks for a first-sync step"),
            ("a result the first sync relied on dropped",
             lambda m: step(m, "first-sync")["validator"].update({"results": ["u246-sync-area"]}),
             "binds u248-soak, which its results lacks"),
            ("the first sync removed",
             lambda m: milestone(m, "M1R").update({"user_steps": milestone(m, "M1R")["user_steps"][:3]}),
             "The first real sync with AnkiWeb"),
            ("the Mac device run removed", lambda m: milestone(m, "M1R").update(
                {"user_steps": [s for s in milestone(m, "M1R")["user_steps"] if s["id"] != "device-run-mac"]}),
             "no device-run user step on mac"),
        ]
        for name, mutate, expected in cases:
            with self.subTest(name):
                broken = copy.deepcopy(man)
                mutate(broken)
                rc, out = self.check(broken, ManifestTests.plan_for(broken))
                self.assertEqual(rc, 1, out)
                self.assertTrue(any(expected in e for e in out["errors"]), "%r not in %s" % (expected, out["errors"]))


def first_sync(build, started="2026-11-15T09:00:00Z", **change):
    data = {"build": {"commit": build, "dirty": False}, "sync_started_at": started,
            "results": {rid: {"job": job, "commit": build, "finished_at": "2026-11-15T08:%02d:00Z" % n, "outcome": "pass"}
                        for n, (rid, job) in enumerate((("u246-sync-area", "conformance"), ("u248-soak", "sync-soak"),
                                                         ("u250-sync-classes", "fault")))},
            "backup": {"file": "collection-2026-11-15-0850.colpkg", "taken_at": "2026-11-15T08:50:00Z"}}
    for path, value in change.items():
        obj, keys = data, path.split("__")
        for k in keys[:-1]:
            obj = obj[k]
        if value is None:
            del obj[keys[-1]]
        else:
            obj[keys[-1]] = value
    return data


class FirstSyncTests(Sandbox):
    """U240's first-sync validator (M1R): checked against the results the sync
    relied on and the backup before it, never against the gate commit."""

    EVIDENCE = "docs/process/milestone-records/M1R/first-sync.json"

    def setUp(self):
        super().setUp()
        self.run_("unit", job="core-test")
        self.build = self.commit("The build Antoine synced with", {"app/A.swift": "// a\n"})
        self.commit("A fix made during the trial", {"app/A.swift": "// b\n"})
        self.result("core-test")
        self.gate = self.git("rev-parse", "HEAD")

    def man(self):
        step = {"id": "first-sync", "description": "first sync", "evidence": self.EVIDENCE,
                "validator": {"type": "first-sync", "results": ["u246-sync-area", "u248-soak", "u250-sync-classes"]}}
        return manifest(M0={"suites": [{"id": "unit", "run": "unit"}]}, M1R={"user_steps": [step]})

    def test_absent_evidence_awaits_antoine(self):
        rc, ev = self.evaluate(self.man(), "M1R")
        self.assertEqual((rc, ev["outcome"]), (3, "awaiting_user"), ev["failures"])
        self.assertEqual([a["id"] for a in ev["awaiting"]], ["user-step:first-sync"])

    def test_valid_evidence_on_a_build_older_than_the_gate_commit_is_accepted(self):
        self.commit("Record the first sync", {self.EVIDENCE: first_sync(self.build)})
        rc, ev = self.evaluate(self.man(), "M1R")
        self.assertEqual(rc, 0, ev["failures"])
        self.assertEqual(ev["gate_commit"], self.gate)
        details = ev["user_steps"][0]["details"]
        self.assertEqual(details["build_commit"], self.build)
        self.assertEqual(sorted(details["results"]), ["u246-sync-area", "u248-soak", "u250-sync-classes"])

    def test_invalid_evidence_fails_and_names_the_step(self):
        cases = [
            ({"results__u248-soak__commit": self.gate}, "the u248-soak result is on %s, not the build's commit" % self.gate[:12]),
            ({"results__u246-sync-area__finished_at": "2026-11-15T09:00:00Z"},
             "the u246-sync-area result finished at 2026-11-15T09:00:00Z, not before the sync started"),
            ({"results__u250-sync-classes__outcome": "fail"}, 'the u250-sync-classes result is "fail", not a pass'),
            ({"build__dirty": True}, "not from a clean tree"),
            ({"build__dirty": None}, "not from a clean tree"),
            ({"backup__taken_at": "2026-11-15T09:05:00Z"}, "was taken at 2026-11-15T09:05:00Z, not before the sync"),
            ({"results__u250-sync-classes": None}, "records no u250-sync-classes result"),
            ({"build__commit": "f" * 40}, "build.commit must be a full commit id of this repository"),
            ({"sync_started_at": "yesterday"}, "sync_started_at must be"),
            ({"backup": None}, "expected {build"),
        ]
        for change, expected in cases:
            with self.subTest(expected):
                self.commit("Record the first sync", {self.EVIDENCE: first_sync(self.build, **change)})
                rc, ev = self.evaluate(self.man(), "M1R")
                self.assertEqual((rc, ev["outcome"]), (1, "fail"))
                message = self.failure(ev, "user-step:first-sync")
                self.assertTrue(message.startswith("first-sync: "), message)
                self.assertIn(expected, message)
                self.assertFalse(os.path.exists(self.record))

    def test_m1r_device_runs_await_each_device(self):
        steps = [{"id": "device-run-%s" % d, "description": "run",
                  "evidence": "docs/process/milestone-records/device-runs/M1R/%s.json" % d,
                  "validator": {"type": "device-run", "device": d}} for d in ("iphone", "ipad", "mac")]
        man = manifest(M0={"suites": [{"id": "unit", "run": "unit"}]},
                       M1R={"user_steps": steps, "budgets": {"review": ["R1807", "R1813"]}})
        rc, ev = self.evaluate(man, "M1R")
        self.assertEqual((rc, ev["outcome"]), (3, "awaiting_user"), ev["failures"])
        self.assertEqual([a["id"] for a in ev["awaiting"]],
                         ["user-step:device-run-iphone", "user-step:device-run-ipad", "user-step:device-run-mac"])
        self.assertIn("device-runs/M1R/iphone.json", ev["awaiting"][0]["message"])


class MilestoneRunsTests(Sandbox):
    """`runs`: what `tools/mg milestone <M>` plans (U240's "runs exactly the
    M1R suites"): the runs M's gate reads and the runs they take files from,
    never a later milestone's suite."""

    def runs(self, man, milestone):
        paths = {}
        for name, value in (("manifest", man), ("config", self.config)):
            paths[name] = os.path.join(self.tmp, name + ".json")
            with open(paths[name], "w", encoding="utf-8") as f:
                json.dump(value, f)
        p = subprocess.run([sys.executable, "-B", GATE, "runs", "--manifest", paths["manifest"], "--config",
                            paths["config"], "--milestone", milestone], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        return p.returncode, json.loads(p.stdout.decode() or "{}")

    def reviews_first(self):
        self.run_("unit", job="core-test")
        self.run_("live", job="conformance", args=["--live"], tier="T1", cls="conformance")
        self.run_("harvest", job="harvest-report", args=["--live-report", "{artifact:live:t1-live.json}"], tier="T3",
                  cls="conformance")
        self.run_("review-loop", job="u243", cls="perf")
        self.run_("snap", job="snapshots", cls="ui")
        self.run_("native", job="fp-probe", tier="T3", cls="conformance")
        self.run_("bench", job="bench-mac", tier="T3", cls="perf")
        self.run_("audit", job="release-audit", tier="T3", cls="static", release_only=True)
        self.run_("device", job="device-run", tier="T4", cls="perf", user_step="device-run")
        self.run_("fuzz", job="fuzz", tier="T2", cls="unit")
        man = manifest(
            M0={"suites": [{"id": "unit", "run": "unit"}, {"id": "harvest", "run": "harvest"}]},
            M1R={"suites": [{"id": "review-loop", "run": "review-loop"},
                            {"id": "soak", "run": "u248-soak", "pending": "U248"}],
                 "user_steps": [{"id": "device-run-mac", "description": "run",
                                 "evidence": "docs/process/milestone-records/device-runs/M1R/mac.json",
                                 "validator": {"type": "device-run", "device": "mac"}}],
                 "deferred": [{"id": "foldable", "dependency": "xcode-27.1-foldable-simulator",
                               "condition": FOLDABLE,
                               "cells": {"run": "snap", "scope": {"detail": "devices", "items": ["foldable"]}},
                               "approved_by": "antoine"}]},
            M1={"suites": [{"id": "bench-mac", "run": "bench"}]})
        man["global"].append({"id": "native-oracle", "kind": "suite", "run": "native", "from": "M1R"})
        return man

    def test_m1r_runs_its_gates_runs_and_never_a_later_milestones(self):
        rc, out = self.runs(self.reviews_first(), "M1R")
        self.assertEqual(rc, 0, out)
        # M0's suites (cumulative), M1R's own, the live run whose report
        # harvest takes, the native oracle (global from M1R), the deferred
        # cells' run, and the device run its user step names (listed as
        # awaiting Antoine). Not M1's bench-mac, the release-only audit, T2,
        # or the pending soak, which has no tiers run yet.
        self.assertEqual(out["runs"], ["device", "harvest", "live", "native", "review-loop", "snap", "unit"])

    def test_m0_leaves_out_what_applies_from_m1r(self):
        rc, out = self.runs(self.reviews_first(), "M0")
        self.assertEqual(rc, 0, out)
        self.assertEqual(out["runs"], ["harvest", "live", "unit"])

    def test_m1_adds_its_own_suites(self):
        rc, out = self.runs(self.reviews_first(), "M1")
        self.assertEqual(rc, 0, out)
        # M1 has no device-run step in this manifest: no T4 run is listed.
        self.assertEqual(out["runs"], ["bench", "harvest", "live", "native", "review-loop", "snap", "unit"])

    def test_an_unknown_milestone_or_an_invalid_manifest_selects_nothing(self):
        man = self.reviews_first()
        rc, out = self.runs(man, "M1X")
        self.assertEqual(rc, 2)
        self.assertNotIn("runs", out)
        self.assertIn("M0, M1R, M1, M2, M3, M4, M5", out["errors"][0])
        milestone(man, "M1")["suites"].append({"id": "nowhere", "run": "nowhere"})
        rc, out = self.runs(man, "M1R")
        self.assertEqual(rc, 2)
        self.assertNotIn("runs", out)
        self.assertTrue(any("run nowhere is not in tools/gate/tiers.yaml" in e for e in out["errors"]), out)


if __name__ == "__main__":
    unittest.main()
