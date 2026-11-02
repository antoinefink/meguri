# One long-lived VNC connection, so Tart's server sees a single client. Commands come from a FIFO:
#   move x y | click | key name | type text | shot path | quit
import os, sys, time
from vncdotool import api
url, cmds = sys.argv[1], sys.argv[2]
p = url.split("vnc://:",1)[1]; pw, hp = p.rsplit("@",1); port = hp.rsplit(":",1)[1]
c = api.connect(f"127.0.0.1::{port}", password=pw)
c.timeout = 60
try: c.factory.force_caps = True
except Exception: pass
print("connected", flush=True)
while True:
    with open(cmds) as f:
        for line in f:
            a = line.strip().split(" ",1)
            try:
                if a[0]=="move": x,y=a[1].split(); c.mouseMove(int(x),int(y))
                elif a[0]=="click": c.mousePress(1)
                elif a[0]=="key": c.keyPress(a[1])
                elif a[0]=="type":
                    for ch in a[1]: c.keyPress(ch)
                elif a[0]=="shot": c.captureScreen(a[1])
                elif a[0]=="quit": c.disconnect(); api.shutdown(); sys.exit(0)
                print("ok", line.strip()[:40], flush=True)
            except Exception as e:
                print("error", line.strip()[:40], e, flush=True)
