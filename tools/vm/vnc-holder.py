# Holds the one VNC connection to the test VM for tools/vm/duo-fold.sh: Tart crashes
# when clients reconnect. Reads commands from a FIFO, one per line:
#   move <x> <y> | click | key <name> | shot <path> | quit
# usage: DUO_FOLD_VNC_URL=vnc://:<password>@<host>:<port> vnc-holder.py <fifo> <pid file>
import json
import math
import os
from pathlib import Path
import sys
import time

from vncdotool import api
from vncdotool.client import VNCDoToolClient, VNCDoToolFactory

quitting = False


class ResizingVNCClient(VNCDoToolClient):
    def connectionLost(self, reason):
        try:
            super().connectionLost(reason)
        finally:
            if not quitting:
                # Twisted calls this on its reactor thread. The command thread
                # may be blocked opening/reading the FIFO indefinitely.
                print("disconnected", flush=True)
                os._exit(1)

    def postureClick(self, deadline, receipt):
        # This entire method runs synchronously on the existing reactor thread.
        # A delayed proxy invocation may refuse, but can never click after expiry.
        result = {"joined": True, "executed": False}
        try:
            if not math.isfinite(deadline) or time.monotonic() >= deadline:
                result["error"] = "expired"
            else:
                result["executed"] = True
                try:
                    self.mouseDown(1)
                finally:
                    self.mouseUp(1)
        except Exception:
            # These two RFB writes leave no deferred input on this thread.
            result["error"] = "input"
        temporary = Path(receipt + ".tmp")
        temporary.write_text(json.dumps(result) + "\n")
        temporary.replace(receipt)
        return self

    def captureScreen(self, *args, **kwargs):
        # Tart can resize the image while request bounds retain the boot size.
        if self.screen is not None:
            self.width, self.height = self.screen.size
        return super().captureScreen(*args, **kwargs)


class ResizingVNCFactory(VNCDoToolFactory):
    protocol = ResizingVNCClient


def run(client, line):
    name, _, rest = line.partition(" ")
    if name == "move":
        x, y = rest.split()
        client.mouseMove(int(x), int(y))
    elif name == "click":
        client.mousePress(1)
    elif name == "posture-click":
        deadline, receipt = rest.split(" ", 1)
        client.postureClick(float(deadline), receipt)
    elif name == "key":
        client.keyPress(rest)
    elif name == "shot":
        client.captureScreen(rest)
    else:
        raise ValueError(f"unknown command {name}")


def main():
    global quitting
    fifo, pid_file = sys.argv[1], sys.argv[2]
    secret, _, address = os.environ["DUO_FOLD_VNC_URL"].removeprefix("vnc://:").rpartition("@")
    port = address.rpartition(":")[2]
    client = api.connect(f"127.0.0.1::{port}", password=secret,
                         factory_class=ResizingVNCFactory)
    client.timeout = 60
    client.factory.force_caps = True
    # Force authentication before publishing readiness, using this connection.
    client.refreshScreen()
    with open(pid_file, "w") as f:
        f.write(str(os.getpid()))
    print("connected", flush=True)
    print("posture-click-v1", flush=True)
    while True:
        with open(fifo) as commands:
            for raw in commands:
                line = raw.strip()
                if line == "quit":
                    quitting = True
                    client.disconnect()
                    api.shutdown()
                    return
                try:
                    run(client, line)
                    print("ok", line.split(" ")[0], flush=True)
                except Exception as error:
                    print("error", line.split(" ")[0], error, flush=True)


if __name__ == "__main__":
    main()
