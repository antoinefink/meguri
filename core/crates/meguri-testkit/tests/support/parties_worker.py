"""A stand-in oracle worker for actor scenarios (tests/parties.rs).

It speaks the interactive part of conformance/schema/protocol.json (hello,
begin, step, end) for actor scenarios, plays every actor the caller does not,
and checks the caller's requests the way sync-ops.md S10.4 asks a worker to:
`done` exactly for steps of caller-played actors, `dumps` exactly for
`converge` with caller actors. It runs no Anki: each op it plays has a fixed
output, so tests can write the golden by hand.

- sync: {"sync": {"result": "ok"}}, or "no_changes" for a normal sync
- add_note: note ids 1001, 1002, ... and card ids 2001, 2002, ...
- converge: {"converge": {"differences": {<actor>: []}, "dump": {"col": "converged"}}};
  a caller's dump other than {"col": "converged"} differs at /col
- observe: {"<kind>": "oracle"} per request
- every other op: {}
- the setup: one note and one card per setup note, ids as add_note's

Environment: TZ, LOCK; LOG, a file that receives every request line;
ACTORS, CONVERGE, UPSTREAM and RECORDER, the endpoints begin reports (empty:
null); OMIT, a comma-separated list of those names (lower case) begin leaves
out of `endpoints`; FAIL_BEGIN, an error kind begin answers with instead.
"""

import json
import os
import sys

LOCK = os.environ["LOCK"]
TZ = os.environ.get("TZ", "UTC")


def endpoint(name, default):
    value = os.environ.get(name, default)
    return value or None


def reply(value):
    encoded = json.dumps(value, separators=(",", ":"))
    if os.environ.get("ROUTE_DUPLICATE") and isinstance(value.get("sync", {}).get("route"), dict):
        encoded = encoded.replace('"route":{"proxy":', '"route":{"worker":"foreign","proxy":')
    sys.stdout.write(encoded + "\n")
    sys.stdout.flush()


def error(kind, **fields):
    reply({"ok": False, "error": dict(fields, kind=kind)})


class Session:
    def __init__(self, request):
        scenario = request["scenario"]
        self.id = scenario["id"]
        self.steps = scenario.get("steps", [])
        self.actors = scenario["actors"]
        self.caller = request.get("caller_actors", [])
        self.current = self.actors[0]
        self.setup_notes = scenario.get("setup", {}).get("notes", [])
        self.outputs = []
        self.notes = 0
        self.cards = 0

    def new_note(self):
        self.notes += 1
        self.cards += 1
        return 1000 + self.notes, 2000 + self.cards

    def setup(self):
        ids = {"notes": [], "cards": []}
        for _ in self.setup_notes:
            note, card = self.new_note()
            ids["notes"].append(note)
            ids["cards"].append(card)
        return ids

    def play(self, op, args):
        if op == "sync":
            return {"sync": {"result": "no_changes" if args["kind"] == "normal" else "ok"}}, None
        if op == "add_note":
            note, card = self.new_note()
            output = {"add_note": {"note": f"#n{note - 1000}", "cards": [f"#c{card - 2000}"]}}
            return output, {"notes": [note], "cards": [card]}
        if op == "observe":
            out = {}
            for item in args:
                kind = item if isinstance(item, str) else next(iter(item))
                out[kind.split("(")[0].strip()] = "oracle"
            return out, None
        return {}, None


def main():
    session = None
    for line in sys.stdin:
        with open(os.environ["LOG"], "a", encoding="utf-8") as log:
            log.write(line)
        request = json.loads(line)
        cmd = request["cmd"]
        if cmd == "hello":
            reply({"ok": True, "protocol": 1, "anki_version": "26.9.3", "lock_sha256": LOCK,
                   "driver_sha256": "b" * 64, "tz": TZ})
        elif cmd == "begin":
            if os.environ.get("FAIL_BEGIN"):
                error(os.environ["FAIL_BEGIN"], detail="stand-in")
                continue
            session = Session(request)
            if session.actors[0] in session.caller:
                given = request.get("ids", {"notes": [], "cards": []})
                session.notes, session.cards = len(given["notes"]), len(given["cards"])
                ids = {"notes": [], "cards": []}
            else:
                ids = session.setup()
            endpoints = {"actors": endpoint("ACTORS", "http://172.20.0.5:31000/"),
                         "converge": endpoint("CONVERGE", "http://127.0.0.1:27701/"),
                         "upstream": endpoint("UPSTREAM", "172.20.0.4:27701"),
                         "recorder": endpoint("RECORDER", "")}
            for name in filter(None, os.environ.get("OMIT", "").split(",")):
                del endpoints[name]
            if session.caller and os.environ.get("MIXED_BAD_IDS"):
                ids = {"notes": "bad", "cards": []}
            route = None
            if session.caller:
                route = {"proxy": "meguri-edge-" + "a" * 32 + "-" + "b" * 32,
                         "worker": os.environ.get("WORKER", "")}
                if os.environ.get("ROUTE_WORKER"):
                    route["worker"] = os.environ["ROUTE_WORKER"]
                if os.environ.get("ROUTE_PROXY"):
                    route["proxy"] = os.environ["ROUTE_PROXY"]
                if os.environ.get("ROUTE_EXTRA"):
                    route["extra"] = "refused"
                if os.environ.get("ROUTE_ARRAY"):
                    route = [route["proxy"], route["worker"]]
                if os.environ.get("ROUTE_NULL"):
                    route = None
            sync = {"endpoints": endpoints, "route": route,
                    "account": {"username": "user1", "password": "test-only"}}
            if os.environ.get("OMIT_ROUTE"):
                del sync["route"]
            if os.environ.get("BEGIN_EXIT"):
                sys.exit(7)
            reply({"ok": True, "scenario_id": session.id, "steps": len(session.steps), "ids": ids,
                   "sync": sync})
        elif cmd == "step":
            index = len(session.outputs)
            if session.caller and os.environ.get("MIXED_STEP_EXIT"):
                sys.exit(7)
            if session.caller and os.environ.get("MIXED_STEP_ERROR"):
                error("stand_in_step_error")
                continue
            if index >= len(session.steps):
                error("no_more_steps")
                continue
            (op, args), = session.steps[index].items()
            caller_step = (op not in ("advance", "set_clock", "net_fault", "converge", "actor")
                           and session.current in session.caller)
            if caller_step != ("done" in request):
                error("protocol_error", step=index, detail="done does not fit the step")
                continue
            if op == "converge" and session.caller and set(request.get("dumps", {})) != set(session.caller):
                error("protocol_error", step=index, detail="dumps do not fit the caller's actors")
                continue
            ids = {"notes": [], "cards": []}
            if op == "actor":
                session.current = args
                output = {}
            elif op == "converge":
                dumps = request.get("dumps", {})
                differences = {a: ([] if dumps.get(a, {"col": "converged"}) == {"col": "converged"} else ["/col"])
                               for a in session.actors}
                output = {"converge": {"differences": differences, "dump": {"col": "converged"}}}
            elif caller_step:
                output = request["done"]["output"]
                ids = {"notes": request["done"]["notes"], "cards": request["done"]["cards"]}
                session.notes += len(ids["notes"])
                session.cards += len(ids["cards"])
            else:
                output, created = session.play(op, args)
                ids = created or ids
            session.outputs.append(output)
            reply({"ok": True, "index": -1 if session.caller and os.environ.get("MIXED_BAD_INDEX") else index, "observation": output, "ids": ids})
        elif cmd == "end":
            if session.caller and os.environ.get("MIXED_END_ERROR"):
                error("stand_in_end_error")
                session = None
                continue
            if session.caller and os.environ.get("MIXED_BAD_END"):
                reply({"ok": True})
                session = None
                continue
            route = None
            if session.caller:
                route = {"proxy": "meguri-edge-" + "a" * 32 + "-" + "b" * 32,
                         "worker": os.environ.get("WORKER", "")}
                if os.environ.get("ROUTE_WORKER"):
                    route["worker"] = os.environ["ROUTE_WORKER"]
                if os.environ.get("ROUTE_PROXY"):
                    route["proxy"] = os.environ["ROUTE_PROXY"]
                if os.environ.get("ROUTE_EXTRA"):
                    route["extra"] = "refused"
                if os.environ.get("ROUTE_ARRAY"):
                    route = [route["proxy"], route["worker"]]
                if os.environ.get("ROUTE_NULL"):
                    route = None
            sync = {"endpoints": endpoints, "route": route,
                    "account": {"username": "user1", "password": "test-only"}}
            if os.environ.get("OMIT_ROUTE"):
                del sync["route"]
            if os.environ.get("BEGIN_EXIT"):
                sys.exit(7)
            reply({"ok": True, "scenario_id": session.id, "observations": {"steps": session.outputs}})
            session = None
        else:
            error("unknown_cmd", cmd=cmd)


main()
