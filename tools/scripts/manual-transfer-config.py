#!/usr/bin/env python3
"""RF4 synthetic account derivation. Secrets travel only over stdin/stdout pipes."""
import json
import sys


def accounts(path):
    entries = {}
    with open(path, encoding="utf-8") as stream:
        for line in stream:
            key, sep, value = line.strip().partition("=")
            if sep and key in ("SYNC_USER3", "SYNC_USER4"):
                if key in entries:
                    raise ValueError("duplicate synthetic account")
                entries[key] = value
    a, b = (entries[key].split(":", 1) for key in ("SYNC_USER3", "SYNC_USER4"))
    # This is a dedicated generated-data job, never a generic account loader.
    if a != ["meguri-c", "meguri-test-c"] or b != ["meguri-d", "meguri-test-d"]:
        raise ValueError("reserved synthetic examples changed")
    return a, b


def main():
    mode, path = sys.argv[1:3]
    a, b = accounts(path)
    if mode == "check-users":
        inventory = json.load(sys.stdin)
        if len(inventory) != 1:
            raise ValueError("expected one rig container")
        env = inventory[0]["Config"]["Env"]
        for key, pair in (("SYNC_USER3", a), ("SYNC_USER4", b)):
            expected = key + "=" + ":".join(pair)
            if [entry for entry in env if entry.startswith(key + "=")] != [expected]:
                raise ValueError("rig synthetic account mismatch")
    elif mode == "manifest":
        port = int(sys.argv[3])
        if port != 18765:
            raise ValueError("unscoped VM port")
        json.dump(dict(endpoint="http://127.0.0.1:%d/" % port,
                       username=a[0], password=a[1], deckID=1,
                       exportUsername=b[0], exportPassword=b[1]), sys.stdout)
    else:
        raise ValueError("unknown operation")


if __name__ == "__main__":
    try:
        main()
    except (ValueError, KeyError, IndexError, OSError, TypeError):
        # Never include input text, exception reprs or an inspect inventory.
        sys.stderr.write("manual-transfer-data: synthetic configuration refused\n")
        sys.exit(1)
