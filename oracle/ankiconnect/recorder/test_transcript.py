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
        with self.assertRaises(RunError):
            self.tokens.real("#n2")


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


if __name__ == "__main__":
    unittest.main()
