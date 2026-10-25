"""Bind a fixed TCP forwarding endpoint using the shared socket routines.

Trusted arguments select its destination. Binding the listener makes no
upstream connection; clock and account settings do not enter this process.
"""
import asyncio
import importlib.util
import sys

def load_relay(path="/opt/rig_forward.py"):
    spec = importlib.util.spec_from_file_location("route_bytes", path)
    relay = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(relay)
    return relay

if __name__ == "__main__":
    host, port = sys.argv[1:]
    relay = load_relay()
    # listen binds without contacting upstream; serve connects only on accept.
    async def main():
        await relay.listen(asyncio.get_running_loop(), 8080, host, int(port))
    asyncio.run(main())
