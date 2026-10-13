"""The sync rig's only published container (docker/compose.yml `sync-edge`).

The rig's server and proxies sit on an internal network: it has no route out,
and Docker publishes no port from it. This process is on that network and on
the publishing one, and forwards each port it listens on to one fixed rig
service. Its routes are constants, so nothing a client sends, toxiproxy API
calls included, can make the rig connect beyond its own services (R1876).

A reset on either side is passed on as a reset (SO_LINGER 0), so toxiproxy's
reset_peer reaches the client as one; an orderly close is passed on as a
half-close.
"""

import asyncio
import socket
import struct
import sys

# Listening port -> (rig service, port). Never taken from a request or the
# environment.
ROUTES = {
    8080: ("syncserver", 8080),
    8081: ("toxiproxy", 8081),
    8474: ("toxiproxy", 8474),
    8082: ("mitmdump", 8080),
}
CHUNK = 64 * 1024
TASKS = set()


def reset(sock):
    try:
        sock.setsockopt(socket.SOL_SOCKET, socket.SO_LINGER, struct.pack("ii", 1, 0))
    except OSError:
        pass
    sock.close()


async def copy(loop, src, dst):
    """Copies src to dst until src ends (True, dst half-closed) or fails (False)."""
    while True:
        try:
            data = await loop.sock_recv(src, CHUNK)
        except OSError:
            return False
        if not data:
            try:
                dst.shutdown(socket.SHUT_WR)
            except OSError:
                return False
            return True
        try:
            await loop.sock_sendall(dst, data)
        except OSError:
            return False


async def connect(loop, host, port):
    for family, kind, proto, _, addr in await loop.getaddrinfo(host, port, type=socket.SOCK_STREAM):
        sock = socket.socket(family, kind, proto)
        sock.setblocking(False)
        try:
            await loop.sock_connect(sock, addr)
            return sock
        except OSError:
            sock.close()
    raise OSError(f"cannot connect to {host}:{port}")


async def serve(loop, client, host, port):
    try:
        upstream = await connect(loop, host, port)
    except OSError:
        reset(client)
        return
    copies = [loop.create_task(copy(loop, client, upstream)), loop.create_task(copy(loop, upstream, client))]
    clean = True
    for done in asyncio.as_completed(copies):
        if not await done:
            clean = False
            break
    if clean:
        client.close()
        upstream.close()
        return
    for task in copies:
        task.cancel()
    await asyncio.gather(*copies, return_exceptions=True)
    reset(client)
    reset(upstream)


async def listen(loop, listen_port, host, port):
    server = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    server.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    server.bind(("0.0.0.0", listen_port))
    server.listen(64)
    server.setblocking(False)
    while True:
        client, _ = await loop.sock_accept(server)
        client.setblocking(False)
        task = loop.create_task(serve(loop, client, host, port))
        TASKS.add(task)
        task.add_done_callback(TASKS.discard)


async def main():
    loop = asyncio.get_running_loop()
    for listen_port, (host, port) in ROUTES.items():
        print(f"sync-edge: :{listen_port} -> {host}:{port}", file=sys.stderr, flush=True)
    await asyncio.gather(*(listen(loop, lp, host, port) for lp, (host, port) in ROUTES.items()))


if __name__ == "__main__":
    asyncio.run(main())
