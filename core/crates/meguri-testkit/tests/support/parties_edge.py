"""Synthetic lease protocol for Parties tests; no transport parity claim."""
import json
import pathlib
import sys
import time
import uuid

log, mode, endpoint = sys.argv[1:]
request = json.load(sys.stdin)
with open(log, "a", encoding="utf-8") as stream:
    stream.write(json.dumps(request) + "\n")
if request["cmd"] == "release":
    if mode == "release-unknown":
        print('{"released":true,"unexpected":1}')
    elif mode == "release-json":
        print('invalid response')
    elif mode == "release-exit":
        sys.exit(9)
    elif mode == "release-fail":
        print('{"released":false}')
    else:
        print('{"released":true}')
elif mode == "exit":
    sys.exit(9)
elif mode == "invalid-json":
    print('invalid response')
elif mode == "panic-helper":
    # The bounded-invocation test interrupts this sleeping child.
    pathlib.Path(log + ".pid").write_text(str(__import__('os').getpid()))
    time.sleep(120)
else:
    if mode == "barrier":
        deadline = time.monotonic() + 2
        while sum(json.loads(line)["cmd"] == "acquire" for line in pathlib.Path(log).read_text().splitlines()) < 2:
            if time.monotonic() >= deadline:
                sys.exit(8)
            time.sleep(0.01)
    response = {"endpoint": endpoint, "lease": uuid.uuid4().hex}
    if mode == "unknown":
        response["unexpected"] = "no"
    if mode == "empty":
        response["endpoint"] = ""
    if mode == "empty-lease":
        response["lease"] = ""
    print(json.dumps(response))
    if mode == "response-exit":
        sys.stdout.flush()
        sys.exit(9)
