# SPDX-License-Identifier: AGPL-3.0-or-later
"""Actor scenarios end to end (specs/conformance/sync-ops.md): real workers,
the real sync server, real libfaketime. Expected results come from the spec
(outcomes, rejection kinds, step indices, pointer order), never from the
driver.

Tests that need a fault proxy run only when ORACLE_SYNC_FAULTS names a
toxiproxy API on this network (tools/mg starts sync-faults for oracle test);
the others need nothing but the oracle image.
"""

from __future__ import annotations

import http.client
import json
import os
import socket
import threading
import unittest
import urllib.parse
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

from driver.harness import WorkerProcess
from driver.tests.test_worker import epoch

FAULTS = os.environ.get("ORACLE_SYNC_FAULTS")
needs_faults = unittest.skipUnless(FAULTS, "ORACLE_SYNC_FAULTS is not set (tools/mg starts sync-faults)")
START = "2030-01-15T10:00:00"
SETUP = {
    "decks": ["Words"],
    "notetypes": [{"name": "Pair", "fields": ["Front", "Back"],
                   "templates": [{"name": "Forward", "front": "{{Front}}", "back": "{{Back}}"}]}],
    "notes": [{"notetype": "Pair", "deck": "Words", "fields": ["sun", "sol"]}],
}
UPLOAD = [{"advance": "1m"}, {"sync": {"kind": "login"}}, {"sync": {"kind": "normal"}}, {"sync": {"kind": "full_upload"}}]
OK = {"sync": {"result": "ok"}}
AUTH_FAILED = {"sync": {"error": "auth_failed"}}
STATUS = {"sync": {"kind": "status"}}
MOON = {"add_note": {"notetype": "Pair", "deck": "Words", "fields": ["moon", "luna"]}}


def result(value):
    return {"sync": {"result": value}}


def actor_scenario(sid, actors, steps, server=None):
    sc = {"id": sid, "clock": {"tz": "UTC", "start": START}, "actors": actors, "setup": SETUP, "steps": steps}
    if server is not None:
        sc["server"] = server
    return sc


def two_party(sid="t/two"):
    return actor_scenario(sid, ["client_a", "client_c"], UPLOAD + [
        {"advance": "1m"},
        {"actor": "client_c"},
        {"observe": ["card(#c1)"]},
        {"sync": {"kind": "login"}},
        {"sync": {"kind": "normal"}},
        {"sync": {"kind": "full_download"}},
        {"sync": {"kind": "normal"}},
        {"observe": ["card(#c1)", "ids"]},
        {"converge": {}},
    ])


def run(test, worker, sc):
    resp = worker.request({"cmd": "run", "scenario": sc})
    test.assertTrue(resp["ok"], resp)
    return resp["observations"]["steps"]


def sync_servers():
    """Command lines of the sync servers running in this container."""
    found = []
    for pid in filter(str.isdigit, os.listdir("/proc")):
        try:
            with open(f"/proc/{pid}/cmdline", "rb") as f:
                cmd = f.read().split(b"\0")
        except OSError:
            continue
        if b"anki.syncserver" in cmd:
            found.append(pid)
    return found


def environ_of(pid):
    with open(f"/proc/{pid}/environ", "rb") as f:
        return dict(e.decode().split("=", 1) for e in f.read().split(b"\0") if b"=" in e)


class ForwardProxy:
    """A stand-in for the recorder (sync-ops.md S9): an HTTP forward proxy on
    this container's own address (not loopback: no_proxy covers that) that
    strips /<actor>/<step> from each request, remembers it in `seen` and
    forwards it to the server. A request of an (actor, step) in `refuse` gets
    the server's answer to credentials or a host key it does not know instead:
    403 with the body 403 (specs/sync/transport.md T14)."""

    def __init__(self, refuse=()):
        self.seen: list[tuple[str, int, str]] = []
        self.refuse = set(refuse)
        lock = threading.Lock()
        proxy = self

        class Forward(BaseHTTPRequestHandler):
            def do_POST(self):  # noqa: N802 - http.server's name
                url = urllib.parse.urlsplit(self.path)
                _, actor, step, rest = url.path.split("/", 3)
                with lock:
                    proxy.seen.append((actor, int(step), "/" + rest))
                if self.headers.get("Transfer-Encoding", "").lower() == "chunked":
                    body = b""
                    while size := int(self.rfile.readline().split(b";")[0], 16):
                        body += self.rfile.read(size)
                        self.rfile.readline()
                    while self.rfile.readline() not in (b"\r\n", b"\n", b""):
                        pass
                else:
                    body = self.rfile.read(int(self.headers.get("Content-Length") or 0))
                if (actor, int(step)) in proxy.refuse:
                    self.send_response(403)
                    self.send_header("Content-Type", "text/plain; charset=utf-8")
                    self.send_header("Content-Length", "3")
                    self.end_headers()
                    self.wfile.write(b"403")
                    return
                hop = ("proxy-connection", "connection", "transfer-encoding", "content-length")
                headers = {k: v for k, v in self.headers.items() if k.lower() not in hop}
                headers["Content-Length"] = str(len(body))
                conn = http.client.HTTPConnection(url.hostname, url.port, timeout=30)
                conn.request("POST", "/" + rest, body=body, headers=headers)
                resp = conn.getresponse()
                data = resp.read()
                self.send_response(resp.status)
                for k, v in resp.getheaders():
                    if k.lower() not in ("transfer-encoding", "content-length", "connection"):
                        self.send_header(k, v)
                self.send_header("Content-Length", str(len(data)))
                self.end_headers()
                self.wfile.write(data)
                conn.close()

            def log_message(self, *args):
                pass

        self.server = ThreadingHTTPServer((socket.gethostbyname(socket.gethostname()), 0), Forward)
        self.address = "{}:{}".format(*self.server.server_address[:2])

    def __enter__(self):
        threading.Thread(target=self.server.serve_forever, daemon=True).start()
        return self

    def __exit__(self, *exc):
        self.server.shutdown()
        self.server.server_close()


class TwoParty(unittest.TestCase):
    def test_an_upload_and_a_download_converge(self):
        with WorkerProcess("UTC") as w:
            out = run(self, w, two_party())
            leftovers = sorted(os.listdir(w.scratch))
        t = epoch(START, "UTC") * 1000
        # S4.1: the server's new collection has no cards, so client_a may only
        # upload; client_c's has none, so it may only download; then nothing.
        self.assertEqual(out[:6], [{}, OK, {"sync": {"result": "full_upload"}}, OK, {}, {}])
        self.assertEqual(out[6], {"card": {"#c1": None}})  # S2.4: absent on client_c
        self.assertEqual(out[7:11], [OK, {"sync": {"result": "full_download"}}, OK, {"sync": {"result": "no_changes"}}])
        self.assertEqual(out[11]["card"]["#c1"]["nid"], "#n1")
        self.assertEqual(out[11]["ids"], {"#n1": t, "#c1": t})  # sync keeps real ids
        converge = out[12]["converge"]
        self.assertEqual(converge["differences"], {"client_a": [], "client_c": []})
        self.assertEqual(sorted(converge["dump"]), sorted(
            ["col", "notes", "cards", "revlog", "graves", "decks", "deck_config", "notetypes", "fields",
             "templates", "tags", "config"]))
        self.assertEqual(sorted(converge["dump"]["col"]["1"]),  # S7.5: without mod, usn and ls
                         ["conf", "crt", "dconf", "decks", "dty", "id", "models", "scm", "tags", "ver"])
        # S7.3, S7.5: config entries without usn and mtime_secs.
        self.assertIn("activeDecks", converge["dump"]["config"])
        self.assertEqual({tuple(sorted(row)) for row in converge["dump"]["config"].values()}, {("KEY", "val")})
        self.assertEqual(converge["dump"]["notes"][str(t)]["guid"], "#random")
        self.assertEqual(converge["dump"]["notes"][str(t)]["flds"], "sun\x1fsol")
        # The server and its base directory are gone with the session.
        self.assertEqual(leftovers, ["clock"])
        self.assertEqual(sync_servers(), [])

    def test_the_server_is_a_child_without_proxy_variables_on_the_scenario_clock(self):
        env = {"http_proxy": "http://127.0.0.1:9", "HTTPS_PROXY": "http://127.0.0.1:9",
               "All_Proxy": "http://127.0.0.1:9", "NO_PROXY": "nothing"}
        sc = two_party()
        with WorkerProcess("UTC", extra_env=env) as w:
            begun = w.request({"cmd": "begin", "scenario": sc})
            servers = sync_servers()
            server_env = environ_of(servers[0]) if servers else {}
            steps = [w.request({"cmd": "step"}) for _ in sc["steps"]]
            w.request({"cmd": "end"})
            after = sync_servers()
        t = epoch(START, "UTC") * 1000
        self.assertTrue(begun["ok"], begun)
        self.assertEqual(begun["ids"], {"notes": [t], "cards": [t]})
        self.assertEqual(len(servers), 1)
        self.assertEqual([k for k in server_env if k.lower().endswith("_proxy")], [])
        self.assertEqual(server_env["SYNC_HOST"], "127.0.0.1")
        self.assertEqual(server_env["SYNC_USER1"], "{username}:{password}".format(**begun["sync"]["account"]))
        self.assertIn("libfaketime", server_env["LD_PRELOAD"])
        self.assertEqual(server_env["FAKETIME_TIMESTAMP_FILE"], os.path.join(w.scratch, "clock"))
        port = server_env["SYNC_PORT"]
        self.assertEqual(begun["sync"]["endpoints"], {
            "actors": f"http://127.0.0.1:{port}/", "converge": f"http://127.0.0.1:{port}/",
            "upstream": None, "recorder": None,
        })
        # The stray proxies above lead nowhere: a sync through them would fail.
        self.assertTrue(all(s["ok"] for s in steps), steps)
        self.assertEqual(steps[12]["observation"]["converge"]["differences"], {"client_a": [], "client_c": []})
        self.assertEqual([s["ids"] for s in steps[:3]], [{"notes": [], "cards": []}] * 3)
        self.assertEqual(after, [])


class ThreeParty(unittest.TestCase):
    def test_incremental_rounds_restamp_config_and_still_converge(self):
        config = {"db_dump": {"tables": ["config"]}}
        steps = UPLOAD + [
            {"advance": "1m"}, {"actor": "client_b"},                                          # 4, 5
            {"sync": {"kind": "login"}}, {"sync": {"kind": "normal"}}, {"sync": {"kind": "full_download"}},
            {"advance": "1m"},                                                                  # 9
            {"add_note": {"notetype": "Pair", "deck": "Words", "fields": ["moon", "luna"]}},
            {"sync": {"kind": "normal"}},                                                       # 11
            {"advance": "1m"}, {"actor": "client_c"},
            {"sync": {"kind": "login"}}, {"sync": {"kind": "normal"}}, {"sync": {"kind": "full_download"}},
            {"advance": "1m"},                                                                  # 17
            {"answer": {"card": "#c1", "rating": "good", "time_ms": 3000}},
            {"sync": {"kind": "normal"}},                                                       # 19
            {"advance": "1m"}, {"actor": "client_a"}, {"sync": {"kind": "normal"}},             # 20-22
            {"observe": ["card(#c2)", "revlog(#c1)", config]},                                  # 23
            {"advance": "1m"}, {"actor": "client_b"}, {"sync": {"kind": "normal"}},             # 24-26
            {"observe": ["card(#c2)", "revlog(#c1)", config]},                                  # 27
            {"converge": {}},                                                                   # 28
        ]
        with WorkerProcess("UTC") as w:
            out = run(self, w, actor_scenario("t/three", ["client_a", "client_b", "client_c"], steps))
        no_changes = {"sync": {"result": "no_changes"}}
        self.assertEqual([out[i] for i in (11, 19, 22, 26)], [no_changes] * 4)
        # The note from client_b and the review from client_c reached both.
        a, b = out[23], out[27]
        self.assertEqual(a["card"]["#c2"]["nid"], "#n2")
        self.assertEqual(b["card"], a["card"])
        self.assertEqual(len(a["revlog"]["#c1"]), 1)
        self.assertEqual(b["revlog"], a["revlog"])
        # S7.3's three-party example: each receiver restamps every config entry
        # with its own clock, so the stamps differ where the values agree.
        start = epoch(START, "UTC")
        stamps = lambda obs: {row["mtime_secs"] for row in obs["db_dump"]["config"].values()}  # noqa: E731
        values = lambda obs: {k: row["val"] for k, row in obs["db_dump"]["config"].items()}  # noqa: E731
        self.assertEqual((stamps(a), stamps(b)), ({start + 6 * 60}, {start + 7 * 60}))
        self.assertEqual(values(a), values(b))
        # Excluding the config stamps (and nothing else) makes all three converge.
        self.assertEqual(out[28]["converge"]["differences"], {"client_a": [], "client_b": [], "client_c": []})
        self.assertEqual({tuple(sorted(row)) for row in out[28]["converge"]["dump"]["config"].values()},
                         {("KEY", "val")})


class ServerClock(unittest.TestCase):
    def offset_run(self, offset, steps):
        sc = actor_scenario("t/clock", ["client_a"], steps, server={"clock_offset_secs": offset})
        with WorkerProcess("UTC") as w:
            out = run(self, w, sc)
            leftovers = sorted(os.listdir(w.scratch))
        self.assertEqual(leftovers, ["clock"])
        return out

    def test_a_server_more_than_300s_off_refuses_the_status_check(self):
        steps = [{"advance": "1m"}, {"observe": ["db_dump"]}, {"sync": {"kind": "login"}},
                 {"sync": {"kind": "normal"}}, {"observe": ["db_dump"]}]
        for offset in (301, -301):
            with self.subTest(offset=offset):
                out = self.offset_run(offset, steps)
                self.assertEqual(out[2:4], [OK, {"sync": {"error": "clock_incorrect"}}])
                self.assertEqual(out[4], out[1])  # S4.4: the collection does not change

    def test_status_is_clock_incorrect_only_when_it_asks_the_server(self):
        steps = [
            {"advance": "1m"},                    # 0
            {"sync": {"kind": "login"}},          # 1
            {"observe": ["db_dump"]},             # 2
            STATUS,                               # 3: never synced, so the server is asked
            {"observe": ["db_dump"]},             # 4
            {"sync": {"kind": "full_upload"}},    # 5: no status check (S4.1)
            {"advance": "1m"},                    # 6
            MOON,                                 # 7
            STATUS,                               # 8: local changes, answered offline
        ]
        for offset in (301, -301):
            with self.subTest(offset=offset):
                out = self.offset_run(offset, steps)
                self.assertEqual(out[3], {"sync": {"error": "clock_incorrect"}})
                self.assertEqual(out[4], out[2])  # S4.4: the collection does not change
                self.assertEqual(out[5], OK)
                # S4.4: status gets clock_incorrect only when it asks the server;
                # meta-status.md M6 step 1 answers local changes without asking.
                self.assertEqual(out[8], result("normal_sync"))

    def test_300s_is_within_the_limit_and_the_server_clock_follows_the_scenario(self):
        steps = [{"advance": "1m"}, {"sync": {"kind": "login"}}, {"sync": {"kind": "normal"}},
                 {"advance": "2d"}, {"sync": {"kind": "normal"}}]
        out = self.offset_run(300, steps)
        # Had the server's clock stayed put, the second check would be 2 days off.
        self.assertEqual(out[1:3], [OK, {"sync": {"result": "full_upload"}}])
        self.assertEqual(out[4], {"sync": {"result": "full_upload"}})


class Checks(unittest.TestCase):
    def test_actor_scenarios_are_checked_before_any_collection(self):
        login = {"sync": {"kind": "login"}}
        cases = {
            "handoff": (actor_scenario("t/x", ["client_a"], [{"handoff": "meguri"}]), 0),
            "undeclared actor": (actor_scenario("t/x", ["client_a"], [{"actor": "client_b"}]), 0),
            "first sync not login": (actor_scenario("t/x", ["client_a", "client_b"],
                                                    [login, {"actor": "client_b"}, {"sync": {"kind": "normal"}}]), 2),
            # S4.5: credentials only with login, and only its two values.
            "credentials with normal": (actor_scenario("t/x", ["client_a"],
                                                       [login, {"sync": {"kind": "normal", "credentials": "account"}}]), 1),
            "status before login": (actor_scenario("t/x", ["client_a"], [STATUS]), 0),
            "credentials with status": (actor_scenario("t/x", ["client_a"],
                                                       [login, {"sync": {"kind": "status", "credentials": "account"}}]), 1),
            "unknown credentials": (actor_scenario("t/x", ["client_a"],
                                                   [{"sync": {"kind": "login", "credentials": "guest"}}]), 0),
            "credentials not a string": (actor_scenario("t/x", ["client_a"],
                                                        [{"sync": {"kind": "login", "credentials": True}}]), 0),
            "credentials null": (actor_scenario("t/x", ["client_a"],
                                                [{"sync": {"kind": "login", "credentials": None}}]), 0),
            "unknown sync key": (actor_scenario("t/x", ["client_a"],
                                                [{"sync": {"kind": "login", "password": "x"}}]), 0),
            "sync without actors": ({"id": "t/x", "clock": {"tz": "UTC", "start": START}, "steps": [login]}, 0),
            "converge without actors": ({"id": "t/x", "clock": {"tz": "UTC", "start": START},
                                         "steps": [{"converge": {}}]}, 0),
            "server without actors": ({"id": "t/x", "clock": {"tz": "UTC", "start": START},
                                       "server": {"clock_offset_secs": 1}}, None),
            "offset too large": (actor_scenario("t/x", ["client_a"], [], server={"clock_offset_secs": 86401}), None),
            "offset not an integer": (actor_scenario("t/x", ["client_a"], [], server={"clock_offset_secs": 1.0}), None),
            "unknown server key": (actor_scenario("t/x", ["client_a"], [], server={"port": 1}), None),
            "duplicate actor": (actor_scenario("t/x", ["client_a", "client_a"], []), None),
            "bad actor name": (actor_scenario("t/x", ["meguri_1"], []), None),
            "no actors": (actor_scenario("t/x", [], []), None),
        }
        with WorkerProcess("UTC") as w:
            results = {case: w.request({"cmd": "run", "scenario": sc}) for case, (sc, _) in cases.items()}
            segment = w.request({"cmd": "run", "scenario": two_party(), "steps": [0, 2]})
            leftovers = sorted(os.listdir(w.scratch))
        for case, (_, step) in cases.items():
            with self.subTest(case=case):
                err = results[case].get("error", {})
                self.assertEqual((results[case]["ok"], err.get("kind"), err.get("step")), (False, "invalid_scenario", step))
        self.assertEqual(segment["error"]["kind"], "invalid_scenario")
        self.assertEqual(leftovers, ["clock"])

    def test_a_run_that_needs_a_fault_proxy_without_one_is_sync_unavailable(self):
        sc = actor_scenario("t/f", ["client_a"], [{"net_fault": {"kind": "clear"}}])
        with WorkerProcess("UTC", extra_env={"ORACLE_SYNC_FAULTS": ""}) as w:
            unset = w.request({"cmd": "run", "scenario": sc})
            caller = w.request({"cmd": "begin", "scenario": two_party(), "caller_actors": []})
            w.request({"cmd": "end"})
        with WorkerProcess("UTC", extra_env={"ORACLE_SYNC_FAULTS": "127.0.0.1:9"}) as w:
            refused = w.request({"cmd": "run", "scenario": sc})
            leftovers = sorted(os.listdir(w.scratch))
        self.assertEqual(unset["error"]["kind"], "sync_unavailable")
        self.assertTrue(caller["ok"], caller)  # no caller actors: no fault proxy needed
        self.assertEqual(refused["error"]["kind"], "sync_unavailable")
        self.assertEqual(leftovers, ["clock"])
        self.assertEqual(sync_servers(), [])


class Unit(unittest.TestCase):
    def test_pointers_list_first_disagreements_in_jcs_order(self):
        from driver.ops.sync_client import differences

        ref = {"cards": {"10": {"a": 1, "b": 2.0}, "9": {"a": 1}}, "tags": {"x/y": {"t": "x/y"}, "é": {}},
               "notes": {}}
        act = {"cards": {"10": {"a": 1, "b": 2}, "11": {"a": 1}}, "tags": {"x/y": {"t": "x/z"}, "e~": {}},
               "notes": {}, "graves": {}}
        # 1 vs 1.0 differ; a row or table on one side is reported once, at its
        # own level; member names compare as UTF-16 code units ("1" < "9").
        self.assertEqual(differences(act, ref), [
            "/cards/10/b", "/cards/11", "/cards/9", "/graves", "/tags/e~0", "/tags/x~1y/t", "/tags/é",
        ])
        self.assertEqual(differences(ref, ref), [])

    def test_random_protobuf_fields_are_removed_only_from_well_formed_blobs(self):
        from driver.observe.db_dump import without_field

        # field 1 varint 150, field 9 varint 7, field 2 bytes "ab", field 9 again, field 5 fixed32
        blob = bytes([0x08, 0x96, 0x01, 0x48, 0x07, 0x12, 0x02, 0x61, 0x62, 0x48, 0x01, 0x2D, 1, 2, 3, 4])
        self.assertEqual(without_field(blob, 9), bytes([0x08, 0x96, 0x01, 0x12, 0x02, 0x61, 0x62, 0x2D, 1, 2, 3, 4]))
        self.assertEqual(without_field(blob, 8), blob)
        for bad in (bytes([0x48]), bytes([0x12, 0x05, 0x61]), bytes([0x00, 0x01]), bytes([0x4B, 0x01]),
                    bytes([0x48, 0x07, 0x09, 1, 2])):
            self.assertEqual(without_field(bad, 9), bad, bad.hex())

    def test_db_dump_values_follow_the_storage_class(self):
        import hashlib
        import tempfile

        from anki.collection import Collection
        from driver.observe.db_dump import dump

        with tempfile.TemporaryDirectory(dir=os.environ.get("ORACLE_SCRATCH_ROOT")) as tmp:
            col = Collection(os.path.join(tmp, "collection.anki2"))
            try:
                col.db.execute("insert into tags (tag, usn, collapsed, config) values ('a/b', 3, 0, x'0102')")
                col.db.execute("insert into config (KEY, usn, mtime_secs, val) values ('r', 0, 0, 1.0)")
                col.db.execute("insert into graves (oid, type, usn) values (5, 1, -1)")
                d = dump(col, ["tags", "config", "graves"])
                raw = dump(col, raw=True)
            finally:
                col.close()
        self.assertEqual(d["tags"]["a/b"], {"tag": "a/b", "usn": 3, "collapsed": 0,
                                            "config": {"sha256": hashlib.sha256(b"\x01\x02").hexdigest()}})
        r = d["config"]["r"]["val"]
        self.assertEqual((type(r), r), (float, 1.0))
        self.assertEqual(d["graves"], {"5,1": {"oid": 5, "type": 1, "usn": -1}})
        self.assertEqual(sorted(raw), sorted(["col", "notes", "cards", "revlog", "graves", "decks", "deck_config",
                                              "notetypes", "fields", "templates", "tags", "config"]))


class Recorder(unittest.TestCase):
    def test_oracle_actors_sync_through_the_recorder_with_actor_and_step(self):
        with ForwardProxy() as proxy:
            with WorkerProcess("UTC") as plain:
                expected = run(self, plain, two_party())
            with WorkerProcess("UTC", extra_env={"ORACLE_SYNC_RECORDER": proxy.address}) as w:
                begun = w.request({"cmd": "begin", "scenario": two_party()})
                servers = sync_servers()
                listen = environ_of(servers[0]).get("SYNC_HOST") if len(servers) == 1 else None
                w.request({"cmd": "end"})
                recorded = run(self, w, two_party())
        seen = proxy.seen
        self.assertEqual(recorded, expected)  # S9: recording changes no observation
        endpoints = begun["sync"]["endpoints"]
        self.assertEqual(endpoints["recorder"], proxy.address)
        self.assertTrue(endpoints["converge"].startswith("http://127.0.0.1:"), endpoints)
        self.assertFalse(endpoints["actors"].startswith("http://127.0.0.1:"), endpoints)
        self.assertEqual(listen, "0.0.0.0")  # S3.1: the recorder and the converge client both reach it
        sync_steps = {"client_a": {1, 2, 3}, "client_c": {7, 8, 9, 10}}
        self.assertEqual({a: {s for (x, s, _) in seen if x == a} for a in sync_steps}, sync_steps)
        self.assertEqual({a for (a, _, _) in seen}, set(sync_steps))  # the converge client never passes
        self.assertTrue(all(path.startswith("/sync/") for (_, _, path) in seen), seen)


class Credentials(unittest.TestCase):
    """S4.5's credentials against the scenario's server; S4.2 and S4.4 for what
    a refused login leaves."""

    def test_a_wrong_password_is_auth_failed_and_takes_the_host_key(self):
        steps = [
            {"advance": "1m"},                                              # 0
            {"observe": ["db_dump"]},                                       # 1
            {"sync": {"kind": "login", "credentials": "wrong_password"}},   # 2
            {"observe": ["db_dump"]},                                       # 3
            {"sync": {"kind": "login", "credentials": "account"}},          # 4
            {"sync": {"kind": "normal"}},                                   # 5
            {"sync": {"kind": "login", "credentials": "wrong_password"}},   # 6
            {"sync": {"kind": "normal"}},                                   # 7
        ]
        with WorkerProcess("UTC") as w:
            begun = w.request({"cmd": "begin", "scenario": actor_scenario("t/credentials", ["client_a"], steps)})
            resp = [w.request({"cmd": "step"}) for _ in steps]
            w.request({"cmd": "end"})
            leftovers = sorted(os.listdir(w.scratch))
        self.assertTrue(begun["ok"], begun)
        self.assertTrue(all(r["ok"] for r in resp[:7]), resp)
        out = [r["observation"] for r in resp[:7]]
        self.assertEqual(out[2], AUTH_FAILED)
        self.assertEqual(out[3], out[1])  # S4.4: the collection does not change
        # S4.5: account is the default's credentials; S4.1: the server's new
        # collection has no cards, so the actor may only upload.
        self.assertEqual(out[4:6], [OK, {"sync": {"result": "full_upload"}}])
        self.assertEqual(out[6], AUTH_FAILED)
        # S4.2: the refused login took the key that step 4's login gave.
        err = resp[7].get("error", {})
        self.assertEqual((resp[7]["ok"], err.get("kind"), err.get("step")), (False, "invalid_scenario", 7))
        self.assertEqual(leftovers, ["clock"])
        self.assertEqual(sync_servers(), [])

    def test_wrong_password_is_the_account_password_followed_by_wrong(self):
        from driver.ops.sync_client import login_credentials
        from driver.ops.sync_server import PASSWORD, USERNAME

        # USERNAME and PASSWORD are the account a begin reports (test above:
        # the server's SYNC_USER1 is begin's account).
        self.assertEqual(login_credentials("account"), (USERNAME, PASSWORD))
        self.assertEqual(login_credentials("wrong_password"), (USERNAME, PASSWORD + "-wrong"))


class AuthFailed(unittest.TestCase):
    """S4.4: a 403 is auth_failed for every kind. The stand-in recorder answers
    the chosen steps as the server answers a host key it does not know."""

    def test_every_kind_maps_a_403_to_auth_failed(self):
        steps = [
            {"advance": "1m"},                       # 0
            {"sync": {"kind": "login"}},             # 1
            {"observe": ["db_dump"]},                # 2
            {"sync": {"kind": "normal"}},            # 3: refused
            {"observe": ["db_dump"]},                # 4
            {"sync": {"kind": "login"}},             # 5
            {"sync": {"kind": "full_upload"}},       # 6: refused
            {"sync": {"kind": "login"}},             # 7
            {"sync": {"kind": "full_upload"}},       # 8
            {"advance": "1m"},                       # 9
            {"actor": "client_c"},                   # 10
            {"sync": {"kind": "login"}},             # 11
            {"observe": ["db_dump"]},                # 12
            {"sync": {"kind": "full_download"}},     # 13: refused
            {"observe": ["db_dump"]},                # 14
            {"sync": {"kind": "login"}},             # 15
            {"sync": {"kind": "full_download"}},     # 16
            {"observe": ["card(#c1)"]},              # 17
        ]
        refuse = {("client_a", 3), ("client_a", 6), ("client_c", 13)}
        with ForwardProxy(refuse) as proxy:
            with WorkerProcess("UTC", extra_env={"ORACLE_SYNC_RECORDER": proxy.address}) as w:
                out = run(self, w, actor_scenario("t/auth", ["client_a", "client_c"], steps))
        self.assertEqual([out[i] for i in (3, 6, 13)], [AUTH_FAILED] * 3)
        self.assertEqual(out[4], out[2])  # S4.4: the collection does not change
        self.assertEqual(out[14], out[12])
        self.assertEqual([out[i] for i in (1, 5, 7, 8, 11, 15, 16)], [OK] * 7)
        self.assertEqual(out[17]["card"]["#c1"]["nid"], "#n1")
        self.assertTrue(refuse <= {(a, s) for (a, s, _) in proxy.seen}, proxy.seen)

    def test_after_auth_failed_the_actor_holds_no_host_key(self):
        # status asks the server here: the collection never synced (meta-status.md M6).
        for kind in ("normal", "status"):
            with self.subTest(kind=kind):
                steps = [{"advance": "1m"}, {"sync": {"kind": "login"}}, {"sync": {"kind": kind}},
                         {"sync": {"kind": kind}}]
                with ForwardProxy({("client_a", 2)}) as proxy:
                    with WorkerProcess("UTC", extra_env={"ORACLE_SYNC_RECORDER": proxy.address}) as w:
                        w.request({"cmd": "begin", "scenario": actor_scenario("t/key", ["client_a"], steps)})
                        resp = [w.request({"cmd": "step"}) for _ in steps]
                        w.request({"cmd": "end"})
                self.assertEqual([r.get("observation") for r in resp[:3]], [{}, OK, AUTH_FAILED])
                # S4.2: the key step 1's login gave is gone, so step 3 is a run
                # error and never reaches the server.
                err = resp[3].get("error", {})
                self.assertEqual((resp[3]["ok"], err.get("kind"), err.get("step")), (False, "invalid_scenario", 3))
                self.assertEqual({s for (_, s, _) in proxy.seen}, {1, 2})


class Status(unittest.TestCase):
    """S4.1's status kind (specs/sync/meta-status.md M6). The stand-in recorder
    shows which steps reached the server."""

    def test_the_remembered_answer_serves_299s_and_not_300s(self):
        steps = [
            {"actor": "client_c"},                     # 0: an empty collection
            {"advance": "1m"},                         # 1
            {"sync": {"kind": "login"}},               # 2
            {"sync": {"kind": "normal"}},              # 3: remembered from start + 60 s
            {"actor": "client_a"},                     # 4
            {"advance": "1m"},                         # 5
            {"sync": {"kind": "login"}},               # 6
            {"sync": {"kind": "full_upload"}},         # 7: the server's record changes
            {"actor": "client_c"},                     # 8
            {"advance": "3m59s"},                      # 9: 299 s after step 3
            STATUS,                                    # 10
            {"advance": "1s"},                         # 11: 300 s
            STATUS,                                    # 12
            STATUS,                                    # 13
        ]
        with ForwardProxy() as proxy:
            with WorkerProcess("UTC", extra_env={"ORACLE_SYNC_RECORDER": proxy.address}) as w:
                out = run(self, w, actor_scenario("t/remembered", ["client_a", "client_c"], steps))
        # M3: both collections new, both mod 0: no changes. M6.1: a sync job
        # that found no changes is remembered as such.
        self.assertEqual(out[3], result("no_changes"))
        self.assertEqual(out[7], OK)
        self.assertEqual(out[10], result("no_changes"))  # 299 s: remembered
        # 300 s: the server is asked, and its schema time differs (M3).
        self.assertEqual(out[12], result("full_sync"))
        self.assertEqual(out[13], result("full_sync"))  # M6 step 3: that answer is remembered
        seen = {a: {s for (x, s, _) in proxy.seen if x == a} for a in ("client_a", "client_c")}
        self.assertEqual(seen, {"client_a": {6, 7}, "client_c": {2, 3, 12}})


@needs_faults
class Faults(unittest.TestCase):
    def proxies(self):
        with urllib.request.build_opener(urllib.request.ProxyHandler({})).open(f"http://{FAULTS}/proxies", timeout=5) as r:
            return [name for name in json.load(r) if name.startswith("meguri-oracle-")]

    def test_a_reset_gives_network_and_the_collection_reopens_unchanged(self):
        steps = UPLOAD + [
            {"advance": "1m"}, {"actor": "client_c"}, {"sync": {"kind": "login"}}, {"sync": {"kind": "normal"}},
            {"observe": ["db_dump"]},
            {"net_fault": {"kind": "reset", "direction": "to_client"}},
            {"net_fault": {"kind": "reset", "direction": "to_client"}},  # set twice: no change (S5)
            {"sync": {"kind": "full_download"}},
            {"reopen": {}},
            {"observe": ["db_dump"]},
            {"net_fault": {"kind": "clear"}},
            {"net_fault": {"kind": "reset", "direction": "to_server"}},
            {"sync": {"kind": "full_download"}},
            {"net_fault": {"kind": "clear"}},
            {"sync": {"kind": "full_download"}},
            {"observe": ["card(#c1)"]},
            {"converge": {}},
        ]
        with WorkerProcess("UTC") as w:
            out = run(self, w, actor_scenario("t/reset", ["client_a", "client_c"], steps))
        self.assertEqual(out[9:11], [{}, {}])
        self.assertEqual(out[11], {"sync": {"error": "network"}})
        self.assertEqual(out[13], out[8])  # unchanged after the failed download and a reopen
        self.assertEqual(out[16], {"sync": {"error": "network"}})
        self.assertEqual(out[18], OK)
        self.assertEqual(out[19]["card"]["#c1"]["nid"], "#n1")
        # The actors went through the fault proxy; the converge client reached
        # the server over loopback, outside it (S3.1, S7.1).
        self.assertEqual(out[20]["converge"]["differences"], {"client_a": [], "client_c": []})
        self.assertEqual(self.proxies(), [])  # deleted with the session

    def test_status_answers_without_a_request_while_every_connection_resets(self):
        steps = UPLOAD + [                                                  # 0-3: client_a uploads
            {"net_fault": {"kind": "reset", "direction": "to_server"}},     # 4
            {"sync": {"kind": "normal"}},                                   # 5: always asks
            STATUS,                                                         # 6
            {"advance": "1m"},                                              # 7
            MOON,                                                           # 8
            STATUS,                                                         # 9
            {"db_exec": {"sql": "UPDATE col SET scm = ls + 1000"}},         # 10
            STATUS,                                                         # 11
            {"net_fault": {"kind": "clear"}},                               # 12
        ]
        with WorkerProcess("UTC") as w:
            out = run(self, w, actor_scenario("t/status", ["client_a"], steps))
        self.assertEqual(out[3], OK)
        self.assertEqual(out[5], {"sync": {"error": "network"}})  # every request fails from step 4
        # meta-status.md M6.1: a one-way sync that succeeded is remembered as
        # no changes, and a failed job leaves that answer.
        self.assertEqual(out[6], result("no_changes"))
        self.assertEqual(out[8], {"add_note": {"note": "#n2", "cards": ["#c2"]}})
        # M5, M6 step 1: local changes, then a schema time past the last sync,
        # answered from the collection alone.
        self.assertEqual(out[9], result("normal_sync"))
        self.assertEqual(out[11], result("full_sync"))
        self.assertEqual(self.proxies(), [])

    def test_a_scripted_caller_plays_a_meguri_actor(self):
        from anki.collection import Collection
        from anki.sync import SyncAuth
        import shutil
        import tempfile

        from driver.observe.db_dump import dump

        steps = UPLOAD + [
            {"advance": "1m"},
            {"actor": "meguri"},                                            # 5: global
            {"sync": {"kind": "login"}},                                    # 6: caller
            {"sync": {"kind": "full_download"}},                            # 7: caller
            {"add_note": {"notetype": "Pair", "deck": "Words", "fields": ["x", "y"]}},  # 8: caller, reported only
            {"actor": "client_a"},                                          # 9
            {"observe": ["ids"]},                                           # 10: worker
            {"converge": {}},                                               # 11
        ]
        sc = actor_scenario("t/mixed", ["client_a", "meguri"], steps)
        t = epoch(START, "UTC") * 1000
        tmp = tempfile.mkdtemp(dir=os.environ.get("ORACLE_SCRATCH_ROOT"))
        col = Collection(os.path.join(tmp, "collection.anki2"))
        try:
            with WorkerProcess("UTC") as w:
                bad_caller = w.request({"cmd": "begin", "scenario": sc, "caller_actors": ["client_a"]})
                bad_ids = w.request({"cmd": "begin", "scenario": sc, "caller_actors": ["meguri"],
                                     "ids": {"notes": [], "cards": []}})
                begun = w.request({"cmd": "begin", "scenario": sc, "caller_actors": ["meguri"]})
                self.assertTrue(begun["ok"], begun)
                sync = begun["sync"]
                servers = sync_servers()
                server_env = environ_of(servers[0]) if len(servers) == 1 else {}
                resp = [w.request({"cmd": "step"}) for _ in range(6)]  # 0-5 run on the worker
                stray = w.request({"cmd": "step"})  # step 6 is the caller's
                # The caller logs in and downloads through the run's endpoint.
                endpoint = sync["endpoints"]["actors"]
                auth = col.sync_login(username=sync["account"]["username"],
                                      password=sync["account"]["password"], endpoint=endpoint)
                resp.append(w.request({"cmd": "step", "done": {"output": OK, "notes": [], "cards": []}}))
                col.close_for_full_sync()
                try:
                    col.full_upload_or_download(auth=SyncAuth(hkey=auth.hkey, endpoint=endpoint), server_usn=None,
                                                upload=False)
                finally:
                    col.reopen(after_full_sync=True)
                resp.append(w.request({"cmd": "step", "done": {"output": OK, "notes": [], "cards": []}}))
                added = {"add_note": {"note": "#n2", "cards": ["#c2"]}}
                resp.append(w.request({"cmd": "step", "done": {"output": added, "notes": [7], "cards": [8]}}))
                with_done = w.request({"cmd": "step", "done": {"output": {}, "notes": [], "cards": []}})
                resp += [w.request({"cmd": "step"}) for _ in range(2)]  # 9, 10
                no_dumps = w.request({"cmd": "step"})
                wrong = w.request({"cmd": "step", "dumps": {"meguri": {}, "client_a": {}}})
                resp.append(w.request({"cmd": "step", "dumps": {"meguri": dump(col, raw=True)}}))
                ended = w.request({"cmd": "end"})
        finally:
            col.close()
            shutil.rmtree(tmp, ignore_errors=True)
        self.assertEqual(bad_caller["error"]["kind"], "protocol_error")  # an oracle actor
        self.assertEqual(bad_ids["error"]["kind"], "protocol_error")  # meguri is not the first actor
        self.assertEqual(begun["ids"], {"notes": [t], "cards": [t]})
        api_host = FAULTS.rpartition(":")[0]
        self.assertRegex(sync["endpoints"]["actors"], rf"^http://{api_host}:\d+/$")
        self.assertRegex(sync["endpoints"]["converge"], r"^http://127\.0\.0\.1:\d+/$")
        self.assertRegex(sync["endpoints"]["upstream"], r"^\d+\.\d+\.\d+\.\d+:\d+$")
        self.assertIsNone(sync["endpoints"]["recorder"])
        # S3.1: the fault proxy reaches the server on oracle-internal and the
        # converge client on loopback, so it listens on every address; the
        # converge step below (differences []) is the loopback connection.
        self.assertEqual(len(servers), 1)
        self.assertEqual(server_env.get("SYNC_HOST"), "0.0.0.0")
        self.assertEqual(server_env.get("SYNC_PORT"), sync["endpoints"]["converge"].rstrip("/").rpartition(":")[2])
        for bad in (stray, with_done, no_dumps, wrong):
            self.assertEqual(bad["error"]["kind"], "protocol_error", bad)
        self.assertTrue(all(r["ok"] for r in resp), resp)
        self.assertEqual([r["index"] for r in resp], list(range(12)))
        self.assertEqual(resp[8]["ids"], {"notes": [7], "cards": [8]})
        self.assertEqual(resp[10]["observation"], {"ids": {"#n1": t, "#n2": 7, "#c1": t, "#c2": 8}})
        self.assertEqual(resp[11]["observation"]["converge"]["differences"], {"client_a": [], "meguri": []})
        self.assertEqual(ended["observations"]["steps"][8], added)
        self.assertEqual(self.proxies(), [])


if __name__ == "__main__":
    unittest.main()
