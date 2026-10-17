# SPDX-License-Identifier: AGPL-3.0-or-later
"""Scheduling ops on chosen cards (specs/conformance/sched-ops.md J1-J9)
through real workers. Expected values come from the arguments and the behavior
specs (specs/sched/manual-ops.md M2-M8, specs/sched/set-due-date.md S2-S8, the
step ids of undo-ops.md V4), never from the driver."""

from __future__ import annotations

import json
import unittest

from driver.harness import WorkerProcess
from driver.tests.test_worker import epoch, run_ok

START = "2030-01-15T10:00:00"
T0 = epoch(START, "UTC")
# The scheduler day starts at 04:00 (sched/day-clock): today is day 0 from
# START until NEXT_DAY.
NEXT_DAY = epoch("2030-01-16T04:00:00", "UTC")
SINGLE = {"name": "Single", "fields": ["Front", "Back"], "templates": [{"name": "One", "front": "{{Front}}", "back": "{{Back}}"}]}
PAIR = {
    "name": "Pair",
    "fields": ["Front", "Back"],
    "templates": [
        {"name": "Forward", "front": "{{Front}}", "back": "{{Back}}"},
        {"name": "Reverse", "front": "{{Back}}", "back": "{{Front}}"},
    ],
}
# deck-ops.md K13: a filtered deck row as a build leaves it.
CRAM = {"db_exec": {"sql": "INSERT INTO decks (id, name, mtime_secs, usn, common, kind) VALUES "
                           "(100, 'Cram', 0, 0, X'08011001', X'12130801120a0a066465636b3a78106428d804383c')"}}
SERVER_USN = {"db_exec": {"sql": "UPDATE cards SET usn = 0"}}
COL_MOD = {"db_exec": {"sql": "SELECT mod FROM col"}}
CARD_IDS = {"db_exec": {"sql": "SELECT id FROM cards ORDER BY id"}}
QUEUES = {"db_exec": {"sql": "SELECT queue, mod, usn FROM cards ORDER BY id"}}
REVLOG = {"db_exec": {"sql": "SELECT id, cid, ease, ivl, lastIvl, factor, time, type, usn FROM revlog ORDER BY id"}}
REVLOG_COUNT = {"db_exec": {"sql": "SELECT count(*) FROM revlog"}}
UNDO = {"observe": ["undo_status"]}


def scenario(steps, notes, decks=("Home",)):
    """notes: (notetype, deck, front) per note, in order."""
    setup = {
        "config": {"fsrs": True, "loadBalancerEnabled": False},
        "notetypes": [SINGLE, PAIR],
        "decks": list(decks),
        "notes": [{"notetype": nt, "deck": deck, "fields": [front, "x"]} for nt, deck, front in notes],
    }
    return {"id": "t/sched-manual", "clock": {"tz": "UTC", "start": START}, "setup": setup, "steps": steps}


def singles(*fronts, deck="Home"):
    return [("Single", deck, f) for f in fronts]


def card(n, updates, params=None):
    """Produce a raw update for a single card selected by numeric order."""
    sql = f"UPDATE cards SET {updates} WHERE id = (SELECT id FROM cards ORDER BY id LIMIT 1 OFFSET {n - 1})"
    return {"db_exec": {"sql": sql, **({"params": params} if params else {})}}


def rows(step):
    return step["db_exec"]["rows"]


def status(undo, redo=None):
    return {"undo_status": {"undo": undo, "redo": redo}}


def run(worker, sc):
    return worker.request({"cmd": "run", "scenario": sc})


class BuryAndSuspend(unittest.TestCase):
    def test_bury_cards_follows_the_queue_table_and_writes_only_what_changes(self):
        steps = [
            card(1, "queue = -1"),
            card(2, "queue = -3"),
            card(3, "queue = -2"),
            card(5, "type = 2, queue = 2, ivl = 5, due = 3, factor = 2500"),
            card(6, f"type = 1, queue = 1, due = {T0 + 600}, left = 1"),
            SERVER_USN,
            {"advance": "1m"},
            {"bury": {"cards": ["#c6", "#c5", "#c4", "#c3", "#c2", "#c1"]}},
            QUEUES,
            UNDO,
            REVLOG_COUNT,
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario(steps, singles("a", "b", "c", "d", "e", "f")))
        now = T0 + 60
        # M2: suspended and user-buried cards are left alone (not written);
        # every other queue becomes -3, stamped.
        self.assertEqual(out[7], {})
        self.assertEqual(rows(out[8]), [[-1, T0, 0], [-3, T0, 0]] + [[-3, now, -1]] * 4)
        self.assertEqual(out[9], status("bury"))
        self.assertEqual(rows(out[10]), [[0]])

    def test_bury_and_suspend_notes_take_every_card_of_the_notes(self):
        steps = [
            card(2, "queue = -1"),
            SERVER_USN,
            {"advance": "1m"},
            {"bury": {"notes": ["#n1"]}},
            QUEUES,
            UNDO,
            {"advance": "1m"},
            {"suspend": {"notes": ["#n2", "#n1"]}},
            QUEUES,
            UNDO,
        ]
        notes = [("Pair", "Home", "a"), ("Pair", "Home", "b")]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario(steps, notes))
        # M2: Bury Note skips the note's suspended card (#c2).
        self.assertEqual(rows(out[4]), [[-3, T0 + 60, -1], [-1, T0, 0], [0, T0, 0], [0, T0, 0]])
        self.assertEqual(out[5], status("bury"))
        # M3: a buried card can be suspended; a suspended one is not written.
        self.assertEqual(rows(out[8]), [[-1, T0 + 120, -1], [-1, T0, 0], [-1, T0 + 120, -1], [-1, T0 + 120, -1]])
        self.assertEqual(out[9], status("suspend"))

    def test_suspend_cards_follows_the_queue_table(self):
        steps = [
            card(1, "queue = -1"),
            card(2, "queue = -3"),
            card(3, "queue = -2"),
            SERVER_USN,
            {"advance": "1m"},
            {"suspend": {"cards": ["#c4", "#c3", "#c2", "#c1"]}},
            QUEUES,
            UNDO,
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario(steps, singles("a", "b", "c", "d")))
        self.assertEqual(rows(out[6]), [[-1, T0, 0]] + [[-1, T0 + 60, -1]] * 3)
        self.assertEqual(out[7], status("suspend"))

    def test_unsuspend_restores_the_queue_of_the_type_and_leaves_active_cards(self):
        steps = [
            card(1, "queue = -1"),
            card(2, "type = 2, queue = -3, ivl = 4, due = 5, factor = 2500"),
            card(3, f"type = 1, queue = -2, due = {T0 + 600}, left = 1"),
            card(4, "type = 3, queue = -1, ivl = 2, due = 1, factor = 2500, left = 1"),
            SERVER_USN,
            {"advance": "1m"},
            {"unsuspend": {"cards": ["#c5", "#c4", "#c3", "#c2", "#c1"]}},
            QUEUES,
            UNDO,
            REVLOG_COUNT,
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario(steps, singles("a", "b", "c", "d", "e")))
        now = T0 + 60
        # M4 with sched/card-data K3: new 0, review 2, learning due at a second
        # 1, relearning due on a day 3; the active #c5 is not written.
        self.assertEqual(rows(out[7]), [[0, now, -1], [2, now, -1], [1, now, -1], [3, now, -1], [0, T0, 0]])
        self.assertEqual(out[8], status("unbury_unsuspend"))
        self.assertEqual(rows(out[9]), [[0]])

    def test_a_deleted_card_or_note_is_passed_on_and_skipped(self):
        steps = [
            {"delete_notes": {"notes": ["#n2"]}},
            {"advance": "1m"},
            {"bury": {"cards": ["#c2", "#c1"]}},
            {"suspend": {"notes": ["#n2", "#n3"]}},
            {"db_exec": {"sql": "SELECT queue FROM cards ORDER BY id"}},
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario(steps, singles("a", "b", "c")))
        self.assertEqual(rows(out[4]), [[-3], [-1]])


class UnburyDeck(unittest.TestCase):
    def test_the_three_modes_on_a_deck_tree(self):
        # The example of manual-ops M5.
        steps = [
            card(1, "queue = -2"),
            card(2, "queue = -3"),
            card(3, f"type = 1, queue = -2, due = {T0 + 600}, left = 1"),
            card(4, "type = 1, queue = -2, due = 3, left = 1"),
            {"advance": "1m"},
            {"unbury_deck": {"deck": "Home", "mode": "siblings"}},
            {"db_exec": {"sql": "SELECT queue FROM cards ORDER BY id"}},
            UNDO,
            {"unbury_deck": {"deck": "Other", "mode": "all"}},
            {"db_exec": {"sql": "SELECT queue FROM cards ORDER BY id"}},
            {"unbury_deck": {"deck": "Home", "mode": "manual"}},
            {"db_exec": {"sql": "SELECT queue FROM cards ORDER BY id"}},
        ]
        notes = singles("s1", "s2", deck="Home::Sub") + singles("o1", "o2", deck="Other")
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario(steps, notes, decks=("Home", "Home::Sub", "Other")))
        self.assertEqual(rows(out[6]), [[0], [-3], [-2], [-2]])
        self.assertEqual(out[7], status("unbury_unsuspend"))
        self.assertEqual(rows(out[9]), [[0], [-3], [1], [3]])
        self.assertEqual(rows(out[11]), [[0], [0], [1], [3]])

    def test_decks_are_found_by_their_stored_name_at_the_step(self):
        steps = [
            CRAM,
            card(1, "odid = did, did = 100, odue = due, due = -100000, queue = -3"),
            {"unbury_deck": {"deck": "Cram", "mode": "manual"}},
            {"db_exec": {"sql": "SELECT queue, did FROM cards"}},
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario(steps, singles("a")))
            # K1: compared exactly, no case folding; a missing deck is unknown_ref.
            for name in ("home", "Nope", "Home::"):
                resp = run(w, scenario([{"unbury_deck": {"deck": name, "mode": "all"}}], singles("a")))
                with self.subTest(name=name):
                    self.assertEqual(resp["error"], {"kind": "unknown_ref", "ref": name, "step": 0}, resp)
        # K3 on the filtered due: a new card comes back to queue 0 in Cram.
        self.assertEqual(rows(out[3]), [[0, 100]])


class SetDue(unittest.TestCase):
    def test_the_five_cards_of_s8_in_one_operation(self):
        lrt = NEXT_DAY - 496_800
        memory = {"s": 4.5, "d": 5.048, "lrt": lrt}
        steps = [
            card(1, "type = 2, queue = 2, ivl = 10, factor = 2500, due = 5, data = ?", [json.dumps(memory)]),
            card(3, "type = 2, queue = -1, ivl = 10, due = 2, factor = 2300"),
            card(4, f"type = 3, queue = 1, ivl = 4, factor = 0, left = 1, due = {T0 + 60 + 600}"),
            card(5, f"type = 1, queue = 1, ivl = 0, left = 2, due = {T0 + 60 + 600}"),
            SERVER_USN,
            {"advance": "1m"},
            {"set_due": {"cards": ["#c5", "#c4", "#c3", "#c2", "#c1"], "days": "3", "context": "browser"}},
            {"db_exec": {"sql": "SELECT type, queue, due, ivl, factor, left, mod, usn FROM cards ORDER BY id"}},
            {"db_exec": {"sql": "SELECT data FROM cards ORDER BY id LIMIT 2"}},
            REVLOG,
            CARD_IDS,
            {"db_exec": {"sql": "SELECT CAST(val AS TEXT), usn FROM config WHERE KEY = 'setDueBrowser'"}},
            UNDO,
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario(steps, singles("a", "b", "c", "d", "e")))
        now = T0 + 60
        self.assertEqual(out[6], {})
        self.assertEqual(rows(out[7]), [
            [2, 2, 3, 8, 2500, 0, now, -1],
            [2, 2, 3, 0, 2500, 0, now, -1],
            [2, 2, 3, 11, 2300, 0, now, -1],
            [2, 2, 3, 7, 2500, 1, now, -1],
            [2, 2, 3, 0, 2500, 2, now, -1],
        ])
        data = [json.loads(d) for (d,) in rows(out[8])]
        self.assertEqual(data, [memory, {"pos": 2}])
        # S5 and S7: one manual row per card, ids in card id order whatever
        # the listed order; d 5.048 gives factor 549 (truncated).
        cids = [cid for (cid,) in rows(out[10])]
        expected = [(0, 8, 10, 549), (0, 0, 0, 2500), (0, 11, 10, 2300), (0, 7, 4, 2500), (0, 0, 0, 2500)]
        self.assertEqual(rows(out[9]), [
            [now * 1000 + k, cids[k], ease, ivl, last, factor, 0, 4, -1]
            for k, (ease, ivl, last, factor) in enumerate(expected)
        ])
        # S6: the text as typed, a JSON string.
        self.assertEqual(rows(out[11]), [['"3"', -1]])
        self.assertEqual(out[12], status("set_due_date"))

    def test_a_refused_text_is_invalid_days_and_writes_nothing(self):
        texts = ["abc", "-1", "3--4", "", " 3", "٣", "4294967296"]
        steps = [
            SERVER_USN,
            COL_MOD,
            {"advance": "1m"},
            *({"set_due": {"cards": ["#c1"], "days": t, "context": "reviewer"}} for t in texts),
            QUEUES,
            REVLOG_COUNT,
            COL_MOD,
            {"db_exec": {"sql": "SELECT count(*) FROM config WHERE KEY LIKE 'setDue%'"}},
            {"set_due": {"cards": ["#c1"], "days": "007"}},
            {"db_exec": {"sql": "SELECT type, queue, due FROM cards"}},
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario(steps, singles("a")))
        n = 3 + len(texts)
        self.assertEqual(out[3:n], [{"set_due": {"error": "invalid_days"}}] * len(texts))
        self.assertEqual(rows(out[n]), [[0, T0, 0]])
        self.assertEqual(rows(out[n + 1]), [[0]])
        self.assertEqual(out[n + 2], out[1])
        self.assertEqual(rows(out[n + 3]), [[0]])
        # S2: leading zeros are allowed.
        self.assertEqual(rows(out[n + 5]), [[2, 2, 7]])

    def test_a_card_refused_by_anki_is_a_run_error_not_invalid_days(self):
        # S4.1: a damaged row (odid 0) inside a filtered deck has a filtered
        # home deck, which Anki refuses with invalid input; the text is fine.
        steps = [CRAM, card(1, "did = 100"), {"set_due": {"cards": ["#c1"], "days": "3"}}]
        with WorkerProcess("UTC") as w:
            resp = run(w, scenario(steps, singles("a")))
        self.assertEqual(resp["error"], {"kind": "anki_error", "op": "set_due", "exception": "InvalidInput", "step": 2})

    def test_malformed_arguments(self):
        bad = [
            {"cards": ["#c1"], "days": 3},
            {"cards": ["#c1"]},
            {"cards": ["#c1"], "days": "3", "context": "editor"},
            {"cards": ["#c1"], "days": "3", "context": None},
            {"cards": ["#c1"], "days": "3", "colour": "red"},
        ]
        with WorkerProcess("UTC") as w:
            for args in bad:
                resp = run(w, scenario([{"set_due": args}], singles("a")))
                with self.subTest(args=args):
                    self.assertEqual((resp["error"]["kind"], resp["error"].get("step")), ("invalid_scenario", 0), resp)


class Reset(unittest.TestCase):
    STATES = [
        card(1, "type = 2, queue = 2, due = 5, ivl = 10, factor = 2500, reps = 4, lapses = 1"),
        card(4, "type = 2, queue = -1, due = 5, ivl = 3, factor = 2500, reps = 3, lapses = 2"),
        card(5, "type = 2, queue = 2, due = 4, ivl = 6, factor = 2500, reps = 2, data = '{\"pos\":2}'"),
        SERVER_USN,
        {"advance": "1m"},
    ]
    CARDS = {"db_exec": {"sql": "SELECT type, queue, due, ivl, factor, reps, lapses, mod, usn FROM cards ORDER BY id"}}
    KEYS = {"db_exec": {"sql": "SELECT KEY, CAST(val AS TEXT) FROM config WHERE KEY IN "
                               "('nextPos', 'restorePositionBrowser', 'resetCountsBrowser', "
                               "'restorePositionReviewer', 'resetCountsReviewer') ORDER BY KEY"}}

    def reset(self, forget):
        steps = [*self.STATES, {"forget": forget}, self.CARDS, REVLOG, CARD_IDS, self.KEYS, UNDO]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario(steps, singles("a", "b", "c", "d", "e", "f")))
        k = len(self.STATES)
        self.assertEqual(out[k], {})
        return rows(out[k + 1]), rows(out[k + 2]), [cid for (cid,) in rows(out[k + 3])], rows(out[k + 4]), out[k + 5]

    def markers(self, cids, order, last_ivls):
        """M6.3: one marker per card, ids in the listed order."""
        now_ms = (T0 + 60) * 1000
        return [[now_ms + k, cids[n - 1], 0, 0, last, 0, 0, 4, -1] for k, (n, last) in enumerate(zip(order, last_ivls))]

    def test_restore_position_from_the_browser(self):
        cards, revlog, cids, keys, undo = self.reset(
            {"cards": ["#c1", "#c3", "#c4", "#c5"], "restore_position": True, "context": "browser"})
        now = T0 + 60
        # M6.2's table, "restore on": 7, 3, 8, 2 in the listed order; nextPos 9.
        self.assertEqual([row[2] for row in cards], [7, 2, 3, 8, 2, 6])
        self.assertEqual([row[:2] + row[3:] for row in cards], [
            [0, 0, 0, 0, 4, 1, now, -1],
            [0, 0, 0, 0, 0, 0, T0, 0],
            [0, 0, 0, 0, 0, 0, now, -1],
            [0, 0, 0, 0, 3, 2, now, -1],
            [0, 0, 0, 0, 2, 0, now, -1],
            [0, 0, 0, 0, 0, 0, T0, 0],
        ])
        self.assertEqual(revlog, self.markers(cids, [1, 3, 4, 5], [10, 0, 3, 6]))
        self.assertEqual(keys, [["nextPos", "9"], ["resetCountsBrowser", "false"], ["restorePositionBrowser", "true"]])
        self.assertEqual(undo, status("reset_card"))

    def test_end_of_queue_with_reset_counts_from_the_review_screen(self):
        cards, revlog, cids, keys, _ = self.reset(
            {"cards": ["#c1", "#c3", "#c4", "#c5"], "reset_counts": True, "context": "reviewer"})
        # M6.2's table, "restore off": 7, 8, 9, 10; nextPos 11; counts 0.
        self.assertEqual([(row[2], row[5], row[6]) for row in cards],
                         [(7, 0, 0), (2, 0, 0), (8, 0, 0), (9, 0, 0), (10, 0, 0), (6, 0, 0)])
        self.assertEqual(revlog, self.markers(cids, [1, 3, 4, 5], [10, 0, 3, 6]))
        self.assertEqual(keys, [["nextPos", "11"], ["resetCountsReviewer", "true"], ["restorePositionReviewer", "false"]])

    def test_defaults_and_no_context(self):
        cards, _, _, keys, _ = self.reset({"cards": ["#c1"]})
        # J7: both choices off when missing; no context writes no choice key.
        self.assertEqual((cards[0][2], cards[0][5], cards[0][6]), (7, 4, 1))
        self.assertEqual(keys, [["nextPos", "8"]])

    def test_malformed_arguments(self):
        bad = [
            {"cards": ["#c1"], "restore_position": 1},
            {"cards": ["#c1"], "reset_counts": None},
            {"cards": ["#c1"], "context": "deck"},
            {"cards": ["#c1", "#c1"]},
            {"cards": []},
            {"restore_position": True},
        ]
        with WorkerProcess("UTC") as w:
            for args in bad:
                resp = run(w, scenario([{"forget": args}], singles("a")))
                with self.subTest(args=args):
                    self.assertEqual((resp["error"]["kind"], resp["error"].get("step")), ("invalid_scenario", 0), resp)


class GradeNow(unittest.TestCase):
    def test_the_example_of_m7_in_the_listed_order(self):
        steps = [
            card(1, "type = 2, queue = 2, ivl = 3, due = 5, factor = 2500, reps = 3"),
            card(2, "queue = -2"),
            card(3, "queue = -1"),
            {"advance": "1m"},
            {"grade_now": {"cards": ["#c2", "#c3", "#c1"], "rating": "good"}},
            {"db_exec": {"sql": "SELECT type, queue, left FROM cards ORDER BY id LIMIT 2 OFFSET 1"}},
            {"db_exec": {"sql": "SELECT r.id, c.id, r.ease, r.ivl, r.lastIvl, r.factor, r.time, r.type "
                                "FROM revlog r JOIN cards c ON c.id = r.cid ORDER BY r.id"}},
            CARD_IDS,
            UNDO,
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario(steps, singles("a", "b", "c")))
        now_ms = (T0 + 60) * 1000
        cids = [cid for (cid,) in rows(out[7])]
        self.assertEqual(out[4], {})
        # Both new cards become learning cards whatever their queue before.
        self.assertEqual(rows(out[5]), [[1, 1, 1], [1, 1, 1]])
        logged = rows(out[6])
        self.assertEqual([row[:2] for row in logged], [[now_ms, cids[1]], [now_ms + 1, cids[2]], [now_ms + 2, cids[0]]])
        self.assertEqual([row[2:] for row in logged[:2]], [[3, -600, 0, 224, 0, 0]] * 2)
        self.assertEqual((logged[2][2], logged[2][4], logged[2][6], logged[2][7]), (3, 3, 0, 1))
        self.assertEqual(out[8], status("grade_now"))

    def test_a_deleted_card_is_a_run_error(self):
        steps = [{"delete_notes": {"notes": ["#n2"]}}, {"grade_now": {"cards": ["#c1", "#c2"], "rating": "easy"}}]
        with WorkerProcess("UTC") as w:
            resp = run(w, scenario(steps, singles("a", "b")))
        self.assertEqual((resp["error"]["kind"], resp["error"]["op"], resp["error"]["step"]), ("anki_error", "grade_now", 1))

    def test_malformed_arguments(self):
        bad = [{"cards": ["#c1"]}, {"cards": ["#c1"], "rating": 3}, {"cards": ["#c1"], "rating": "great"},
               {"cards": ["#c1", "#c1"], "rating": "good"}]
        with WorkerProcess("UTC") as w:
            for args in bad:
                resp = run(w, scenario([{"grade_now": args}], singles("a")))
                with self.subTest(args=args):
                    self.assertEqual((resp["error"]["kind"], resp["error"].get("step")), ("invalid_scenario", 0), resp)


class EmptyFiltered(unittest.TestCase):
    def test_the_table_of_m8_and_the_refusal_of_a_normal_deck(self):
        epoch_due = T0 + 600
        steps = [
            CRAM,
            card(1, "odid = did, did = 100, odue = due, due = -100000"),
            card(2, "type = 2, queue = 2, ivl = 3, factor = 2500, odid = did, did = 100, odue = 5, due = -99999"),
            card(3, f"type = 1, queue = 4, left = 1, odid = did, did = 100, odue = {epoch_due}, due = {epoch_due - 60}"),
            card(4, "type = 3, queue = 4, ivl = 2, factor = 2500, left = 1, odid = did, did = 100, odue = 2, "
                    f"due = {epoch_due}"),
            card(5, "type = 3, queue = -3, ivl = 2, factor = 2500, left = 1, odid = did, did = 100, odue = 1, "
                    "due = -99998"),
            card(6, "type = 2, queue = -1, ivl = 6, factor = 2500, odid = did, did = 100, odue = 0, due = 2"),
            card(7, "did = 100, due = 8"),
            SERVER_USN,
            {"advance": "1m"},
            COL_MOD,
            {"empty_filtered": {"deck": "Home"}},
            COL_MOD,
            {"empty_filtered": {"deck": "Cram"}},
            {"db_exec": {"sql": "SELECT (SELECT name FROM decks WHERE id = c.did), queue, due, odid, odue, mod, usn "
                                "FROM cards c ORDER BY id"}},
            {"db_exec": {"sql": "SELECT name, mtime_secs, usn FROM decks WHERE id = 100"}},
            REVLOG_COUNT,
            UNDO,
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario(steps, singles("a", "b", "c", "d", "e", "f", "g", "h")))
        now = T0 + 60
        # J9: a normal deck is refused and nothing changes.
        self.assertEqual(out[11], {"empty_filtered": {"error": "not_filtered"}})
        self.assertEqual(out[12], out[10])
        self.assertEqual(out[13], {})
        self.assertEqual(rows(out[14]), [
            ["Home", 0, 1, 0, 0, now, -1],
            ["Home", 2, 5, 0, 0, now, -1],
            ["Home", 1, epoch_due, 0, 0, now, -1],
            ["Home", 3, 2, 0, 0, now, -1],
            ["Home", -3, 1, 0, 0, now, -1],
            ["Home", -1, 2, 0, 0, now, -1],
            ["Cram", 0, 8, 0, 0, now, -1],
            ["Home", 0, 8, 0, 0, T0, 0],
        ])
        self.assertEqual(rows(out[15]), [["Cram", 0, 0]])
        self.assertEqual(rows(out[16]), [[0]])
        self.assertEqual(out[17], status("empty"))

    def test_unknown_decks_and_malformed_arguments(self):
        with WorkerProcess("UTC") as w:
            resp = run(w, scenario([{"empty_filtered": {"deck": "Cram"}}], singles("a")))
            self.assertEqual(resp["error"], {"kind": "unknown_ref", "ref": "Cram", "step": 0})
            for args in ({}, {"deck": ""}, {"deck": 100}, {"deck": "Home", "mode": "all"}, "Home"):
                resp = run(w, scenario([{"empty_filtered": args}], singles("a")))
                with self.subTest(args=args):
                    self.assertEqual((resp["error"]["kind"], resp["error"].get("step")), ("invalid_scenario", 0), resp)


class SharedRules(unittest.TestCase):
    def test_lists_are_checked_before_any_call(self):
        bad = {
            "bury": [{"cards": ["#c1", "#c1"]}, {"notes": ["#n1", "#n1"]}, {"cards": ["#c1"], "notes": ["#n1"]},
                     {"cards": []}, {"notes": []}, {}, {"cards": "#c1"}, {"notes": ["#c1"]}, ["#c1"]],
            "suspend": [{"cards": ["#c2", "#c1", "#c2"]}, {"notes": ["#n1", "#n1"]}, {"cards": ["#n1"]},
                        {"cards": ["#c1"], "notes": ["#n1"]}],
            "unsuspend": [{"cards": ["#c1", "#c1"]}, {"notes": ["#n1"]}, {"cards": []}],
            "unbury_deck": [{"deck": "Home"}, {"deck": "Home", "mode": "user"}, {"deck": "", "mode": "all"},
                            {"deck": "Home", "mode": "all", "x": 1}],
        }
        steps_before = [card(1, "queue = -2"), {"advance": "1m"}]
        with WorkerProcess("UTC") as w:
            for name, cases in bad.items():
                for args in cases:
                    resp = run(w, scenario([*steps_before, {name: args}], singles("a", "b")))
                    with self.subTest(op=name, args=args):
                        self.assertEqual((resp["error"]["kind"], resp["error"].get("step")),
                                         ("invalid_scenario", 2), resp)
            missing = run(w, scenario([{"bury": {"cards": ["#c1", "#c9"]}}], singles("a")))
            self.assertEqual(missing["error"], {"kind": "unknown_ref", "ref": "#c9", "step": 0})
