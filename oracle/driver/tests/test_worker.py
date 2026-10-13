# SPDX-License-Identifier: AGPL-3.0-or-later
"""Workers end to end: real `entrypoint.sh worker` processes, real anki, real
libfaketime. Expected ids, days and offsets are computed here from the scenario
clock with zoneinfo, independently of the driver."""

from __future__ import annotations

import hashlib
import json
import os
import re
import statistics
import threading
import time
import unittest
from datetime import datetime, timedelta
from zoneinfo import ZoneInfo

from driver.harness import ORACLE_HOME, WorkerProcess
from driver.numbers import to_f32
from driver.tests.test_numbers import naive_shortest_digits, sig_digits

NOTETYPE = {
    "name": "QA",
    "fields": ["Q", "A"],
    "templates": [{"name": "C1", "front": "{{Q}}", "back": "{{A}}"}],
    "css": ".card {}",
}


def scenario(sid, tz, start, steps, notes=0, setup=None):
    base = {
        "config": {"fsrs": True},
        "presets": [{"name": "P", "learn_steps": ["1m", "10m"], "desired_retention": 0.9}],
        "decks": [{"name": "A", "preset": "P"}],
        "notetypes": [NOTETYPE],
        "notes": [{"notetype": "QA", "deck": "A", "fields": [f"q{i}", f"a{i}"]} for i in range(1, notes + 1)],
    }
    base.update(setup or {})
    return {"id": sid, "clock": {"tz": tz, "start": start}, "setup": base, "steps": steps}


def epoch(local: str, tz: str) -> int:
    return int(datetime.fromisoformat(local).replace(tzinfo=ZoneInfo(tz)).timestamp())


def next_rollover(epoch_secs: int, tz: str, hour: int = 4) -> int:
    local = datetime.fromtimestamp(epoch_secs, ZoneInfo(tz))
    candidate = local.replace(hour=hour, minute=0, second=0, microsecond=0)
    if candidate <= local:
        candidate = (candidate + timedelta(days=1)).replace(hour=hour)
    return int(candidate.timestamp())


def run_ok(test, worker, sc):
    resp = worker.request({"cmd": "run", "scenario": sc})
    test.assertTrue(resp["ok"], resp)
    return resp["observations"]["steps"]


class Hello(unittest.TestCase):
    def test_hello_reports_version_lock_driver_and_tz(self):
        with WorkerProcess("Europe/Paris") as w:
            resp = w.request({"cmd": "hello", "id": 7})
        lock = hashlib.sha256((ORACLE_HOME / "requirements.lock").read_bytes()).hexdigest()
        self.assertEqual(resp["ok"], True)
        self.assertEqual(resp["id"], 7)
        self.assertEqual(resp["protocol"], 1)
        self.assertEqual(resp["anki_version"], "26.9.3")
        self.assertEqual(resp["lock_sha256"], lock)
        self.assertRegex(resp["driver_sha256"], r"^[0-9a-f]{64}$")
        self.assertEqual(resp["tz"], "Europe/Paris")
        lockfile = (ORACLE_HOME / "requirements.lock").read_text()
        self.assertRegex(lockfile, r"(?m)^anki==26\.9\.3 ")


class Ids(unittest.TestCase):
    def test_frozen_clock_gives_identical_ids_in_separate_workers(self):
        start = "2030-01-15T10:00:00"
        sc = scenario("t/ids", "Europe/Paris", start, [{"observe": ["ids"]}], notes=3)
        runs = []
        for _ in range(2):
            with WorkerProcess("Europe/Paris") as w:
                runs.append(run_ok(self, w, sc)[0]["ids"])
        t = epoch(start, "Europe/Paris") * 1000
        # Millisecond timestamp from the frozen clock, +1 on each collision.
        expected = {"#n1": t, "#n2": t + 1, "#n3": t + 2, "#c1": t, "#c2": t + 1, "#c3": t + 2}
        self.assertEqual(runs[0], expected)
        self.assertEqual(runs[1], runs[0])


class Clock(unittest.TestCase):
    def test_advancing_within_one_worker_moves_the_scheduler_day(self):
        steps = [{"observe": ["timing"]}]
        for delta in ("20m", "23h40m", "4d", "21d"):  # totals: 20 min, 1 d, 5 d, 26 d
            steps += [{"advance": delta}, {"observe": ["timing"]}]
        sc = scenario("t/advance", "Europe/Paris", "2030-01-15T10:00:00", steps)
        with WorkerProcess("Europe/Paris") as w:
            out = run_ok(self, w, sc)
        timings = [s["timing"] for s in out if "timing" in s]
        self.assertEqual([t["days_elapsed"] for t in timings], [0, 0, 1, 5, 26])
        start = epoch("2030-01-15T10:00:00", "Europe/Paris")
        offsets = [0, 20 * 60, 86400, 5 * 86400, 26 * 86400]
        self.assertEqual([t["now"] for t in timings], [start + o for o in offsets])
        self.assertEqual([t["next_day_at"] for t in timings], [next_rollover(start + o, "Europe/Paris") for o in offsets])

    def test_set_clock_follows_wall_time_across_a_dst_change(self):
        steps = [{"set_clock": "2030-03-31T01:59:59"}, {"observe": ["timing"]},
                 {"set_clock": "2030-03-31T03:00:00"}, {"observe": ["timing"]}]
        sc = scenario("t/dst", "Europe/Paris", "2030-03-30T12:00:00", steps)
        with WorkerProcess("Europe/Paris") as w:
            out = run_ok(self, w, sc)
            gap = scenario("t/gap", "Europe/Paris", "2030-03-31T02:30:00", [])
            err = w.request({"cmd": "run", "scenario": gap})
        self.assertEqual([out[1]["timing"]["utc_offset_secs"], out[3]["timing"]["utc_offset_secs"]], [3600, 7200])
        self.assertEqual(out[3]["timing"]["now"] - out[1]["timing"]["now"], 1)
        self.assertEqual(err["ok"], False)
        self.assertEqual(err["error"]["kind"], "invalid_scenario")

    def test_workers_in_different_zones_run_concurrently_without_cross_talk(self):
        zones = {"Asia/Kathmandu": 20700, "UTC": 0}
        start = "2030-01-15T10:00:00"
        steps = [{"observe": ["timing"]}, {"advance": "1d"}, {"observe": ["timing"]}]
        results: dict[str, list] = {}
        errors: list = []

        def drive(tz):
            try:
                with WorkerProcess(tz) as w:
                    hello_tz = w.hello["tz"]
                    runs = [run_ok(self, w, scenario(f"t/{tz}", tz, start, steps, notes=1)) for _ in range(5)]
                results[tz] = [hello_tz, runs]
            except Exception as exc:  # noqa: BLE001
                errors.append(exc)

        threads = [threading.Thread(target=drive, args=(tz,)) for tz in zones]
        for t in threads:
            t.start()
        for t in threads:
            t.join()
        self.assertEqual(errors, [])
        for tz, offset in zones.items():
            hello_tz, runs = results[tz]
            self.assertEqual(hello_tz, tz)
            s = epoch(start, tz)
            for out in runs:
                first, second = out[0]["timing"], out[2]["timing"]
                self.assertEqual(first["utc_offset_secs"], offset)
                self.assertEqual(first["now"], s)
                self.assertEqual(second["now"], s + 86400)
                self.assertEqual(first["next_day_at"], next_rollover(s, tz))
                self.assertEqual(second["days_elapsed"], first["days_elapsed"] + 1)


class Errors(unittest.TestCase):
    def test_unknown_op_is_a_structured_error_and_the_worker_lives_on(self):
        bad = scenario("t/bad", "UTC", "2030-01-15T10:00:00", [{"observe": ["ids"]}, {"frobnicate": {"x": 1}}], notes=1)
        good = scenario("t/good", "UTC", "2030-01-15T10:00:00", [{"observe": ["ids"]}], notes=1)
        with WorkerProcess("UTC") as w:
            err = w.request({"cmd": "run", "id": "bad", "scenario": bad})
            unknown_obs = w.request({"cmd": "run", "scenario": dict(good, steps=[{"observe": ["horoscope"]}])})
            ok = w.request({"cmd": "run", "scenario": good})
            hello = w.request({"cmd": "hello"})
            # Every scenario's collection directory is gone, failed ones included.
            leftovers = sorted(os.listdir(w.scratch))
        self.assertEqual(err, {"ok": False, "id": "bad", "error": {"kind": "unsupported_op", "op": "frobnicate", "step": 1}})
        self.assertEqual(unknown_obs["error"], {"kind": "unsupported_observation", "observation": "horoscope", "step": 0})
        self.assertTrue(ok["ok"])
        self.assertTrue(hello["ok"])
        self.assertEqual(leftovers, ["clock"])

    def test_scenario_in_another_zone_is_refused(self):
        with WorkerProcess("UTC") as w:
            err = w.request({"cmd": "run", "scenario": scenario("t/tz", "Europe/Paris", "2030-01-15T10:00:00", [])})
        self.assertEqual(err["error"], {"kind": "tz_mismatch", "scenario_tz": "Europe/Paris", "worker_tz": "UTC"})

    def test_anki_failures_carry_no_message_text(self):
        steps = [{"observe": [{"fuzz_table": {"card": "#c2", "pairs": [["#c1"]], "intervals": [1]}}]}]
        with WorkerProcess("UTC") as w:
            err = w.request({"cmd": "run", "scenario": scenario("t/x", "UTC", "2030-01-15T10:00:00", steps, notes=2)})
            # Rewriting card 2 to card 1's id violates the cards table's key.
            t = epoch("2030-01-15T10:00:00", "UTC") * 1000
            steps = [{"observe": [{"fuzz_table": {"card": "#c2", "pairs": [[t, 1]], "intervals": [1]}}]}]
            dup = w.request({"cmd": "run", "scenario": scenario("t/y", "UTC", "2030-01-15T10:00:00", steps, notes=2)})
        self.assertEqual(err["error"]["kind"], "invalid_scenario")
        self.assertEqual(dup["error"], {"kind": "anki_error", "op": "observe", "exception": "DBError", "step": 0})


class Throughput(unittest.TestCase):
    def test_one_warm_worker_runs_small_scenarios_quickly(self):
        steps = [{"answer": {"card": "#c1", "rating": "good"}}, {"observe": ["card(#c1)", "next_states(#c1)"]}]
        walls = []
        with WorkerProcess("Europe/Paris") as w:
            run_ok(self, w, scenario("t/warm", "Europe/Paris", "2030-01-15T10:00:00", steps, notes=1))
            for i in range(100):
                sc = scenario(f"t/s{i}", "Europe/Paris", "2030-01-15T10:00:00", steps, notes=1)
                started = time.perf_counter()
                run_ok(self, w, sc)
                walls.append((time.perf_counter() - started) * 1000)
        mean = statistics.mean(walls)
        self.diagnostics = [f"mean {mean:.1f} ms, p95 {sorted(walls)[94]:.1f} ms over 100 scenarios"]
        self.assertLessEqual(mean, 150.0, self.diagnostics[0])


class FloatTrap(unittest.TestCase):
    def test_memory_states_on_the_wire_are_shortest_f32(self):
        steps = [{"answer": {"card": "#c1", "rating": "good"}}, {"advance": "3d"},
                 {"observe": ["next_states(#c1)"]}]
        with WorkerProcess("UTC") as w:
            sc = scenario("t/f", "UTC", "2030-01-15T10:00:00", steps, notes=1)
            w.proc.stdin.write(json.dumps({"cmd": "run", "scenario": sc}) + "\n")
            w.proc.stdin.flush()
            line = w.proc.stdout.readline()
        values = re.findall(r'"(?:stability|difficulty|ease_factor)":(-?[0-9.e+-]+)', line)
        self.assertGreater(len(values), 4)
        widened = 0
        for text in values:
            v32 = to_f32(float(text))
            # As short as the shortest %g text that still parses to the same f32.
            self.assertLessEqual(sig_digits(text), naive_shortest_digits(v32), text)
            widened += repr(v32) != text
        self.assertGreater(widened, 0, "no value would have printed differently as a widened f64")


class Interactive(unittest.TestCase):
    def test_begin_step_end(self):
        sc = scenario("t/i", "UTC", "2030-01-15T10:00:00",
                      [{"answer": {"card": "#c1", "rating": "easy"}}, {"observe": ["card(#c1)"]}], notes=1)
        with WorkerProcess("UTC") as w:
            begun = w.request({"cmd": "begin", "scenario": sc})
            busy = w.request({"cmd": "run", "scenario": sc})
            first = w.request({"cmd": "step"})
            adhoc = w.request({"cmd": "step", "step": {"observe": ["ids(#c1)"]}})
            ended = w.request({"cmd": "end"})
            after = w.request({"cmd": "step"})
        self.assertEqual(begun, {"ok": True, "scenario_id": "t/i", "steps": 2})
        self.assertEqual(busy["error"]["kind"], "session_open")
        self.assertEqual(first, {"ok": True, "index": 0, "observation": {}})
        t = epoch("2030-01-15T10:00:00", "UTC") * 1000
        self.assertEqual(adhoc, {"ok": True, "index": 1, "observation": {"ids": {"#c1": t}}})
        self.assertEqual(ended["observations"]["steps"], [{}, {"ids": {"#c1": t}}])
        self.assertEqual(after["error"]["kind"], "no_session")


class Isolation(unittest.TestCase):
    def test_no_route_to_public_addresses(self):
        probes = [{"net_probe": {"host": "1.1.1.1", "port": 443}}, {"net_probe": {"host": "8.8.8.8", "port": 53}}]
        steps = [{"observe": [p]} for p in probes]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario("t/net", "UTC", "2030-01-15T10:00:00", steps))
        for step in out:
            self.assertEqual(step["net_probe"]["connected"], False, step)
            self.assertIn(step["net_probe"]["error"], ("ENETUNREACH", "EHOSTUNREACH", "timeout"), step)


class ExtensionPoint(unittest.TestCase):
    def test_extra_modules_add_ops_and_stdout_noise_cannot_break_the_stream(self):
        env = {"ORACLE_EXTRA_MODULES": "driver.tests.extra_area"}
        sc = scenario("t/ext", "UTC", "2030-01-15T10:00:00", [{"shout": "hi"}, {"observe": ["echo(#c1)"]}], notes=1)
        with WorkerProcess("UTC", extra_env=env) as w:
            out = run_ok(self, w, sc)
        self.assertEqual(out, [{"shout": "HI"}, {"echo": ["#c1"]}])

    def test_fuzz_stays_on_even_if_anki_test_mode_leaks_in(self):
        t = epoch("2030-01-15T10:00:00", "UTC") * 1000 + 1000
        pairs = [[t + i, i] for i in range(20)]
        steps = [{"observe": [{"fuzz_table": {"card": "#c1", "pairs": pairs, "intervals": [100]}}]}]
        with WorkerProcess("UTC", extra_env={"ANKI_TEST_MODE": "1"}) as w:
            out = run_ok(self, w, scenario("t/fz", "UTC", "2030-01-15T10:00:00", steps, notes=1))
        deltas = [row[2] for row in out[0]["fuzz_table"]["rows"]]
        self.assertTrue(any(deltas), deltas)  # Anki's test mode would make every delta 0


if __name__ == "__main__":
    unittest.main()
