# SPDX-License-Identifier: AGPL-3.0-or-later
"""The queue, deck_counts and congrats kinds and set_current_deck
(specs/conformance/queue-ops.md Q1-Q4) through real workers. Expected values
come from the behavior specs (specs/sched/queue-building.md B8, B9,
orders.md O5, counts.md C2, C3, C7), never from the driver."""

from __future__ import annotations

import unittest

from driver.harness import WorkerProcess
from driver.tests.test_worker import epoch, run_ok

START = "2030-01-15T10:00:00"
NOW = epoch(START, "UTC")
CARD = {"name": "Card", "fields": ["Front", "Back"], "templates": [{"name": "Forward", "front": "{{Front}}", "back": "{{Back}}"}]}


def scenario(decks, notes, steps):
    """notes: one deck name per note, a card each, #cN in that order."""
    return {
        "id": "t/queue",
        "clock": {"tz": "UTC", "start": START},
        "setup": {
            "decks": decks,
            "notetypes": [CARD],
            "notes": [{"notetype": "Card", "deck": deck, "fields": [f"c{i}", "a"]} for i, deck in enumerate(notes, 1)],
        },
        "steps": steps,
    }


def card_states(rows):
    """One db_exec setting (type, queue, due, ivl, left) by logical card number."""
    values = ", ".join(f"({n}, {t}, {q}, {due}, {ivl}, {left})" for n, t, q, due, ivl, left in rows)
    return {"db_exec": {"sql": (
        "UPDATE cards SET type = v.column2, queue = v.column3, due = v.column4, ivl = v.column5, left = v.column6 "
        "FROM (SELECT id AS cid, row_number() OVER (ORDER BY id) AS n FROM cards) AS c "
        f"JOIN (VALUES {values}) AS v ON v.column1 = c.n WHERE cards.id = c.cid"
    )}}


class Queue(unittest.TestCase):
    def test_presentation_order_kinds_and_counts(self):
        steps = [
            # #c1 new; #c2 review due today; #c3 learning due 5 min ago; #c4
            # learning due in 10 min (inside the 20 min learn-ahead limit);
            # #c5 learning due in 2 h (held for later: neither listed nor counted).
            card_states([
                (2, 2, 2, 0, 3, 0),
                (3, 1, 1, NOW - 300, 0, 1),
                (4, 1, 1, NOW + 600, 0, 1),
                (5, 1, 1, NOW + 7200, 0, 1),
            ]),
            # A card no logical id names: a review due yesterday on #c1's note.
            {"db_exec": {"sql": (
                "INSERT INTO cards (id, nid, did, ord, mod, usn, type, queue, due, ivl, factor, reps, lapses, "
                "left, odue, odid, flags, data) SELECT 42, nid, did, 1, mod, usn, 2, 2, -1, 1, 2500, 1, 0, 0, 0, "
                "0, 0, '' FROM cards ORDER BY id LIMIT 1"
            )}},
            {"set_current_deck": {"deck": "Lang"}},
            {"observe": ["queue"]},
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario(["Lang"], ["Lang"] * 5, steps))
        # B8: learning due now, the main list (reviews 42 then #c2 by due; the
        # new card spread into them by O5: a 2, b 1 gives r n r), then the
        # learn-ahead part. B9: learning counts #c3 and #c4.
        self.assertEqual(out[3], {"queue": {
            "cards": ["#c3", "42", "#c1", "#c2", "#c4"],
            "kinds": ["learning", "review", "new", "review", "learning"],
            "counts": {"new": 1, "learning": 2, "review": 2},
        }})

    def test_an_empty_deck_has_an_empty_queue(self):
        steps = [{"set_current_deck": {"deck": "Empty"}}, {"observe": ["queue"]}]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario(["Lang", "Empty"], ["Lang"], steps))
        self.assertEqual(out[1], {"queue": {"cards": [], "kinds": [], "counts": {"new": 0, "learning": 0, "review": 0}}})

    def test_the_three_kinds_take_no_arguments(self):
        with WorkerProcess("UTC") as w:
            resps = [
                w.request({"cmd": "run", "scenario": scenario(["A"], [], [{"observe": [kind]}])})
                for kind in ("queue(#c1)", "deck_counts(x)", "congrats(y)")
            ]
        for resp in resps:
            self.assertEqual((resp["error"]["kind"], resp["error"].get("step")), ("invalid_scenario", 0), resp)


class DeckCounts(unittest.TestCase):
    def test_nested_nodes_with_children_counted_in_the_parent(self):
        steps = [card_states([(4, 1, 1, NOW - 60, 0, 1)]), {"observe": ["deck_counts"]}]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario(["A", "A::B", "C"], ["A", "A::B", "A::B", "A::B"], steps))
        # C2: the empty Default deck is hidden, siblings in name order, each
        # node named by its last component; C3: a parent's numbers include its
        # children's.
        leaf = {"name": "B", "new": 2, "learning": 1, "review": 0, "children": []}
        self.assertEqual(out[1], {"deck_counts": [
            {"name": "A", "new": 3, "learning": 1, "review": 0, "children": [leaf]},
            {"name": "C", "new": 0, "learning": 0, "review": 0, "children": []},
        ]})


class Congrats(unittest.TestCase):
    def test_remaining_cards_buried_kinds_and_the_next_learning_card(self):
        steps = [
            # #c1 new, #c2 learning due in 30 min, #c3 buried by a sibling,
            # #c4 buried by the user.
            card_states([(2, 1, 1, NOW + 1800, 0, 1), (3, 2, -2, 0, 3, 0), (4, 2, -3, 0, 3, 0)]),
            {"set_current_deck": {"deck": "Lang"}},
            {"observe": ["congrats"]},
            {"set_current_deck": {"deck": "Empty"}},
            {"observe": ["congrats"]},
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario(["Lang", "Empty"], ["Lang"] * 4, steps))
        # C7: ready 1,800 - 1,200 s from now; an empty deck: 0 and 86,400.
        self.assertEqual(out[2], {"congrats": {
            "learn_remaining": 1, "secs_until_next_learn": 600, "review_remaining": False,
            "new_remaining": True, "have_sched_buried": True, "have_user_buried": True,
        }})
        self.assertEqual(out[4], {"congrats": {
            "learn_remaining": 0, "secs_until_next_learn": 86400, "review_remaining": False,
            "new_remaining": False, "have_sched_buried": False, "have_user_buried": False,
        }})


class SetCurrentDeck(unittest.TestCase):
    def test_writes_cur_deck_by_stored_name(self):
        same = {"db_exec": {"sql": (
            "SELECT CAST(val AS TEXT) = (SELECT CAST(id AS TEXT) FROM decks WHERE name = 'A' || char(31) || 'B') "
            "FROM config WHERE key = 'curDeck'"
        )}}
        steps = [{"set_current_deck": {"deck": "A::B"}}, same]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario(["A::B"], [], steps))
        self.assertEqual(out, [{}, {"db_exec": {"rows": [[1]]}}])

    def test_unknown_and_malformed_decks_are_errors(self):
        with WorkerProcess("UTC") as w:
            unknown = w.request({"cmd": "run", "scenario": scenario(["A"], [], [{"set_current_deck": {"deck": "a"}}])})
            bad = [
                w.request({"cmd": "run", "scenario": scenario(["A"], [], [{"set_current_deck": args}])})
                for args in ("A", {"deck": "A", "x": 1}, {"deck": 1})
            ]
        self.assertEqual(unknown["error"], {"kind": "unknown_ref", "ref": "a", "step": 0})
        for resp in bad:
            self.assertEqual((resp["error"]["kind"], resp["error"].get("step")), ("invalid_scenario", 0), resp)
