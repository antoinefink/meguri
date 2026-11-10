# SPDX-License-Identifier: AGPL-3.0-or-later
"""Unit tests of the recorder's pure part (no Anki): entrypoint.sh test."""

from __future__ import annotations

import unittest

from recorder import transcript as tx
from recorder.transcript import RunError

MS = 1894701600000


def snapshot(notes=(), cards=(), decks=((1, "Default"),), notetypes=()) -> dict:
    return {"notes": [list(n) for n in notes], "cards": [list(c) for c in cards],
            "decks": [list(d) for d in decks], "notetypes": [list(m) for m in notetypes]}


def env_with(tokens: tx.Tokens) -> tx.Env:
    env = tx.Env(tokens, MS // 1000)
    env.media_dir = "/scratch/anki/oracle/collection.media"
    env.profile = "oracle"
    env.tts_needle = "[TTS-NEEDLE]"
    return env


def base_tokens() -> tx.Tokens:
    t = tx.Tokens()
    t.start(snapshot(notes=[(MS, "g1"), (MS + 1, "g2")], cards=[(MS, MS, 0), (MS + 1, MS + 1, 0)],
                     decks=[(1, "Default"), (MS, "Lang")], notetypes=[(MS + 1, "B"), (MS, "A")]),
            [MS, MS + 1], [MS, MS + 1])
    return t


def scenario(**over) -> dict:
    s = {"id": "servers/ankiconnect/harness/x", "server": "ankiconnect", "source": "oracle",
         "clock": {"tz": "Europe/Paris", "start": "2030-01-15T10:00:00"},
         "steps": [{"http": {"body": {"json": {"action": "version", "version": 6}}}}]}
    s.update(over)
    return s


class ScenarioChecks(unittest.TestCase):
    def test_minimal(self):
        tx.check_scenario(scenario(), "servers/ankiconnect/harness/x")

    def test_refusals(self):
        cases = [
            scenario(source="declared"),
            scenario(server="migaku"),
            scenario(server_config={"read_only": True}),
            scenario(steps=[{"advance": "1m"}]),
            scenario(steps=[{"http": {"connection": "c1"}}]),
            scenario(steps=[{"http": {"http_version": "1.0"}}]),
            scenario(steps=[{"http": {"headers": [["Transfer-Encoding", "chunked"]]}}]),
            scenario(steps=[{"http": {}, "expect": {"status": 200}}]),
            scenario(steps=[{"http": {}}, {"permission": {"answer": "none"}}]),
            scenario(steps=[{"http": {}}, {"wait": "1s"}]),
        ]
        for s in cases:
            with self.subTest(s=s["steps"] if s.get("source") == "oracle" else s), self.assertRaises(RunError) as cm:
                tx.check_scenario(s, None)
            self.assertEqual(cm.exception.kind, "invalid_scenario")

    def test_id_must_match_path(self):
        with self.assertRaises(RunError):
            tx.check_scenario(scenario(), "servers/ankiconnect/harness/y")

    def test_addon_config(self):
        config = tx.addon_config(scenario(server_config={"api_key": "k1"}), {"apiKey": None, "webBindPort": 1})
        self.assertEqual(config["apiKey"], "k1")
        self.assertEqual(config["webCorsOriginList"], ["http://localhost"])
        self.assertEqual(config["webBindPort"], 8765)


class Requests(unittest.TestCase):
    def test_bytes(self):
        env = env_with(base_tokens())
        http = {"headers": [["Origin", "chrome-extension://abc"]],
                "body": {"json": {"action": "notesInfo", "version": 6, "params": {"notes": ["#n2"]}}}}
        body = ('{"action":"notesInfo","version":6,"params":{"notes":[%d]}}' % (MS + 1)).encode()
        self.assertEqual(
            tx.request_bytes(http, env, 0),
            b"POST / HTTP/1.1\r\nHost: 127.0.0.1:8765\r\nOrigin: chrome-extension://abc\r\n"
            + b"Content-Length: %d\r\n\r\n" % len(body) + body)

    def test_no_body_no_length_and_given_host(self):
        env = env_with(base_tokens())
        self.assertEqual(tx.request_bytes({}, env, 0), b"POST / HTTP/1.1\r\nHost: 127.0.0.1:8765\r\n\r\n")
        self.assertEqual(tx.request_bytes({"method": "GET", "headers": [["host", "x:${port}"]]}, env, 0),
                         b"GET / HTTP/1.1\r\nhost: x:8765\r\n\r\n")
        self.assertEqual(tx.request_bytes({"no_host": True}, env, 0), b"POST / HTTP/1.1\r\n\r\n")

    def test_substitution(self):
        env = env_with(base_tokens())
        self.assertEqual(tx.substitute("nid:${#n1},${#n2} $${x}", env, 0), f"nid:{MS},{MS + 1} ${{x}}")
        for bad in ("${nope}", "${#n9}", "${"):
            with self.subTest(bad=bad), self.assertRaises(RunError):
                tx.substitute(bad, env, 0)
        with self.assertRaises(RunError) as cm:
            tx.substitute_json({"notes": ["#n9"]}, env, 3)
        self.assertEqual((cm.exception.kind, cm.exception.fields["step"]), ("unknown_ref", 3))

    def test_compact_json(self):
        self.assertEqual(tx.compact_json({"a": "é\n\x01\"\\ ", "b": [1, 1.5]}),
                         '{"a":"é\\n\\u0001\\"\\\\ ","b":[1,1.5]}'.encode())

    def test_fill(self):
        env = env_with(base_tokens())
        self.assertEqual(tx.body_bytes({"fill": {"size": 5, "byte": 65, "prefix": "x"}}, env, 0), b"xAAAA")


class Replies(unittest.TestCase):
    def test_parse(self):
        head, body = tx.split_head(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n"
                                   b"Access-Control-Allow-Origin: http://localhost\r\nX-Other: 1\r\n"
                                   b"Content-Length: 2\r\n\r\n{}")
        status, headers = tx.parse_head(head)
        self.assertEqual(tx.content_length(headers), 2)
        self.assertEqual(tx.reply_value(status, headers, body, True), {
            "status": 200, "closed": True, "body": {"json": {}},
            "headers": {"content-type": "application/json", "access-control-allow-origin": "http://localhost"}})

    def test_bodies(self):
        self.assertEqual(tx.body_value(b""), {"empty": True})
        self.assertEqual(tx.body_value(b" 12345678901234567890 "), {"json": 12345678901234567890})
        self.assertEqual(tx.body_value(b"NaN"), {"text": "NaN"})
        self.assertEqual(tx.body_value(b"\xff"), {"base64": "/w=="})


class Normalization(unittest.TestCase):
    def setUp(self):
        self.tokens = base_tokens()
        self.norm = tx.Normalizer(env_with(self.tokens))

    def reply(self, value, request):
        out, warnings = self.norm.reply({"status": 200, "headers": {}, "body": {"json": value}, "closed": True}, request)
        return out["body"]["json"], warnings

    def test_tokens_start(self):
        self.assertEqual(self.tokens.token("n", MS + 1), "#n2")
        self.assertEqual(self.tokens.token("d", MS), "#d1")
        self.assertIsNone(self.tokens.token("d", 1))
        self.assertEqual(self.tokens.token("m", MS + 1), "#m2")

    def test_positions_and_mod(self):
        value, warnings = self.reply(
            {"result": [{"noteId": MS + 1, "cards": [MS + 1], "mod": MS // 1000 + 5, "profile": "oracle",
                         "other": MS}], "error": None},
            {"action": "notesInfo", "version": 6})
        self.assertEqual(value["result"][0], {"noteId": "#n2", "cards": ["#c2"], "mod": "@5", "profile": "<profile>",
                                              "other": MS})
        self.assertEqual(warnings, ["/body/json/result/0/other"])

    def test_bare_and_unknown(self):
        value, _ = self.reply([MS, 1000000000001, 7], {"action": "findNotes", "version": 2})
        self.assertEqual(value, ["#n1", "#unknown", 7])
        value, _ = self.reply(MS + 1, {"action": "addNote"})
        self.assertEqual(value, "#n2")

    def test_error_strings(self):
        value, _ = self.reply({"result": None, "error": f"Note was not found: {MS + 1}"}, {"action": "x", "version": 6})
        self.assertEqual(value["error"], "Note was not found: #n2")
        value, _ = self.reply({"result": None, "error": f"other {MS} {MS + 1}"}, {"action": "x", "version": 2})
        self.assertEqual(value["error"], f"other {MS} {MS + 1}")  # MS is a note, card, deck and note type

    def test_multi(self):
        request = {"action": "multi", "version": 6, "params": {"actions": [
            {"action": "findNotes", "params": {}}, {"action": "findCards", "version": 6}]}}
        value, _ = self.reply({"result": [[MS], {"result": [MS + 1], "error": None}], "error": None}, request)
        self.assertEqual(value["result"], [["#n1"], {"result": ["#c2"], "error": None}])

    def test_deck_stats_and_models(self):
        value, _ = self.reply({"result": {str(MS): {"deck_id": MS}}, "error": None}, {"action": "getDeckStats", "version": 6})
        self.assertEqual(value["result"], {"#d1": {"deck_id": "#d1"}})
        value, _ = self.reply({"result": [{"id": MS, "did": None, "tmpls": [{"id": 123}], "flds": [{"id": None}]}],
                               "error": None}, {"action": "findModelsByName", "version": 6})
        self.assertEqual(value["result"], [{"id": "#m1", "did": None, "tmpls": [{"id": "#rand"}], "flds": [{"id": None}]}])

    def test_environment(self):
        value, _ = self.reply({"result": "/scratch/anki/oracle/collection.media", "error": None},
                              {"action": "getMediaDirPath", "version": 6})
        self.assertEqual(value["result"], "<media-dir>")
        value, _ = self.reply([{"question": "a[TTS-NEEDLE]", "nextReviews": ["⁨<1m⁩"]}],
                              {"action": "cardsInfo", "version": 4})
        self.assertEqual(value, [{"question": "a[tts: lang not set]", "nextReviews": ["<1m"]}])

    def test_identity_change_and_reuse(self):
        self.tokens.update(snapshot(notes=[(MS, "g1"), (MS + 1, "g9")], cards=[(MS, MS, 0)],
                                    decks=[(1, "Default"), (MS, "Lang")], notetypes=[(MS, "A"), (MS + 1, "B")]))
        self.assertEqual(self.tokens.token("n", MS + 1), "#n3")
        self.assertIsNone(self.tokens.token("c", MS + 1))
        with self.assertRaises(RunError) as cm:  # #n2's last id is #n3's now
            self.tokens.real("#n2")
        self.assertEqual((cm.exception.kind, cm.exception.fields["ref"]), ("reused_ref", "#n2"))
        self.assertEqual(self.tokens.real("#c2"), MS + 1)  # retired, its id free among cards

    def test_retired_token_keeps_last_id(self):
        # T17: addNote #n1, deleteNotes [#n1], then notesInfo [#n1] sends the deleted id.
        self.tokens.update(snapshot(notes=[(MS, "g1")], cards=[(MS, MS, 0)],
                                    decks=[(1, "Default"), (MS, "Lang")], notetypes=[(MS + 1, "B"), (MS, "A")]))
        self.assertIsNone(self.tokens.token("n", MS + 1))
        self.assertEqual(self.tokens.real("#n2"), MS + 1)
        env = env_with(self.tokens)
        self.assertEqual(tx.substitute_json({"notes": ["#n2"]}, env, 0), {"notes": [MS + 1]})
        self.assertEqual(tx.substitute("nid:${#c2}", env, 0), f"nid:{MS + 1}")
        with self.assertRaises(RunError) as cm:
            self.tokens.real("#n3")
        self.assertEqual(cm.exception.kind, "unknown_ref")
        # The next note takes the freed id: the retired token is reused_ref now.
        self.tokens.update(snapshot(notes=[(MS, "g1"), (MS + 1, "g3")], cards=[(MS, MS, 0), (MS + 1, MS + 1, 0)],
                                    decks=[(1, "Default"), (MS, "Lang")], notetypes=[(MS + 1, "B"), (MS, "A")]))
        self.assertEqual(self.tokens.token("n", MS + 1), "#n3")
        self.assertEqual(self.tokens.token("c", MS + 1), "#c3")
        with self.assertRaises(RunError) as cm:
            tx.substitute_json({"notes": ["#n2"]}, env, 4)
        self.assertEqual((cm.exception.kind, cm.exception.fields["step"]), ("reused_ref", 4))

    def test_retired_last_wins(self):
        live = dict(decks=[(1, "Default"), (MS, "Lang")], notetypes=[(MS + 1, "B"), (MS, "A")])
        self.tokens.update(snapshot(notes=[(MS, "g1")], cards=[(MS, MS, 0)], **live))            # #n2 gone
        self.tokens.update(snapshot(notes=[(MS, "g1"), (MS + 1, "g3")], cards=[(MS, MS, 0)], **live))  # #n3 there
        self.tokens.update(snapshot(notes=[(MS, "g1")], cards=[(MS, MS, 0)], **live))            # #n3 gone
        self.assertEqual(self.tokens.retired_token("n", MS + 1), "#n3")
        self.assertEqual(self.tokens.real("#n2"), MS + 1)

    def test_card_compared_by_note_token(self):
        # deleteNotes then addNote inside one multi: the new note has #n1's id,
        # its card #c1's id and ord. Both are new entities.
        self.tokens.update(snapshot(notes=[(MS, "g7"), (MS + 1, "g2")], cards=[(MS, MS, 0), (MS + 1, MS + 1, 0)],
                                    decks=[(1, "Default"), (MS, "Lang")], notetypes=[(MS + 1, "B"), (MS, "A")]))
        self.assertEqual(self.tokens.token("n", MS), "#n3")
        self.assertEqual(self.tokens.token("c", MS), "#c3")
        self.assertEqual(self.tokens.token("c", MS + 1), "#c2")
        # A card that moves to another live note is new too; an unchanged one keeps its token.
        self.tokens.update(snapshot(notes=[(MS, "g7"), (MS + 1, "g2")], cards=[(MS, MS + 1, 0), (MS + 1, MS + 1, 0)],
                                    decks=[(1, "Default"), (MS, "Lang")], notetypes=[(MS + 1, "B"), (MS, "A")]))
        self.assertEqual(self.tokens.token("c", MS), "#c4")
        self.assertEqual(self.tokens.token("c", MS + 1), "#c2")

    def test_error_strings_retired(self):
        self.tokens.update(snapshot(notes=[(MS, "g1")], cards=[(MS, MS, 0)],
                                    decks=[(1, "Default"), (MS, "Lang")], notetypes=[(MS + 1, "B"), (MS, "A")]))
        value, _ = self.reply({"result": None, "error": f"Note was not found: {MS + 1}"}, {"action": "x", "version": 6})
        self.assertEqual(value["error"], "Note was not found: #n2")
        # The kind comes from the message's start; an id no entity ever had stays.
        value, _ = self.reply({"result": None, "error": f"note {MS} {MS + 1} 1000000000001"}, {"action": "x", "version": 6})
        self.assertEqual(value["error"], "note #n1 #n2 1000000000001")
        # Any other message: MS + 1 is a live note type (#m2) and a retired note and card.
        value, _ = self.reply({"result": None, "error": f"x {MS + 1}"}, {"action": "x", "version": 6})
        self.assertEqual(value["error"], "x #m2")
        # A live entity wins over a retired one with the same id.
        self.tokens.update(snapshot(notes=[(MS, "g1"), (MS + 1, "g3")], decks=[(1, "Default"), (MS, "Lang")],
                                    notetypes=[(MS + 1, "B"), (MS, "A")]))
        value, _ = self.reply({"result": None, "error": f"Note was not found: {MS + 1}"}, {"action": "x", "version": 6})
        self.assertEqual(value["error"], "Note was not found: #n3")
        value, _ = self.reply({"result": None, "error": f"x {MS + 2}"}, {"action": "x", "version": 6})
        self.assertEqual(value["error"], f"x {MS + 2}")

    def test_error_strings_retired_any_kind(self):
        t = tx.Tokens()
        t.start(snapshot(notes=[(MS + 7, "g")], cards=[(MS + 8, MS + 7, 0)]), [MS + 7], [MS + 8])
        t.update(snapshot())
        norm = tx.Normalizer(env_with(t))
        out, _ = norm.reply({"status": 200, "headers": {}, "body": {"json": {"result": None, "error": f"x {MS + 7} {MS + 8}"}},
                             "closed": True}, {"action": "x", "version": 6})
        self.assertEqual(out["body"]["json"]["error"], "x #n1 #c1")


class Observations(unittest.TestCase):
    def setUp(self):
        self.tokens = base_tokens()
        self.norm = tx.Normalizer(env_with(self.tokens))

    def test_db_dump_ids(self):
        # #n2 and #c2 deleted, then a note added at #n2's id with no token (as
        # inside one addNotes): its key stays decimal, its id column #unknown.
        self.tokens.update(snapshot(notes=[(MS, "g1")], cards=[(MS, MS, 0)],
                                    decks=[(1, "Default"), (MS, "Lang")], notetypes=[(MS + 1, "B"), (MS, "A")]))
        dump = {
            "notes": {str(MS): {"id": MS, "mid": MS + 1, "mod": MS // 1000, "flds": f"{MS}"},
                      str(MS + 5): {"id": MS + 5, "mid": MS, "mod": 0}},
            "cards": {str(MS): {"id": MS, "nid": MS, "did": MS, "odid": 0, "due": 1},
                      "7": {"id": 7, "nid": MS, "did": 1, "odid": MS + 9}},
            "revlog": {str(MS): {"id": MS, "cid": MS}},
            "graves": {f"{MS + 1},0": {"oid": MS + 1, "type": 0, "usn": -1},
                       f"{MS + 1},1": {"oid": MS + 1, "type": 1, "usn": -1},
                       f"{MS + 3},1": {"oid": MS + 3, "type": 1, "usn": -1},
                       f"{MS},2": {"oid": MS, "type": 2, "usn": -1}},
            "decks": {"1": {"id": 1, "name": "Default"}, str(MS): {"id": MS, "name": "Lang"}},
            "fields": {f"{MS},0": {"ntid": MS, "ord": 0}},
            "tags": {"t1": {"tag": "t1"}},
        }
        out = self.norm.observation({"db_dump": dump})["db_dump"]
        self.assertEqual(out["notes"], {
            "#n1": {"id": "#n1", "mid": "#m2", "mod": MS // 1000, "flds": f"{MS}"},
            str(MS + 5): {"id": "#unknown", "mid": "#m1", "mod": 0}})
        self.assertEqual(out["cards"], {
            "#c1": {"id": "#c1", "nid": "#n1", "did": "#d1", "odid": 0, "due": 1},
            "7": {"id": 7, "nid": "#n1", "did": 1, "odid": "#unknown"}})
        self.assertEqual(out["revlog"], {str(MS): {"id": MS, "cid": "#c1"}})
        self.assertEqual(out["graves"], {
            "#c2,0": {"oid": "#c2", "type": 0, "usn": -1},
            "#n2,1": {"oid": "#n2", "type": 1, "usn": -1},
            f"{MS + 3},1": {"oid": "#unknown", "type": 1, "usn": -1},
            "#d1,2": {"oid": "#d1", "type": 2, "usn": -1}})  # a live deck when no retired one has the id
        self.assertEqual(out["decks"], {"1": {"id": 1, "name": "Default"}, "#d1": {"id": "#d1", "name": "Lang"}})
        self.assertEqual(out["fields"], {"#m1,0": {"ntid": "#m1", "ord": 0}})
        self.assertEqual(out["tags"], {"t1": {"tag": "t1"}})

    def test_graves_prefer_retired(self):
        # #n2 deleted, #n3 at its id deleted too: the grave names the one retired last.
        live = dict(decks=[(1, "Default"), (MS, "Lang")], notetypes=[(MS + 1, "B"), (MS, "A")])
        self.tokens.update(snapshot(notes=[(MS, "g1")], cards=[(MS, MS, 0)], **live))
        self.tokens.update(snapshot(notes=[(MS, "g1"), (MS + 1, "g3")], cards=[(MS, MS, 0)], **live))
        grave = {f"{MS + 1},1": {"oid": MS + 1, "type": 1, "usn": -1}}
        out = self.norm.observation({"db_dump": {"graves": grave}})["db_dump"]["graves"]
        self.assertEqual(out, {"#n2,1": {"oid": "#n2", "type": 1, "usn": -1}})  # retired before the live #n3
        self.tokens.update(snapshot(notes=[(MS, "g1")], cards=[(MS, MS, 0)], **live))
        out = self.norm.observation({"db_dump": {"graves": grave}})["db_dump"]["graves"]
        self.assertEqual(out, {"#n3,1": {"oid": "#n3", "type": 1, "usn": -1}})

    def test_decks_kind_and_strings(self):
        out = self.norm.observation({"decks": {"Lang": {"id": MS, "name": "Lang"}, "Default": {"id": 1}},
                                     "media_dir": {"a.mp3": {"size": 4}},
                                     "card": {"x": "/scratch/anki/oracle/collection.media"}})
        self.assertEqual(out, {"decks": {"Lang": {"id": "#d1", "name": "Lang"}, "Default": {"id": 1}},
                               "media_dir": {"a.mp3": {"size": 4}}, "card": {"x": "<media-dir>"}})


class Build(unittest.TestCase):
    def test_recorder_sha256(self):
        import hashlib
        import tempfile
        from pathlib import Path

        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            files = {"driver/a.py": b"a", "driver/ops/b.py": b"b",
                     "home/recorder/transcript.py": b"t", "home/recorder/test_transcript.py": b"x",
                     "home/recorder/__pycache__/t.pyc": b"x", "home/addons21/meguri_recorder/__init__.py": b"i",
                     "home/addons21/meguri_recorder/test_x.py": b"y", "home/launch.py": b"l",
                     "home/entrypoint.sh": b"e", "home/ADDON_PIN": b"x", "home/requirements.lock": b"x"}
            for rel, data in files.items():
                (root / rel).parent.mkdir(parents=True, exist_ok=True)
                (root / rel).write_bytes(data)
            driver = [root / "driver/a.py", root / "driver/ops/b.py"]
            got = tx.recorder_sha256(root / "driver", driver, root / "home")

            def line(label, data):
                return label.encode() + b"\0" + hashlib.sha256(data).hexdigest().encode() + b"\n"

            want = hashlib.sha256(b"".join([
                line("driver/a.py", b"a"), line("driver/ops/b.py", b"b"),
                line("ankiconnect/addons21/meguri_recorder/__init__.py", b"i"),
                line("ankiconnect/addons21/meguri_recorder/test_x.py", b"y"),
                line("ankiconnect/entrypoint.sh", b"e"), line("ankiconnect/launch.py", b"l"),
                line("ankiconnect/recorder/transcript.py", b"t")])).hexdigest()
            self.assertEqual(got, want)


class Clock(unittest.TestCase):
    def test_escaped_ids_and_mods(self):
        start = MS // 1000
        check = tx.ClockCheck(start)
        check.check(snapshot(notes=[(MS, "g", start)], cards=[(MS, MS, 0, start)], notetypes=[(MS + 5, "A")]), start, None)
        check.check(snapshot(notes=[(MS, "g", start + 60), (MS + 60000, "h", start + 60)]), start + 60, 1)
        for snap in (snapshot(cards=[(1794319906986, MS, 0, start)]),        # a real-time id
                     snapshot(notes=[(MS, "g", 1794319906)])):                # a real-time mod
            with self.subTest(snap=snap), self.assertRaises(RunError) as cm:
                tx.ClockCheck(start).check(snap, start, 2)
            self.assertEqual(cm.exception.kind, "clock_escaped")


class GuiSettled(unittest.TestCase):
    """The wait before each exchange (T16): the Browse window's deferred
    search (C9.12) must have run before guiSelectedNotes reads its selection."""

    def fake_clock(self):
        now = [0.0]
        return now, (lambda secs: now.__setitem__(0, now[0] + secs)), (lambda: now[0])

    def test_no_window_is_one_read(self):
        reads = []
        now, sleep, mono = self.fake_clock()
        self.assertEqual(tx.wait_gui_settled(lambda: reads.append(1) or {"settled": True, "window": None}, 2,
                                             sleep=sleep, monotonic=mono), 1)
        self.assertEqual(now[0], 0.0)

    def test_waits_for_the_deferred_search(self):
        states = iter([{"settled": False, "window": "browse"}] * 3 + [{"settled": True, "window": "browse"}])
        now, sleep, mono = self.fake_clock()
        self.assertEqual(tx.wait_gui_settled(lambda: next(states), 2, sleep=sleep, monotonic=mono), 4)
        self.assertAlmostEqual(now[0], 3 * tx.GUI_POLL_SECS)

    def test_bounded(self):
        now, sleep, mono = self.fake_clock()
        with self.assertRaises(RunError) as cm:
            tx.wait_gui_settled(lambda: {"settled": False, "window": "browse"}, 2, limit=1.0,
                                sleep=sleep, monotonic=mono)
        self.assertEqual(cm.exception.kind, "timeout")
        self.assertEqual(cm.exception.to_json()["step"], 2)
        self.assertLessEqual(now[0], 1.0 + 2 * tx.GUI_POLL_SECS)

    def test_only_true_settles(self):
        now, sleep, mono = self.fake_clock()
        with self.assertRaises(RunError):
            tx.wait_gui_settled(lambda: {"settled": 1}, 0, limit=0.2, sleep=sleep, monotonic=mono)


if __name__ == "__main__":
    unittest.main()
