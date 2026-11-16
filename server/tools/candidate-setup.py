# SPDX-License-Identifier: MIT
"""Job-owned synthetic setup at the fixed isolated candidate endpoint."""
import http.client
import json
import pathlib
import sys
import time


def main():
    credentials = json.load(sys.stdin)
    if set(credentials) != {"username", "password"} or not all(
        isinstance(value, str) and value for value in credentials.values()
    ):
        raise ValueError("invalid synthetic credentials")
    deadline = time.monotonic() + 20
    while True:
        try:
            connection = http.client.HTTPConnection("127.0.0.1", 8080, timeout=2)
            connection.request("GET", "/health")
            response = connection.getresponse()
            healthy = response.status == 200
            response.read()
            connection.close()
            if healthy:
                break
        except OSError:
            pass
        if time.monotonic() >= deadline:
            raise ValueError("candidate unavailable")
        time.sleep(0.05)
    token = pathlib.Path("/data/owner/state/setup-token").read_text().strip()
    body = json.dumps(dict(credentials, token=token)).encode()
    connection = http.client.HTTPConnection("127.0.0.1", 8080, timeout=20)
    connection.request("POST", "/api/setup", body=body, headers={
        "Content-Type": "application/json", "Origin": "https://candidate.example.test"
    })
    response = connection.getresponse()
    if response.status != 201:
        raise ValueError("candidate setup failed")
    response.read()
    connection.close()


if __name__ == "__main__":
    try:
        main()
    except Exception:
        # Never print credentials, response bodies or exception representations.
        sys.stderr.write("synthetic candidate setup failed\n")
        sys.exit(1)
