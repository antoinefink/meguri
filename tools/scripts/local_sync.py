#!/usr/bin/env python3
"""Manage only Meguri's separately owned persistent local Docker sync service."""

import argparse
import base64
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import secrets
import socket
import subprocess
import sys
import time
import urllib.error
import urllib.request


CONTAINER = "meguri-local-sync"
NETWORK = "meguri-local-sync"
VOLUME = "meguri-local-sync-data"
LABEL = "dev.meguri.local-sync.owner"
IMAGE_ID = re.compile(r"sha256:[0-9a-f]{64}\Z")


class ServiceError(Exception):
    """An intentionally public, credential-free diagnostic."""


def docker(*args, optional=False):
    # Never relay Docker's error bodies, argv, inspect environment or logs.
    result = subprocess.run(
        ["docker", *args], capture_output=True, text=True, timeout=45, check=False
    )
    if result.returncode:
        if optional:
            return None
        raise ServiceError("Docker operation failed; inspect the named service locally.")
    return result.stdout.strip()


def inspect(kind, name, expression):
    value = docker(kind, "inspect", "--format", "{{json " + expression + "}}", name,
                   optional=True)
    return None if value is None else json.loads(value)


def private_write(path, value):
    if path.is_symlink():
        raise ServiceError("Private configuration must not use symbolic links.")
    temporary = path.with_name(path.name + ".new")
    fd = os.open(str(temporary), os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    try:
        with os.fdopen(fd, "w") as stream:
            stream.write(value)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(str(temporary), str(path))
        parent_fd = os.open(str(path.parent), os.O_RDONLY)
        try:
            os.fsync(parent_fd)
        finally:
            os.close(parent_fd)
    finally:
        if temporary.exists():
            temporary.unlink()


def read_private(path):
    if path.is_symlink() or not path.is_file() or path.stat().st_mode & 0o077:
        raise ServiceError("Private configuration is missing or has unsafe permissions.")
    return path.read_text()


def b64(value):
    return base64.b64encode(value).decode("ascii").rstrip("=")


def new_config(directory, image):
    password = secrets.token_urlsafe(36)
    salt = secrets.token_bytes(16)
    digest = hashlib.pbkdf2_hmac("sha256", password.encode(), salt, 600000, 32)
    hashed = "$pbkdf2-sha256$i=600000,l=32$" + b64(salt) + "$" + b64(digest)
    config = {"image": image, "owner": secrets.token_hex(24), "port": None}
    private_write(directory / "server.env", "SYNC_HOST=0.0.0.0\nSYNC_PORT=8080\n"
                  "SYNC_BASE=/data\nPASSWORDS_HASHED=1\nSYNC_USER1=meguri:" + hashed + "\n")
    private_write(directory / "client.json", json.dumps(
        {"username": "meguri", "password": password, "endpoint": None}) + "\n")
    save_config(directory, config)
    return config


def save_config(directory, config):
    private_write(directory / "config.json", json.dumps(config) + "\n")


def owned_resources(config):
    found = {}
    for kind, name in (("container", CONTAINER), ("network", NETWORK), ("volume", VOLUME)):
        labels = inspect(kind, name, ".Config.Labels" if kind == "container" else ".Labels")
        if labels is not None and (not config or labels.get(LABEL) != config["owner"]):
            raise ServiceError("Refusing a foreign resource with the reserved local-sync name.")
        # Docker represents an unlabelled object's labels as null: distinguish absence.
        if labels is None and inspect(kind, name, ".Name") is not None:
            raise ServiceError("Refusing an unlabelled resource with the reserved local-sync name.")
        found[kind] = labels is not None
    return found


def health(port):
    try:
        # Explicitly bypass host HTTP proxies for this loopback-only listener.
        opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
        with opener.open("http://127.0.0.1:" + str(port) + "/health", timeout=1) as response:
            return response.status == 200
    except (OSError, urllib.error.URLError):
        return False


def bound_port():
    ports = inspect("container", CONTAINER, ".NetworkSettings.Ports") or {}
    bindings = ports.get("8080/tcp") or []
    if len(bindings) != 1 or bindings[0].get("HostIp") != "127.0.0.1":
        raise ServiceError("The local service does not have its required loopback binding.")
    return int(bindings[0]["HostPort"])


def available_port():
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as listener:
        listener.bind(("127.0.0.1", 0))
        return listener.getsockname()[1]


def start(directory, config, found):
    image = config["image"]
    run_label = "meguri.mg.run-id=" + os.environ["MG_RUN_ID"]
    if inspect("image", image, ".Id") != image:
        raise ServiceError("The immutable runtime image must already be installed.")
    if found["container"]:
        if inspect("container", CONTAINER, ".Image") != image:
            raise ServiceError("The retained service uses another image; refusing to replace it.")
        ports = inspect("container", CONTAINER, ".HostConfig.PortBindings") or {}
        expected = [{"HostIp": "127.0.0.1", "HostPort": str(config["port"])}]
        if not config["port"] or ports.get("8080/tcp") != expected:
            raise ServiceError("The retained service does not have its configured fixed loopback port.")
    label = LABEL + "=" + config["owner"]
    if not found["network"]:
        docker("network", "create", "--label", label, NETWORK)
    if not found["volume"]:
        docker("volume", "create", "--label", label, VOLUME)
    owned_resources(config)
    # Retry after interrupted initialization; touch only the owned volume's root.
    docker("run", "--rm", "--label", run_label, "--network", "none", "--read-only",
           "--user", "0:0", "--cap-drop", "ALL", "--cap-add", "CHOWN",
           "--security-opt", "no-new-privileges", "--mount",
           "type=volume,src=" + VOLUME + ",dst=/data", "--entrypoint", "/bin/sh",
           image, "-c", "chown 10001:10001 /data")
    owned_resources(config)
    if not found["container"]:
        if not config["port"]:
            config["port"] = available_port()
            save_config(directory, config)
        port = str(config["port"])
        docker("run", "--detach", "--name", CONTAINER, "--label", label,
               "--label", run_label,
               "--network", NETWORK, "--restart", "unless-stopped", "--stop-signal", "SIGINT",
               "--stop-timeout", "20", "--user", "10001:10001", "--read-only", "--tmpfs",
               "/tmp:rw,nosuid,nodev,size=64m", "--cap-drop", "ALL", "--security-opt",
               "no-new-privileges", "--mount", "type=volume,src=" + VOLUME + ",dst=/data",
               "--publish", "127.0.0.1:" + port + ":8080", "--env-file",
               str(directory / "server.env"), "--entrypoint", "python3", image,
               "-m", "anki.syncserver")
    else:
        docker("container", "start", CONTAINER)
    if bound_port() != config["port"]:
        raise ServiceError("The local service did not retain its configured loopback port.")
    client = json.loads(read_private(directory / "client.json"))
    client["endpoint"] = "http://127.0.0.1:" + str(config["port"]) + "/"
    private_write(directory / "client.json", json.dumps(client) + "\n")
    deadline = time.monotonic() + 30
    while time.monotonic() < deadline:
        if health(config["port"]):
            return
        state = inspect("container", CONTAINER, ".State") or {}
        if not state.get("Running") or state.get("Restarting"):
            break
        time.sleep(0.25)
    raise ServiceError("The retained local service did not become healthy within its startup window.")


def operate(directory, action, image):
    docker("info", "--format", "{{.ServerVersion}}")
    config_path = directory / "config.json"
    config = json.loads(read_private(config_path)) if config_path.exists() else None
    if config and not IMAGE_ID.fullmatch(config["image"]):
        raise ServiceError("The retained image ID is invalid.")
    found = owned_resources(config)
    if action == "up":
        if image and not IMAGE_ID.fullmatch(image):
            raise ServiceError("Supply a complete installed sha256 image ID.")
        if config:
            if image and image != config["image"]:
                raise ServiceError("The retained service is pinned to another image.")
            read_private(directory / "client.json")
            read_private(directory / "server.env")
        else:
            if not image or inspect("image", image, ".Id") != image:
                raise ServiceError("First startup requires an already installed immutable image ID.")
            config = new_config(directory, image)
        start(directory, config, found)
    elif action == "down" and found["container"]:
        docker("container", "stop", "--signal", "SIGINT", "--timeout", "20", CONTAINER)
    state = inspect("container", CONTAINER, ".State") if config else None
    return {"state": (state or {}).get("Status", "absent"),
            "healthy": bool(state and state.get("Running") and health(bound_port())),
            "container_id": inspect("container", CONTAINER, ".Id") if state else None,
            "image_id": config["image"] if config else None,
            "volume": VOLUME, "credentials_file": str(directory / "client.json")}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("action", choices=("up", "status", "down"))
    parser.add_argument("--image")
    args = parser.parse_args()
    directory = args.root / "build" / "local-sync"
    try:
        if directory.is_symlink():
            raise ServiceError("Private configuration must not use symbolic links.")
        directory.mkdir(parents=True, exist_ok=True, mode=0o700)
        os.chmod(str(directory), 0o700)
        lock_path = directory / "lock"
        if lock_path.is_symlink():
            raise ServiceError("Private configuration must not use symbolic links.")
        with open(lock_path, "a") as lock:
            os.chmod(str(lock_path), 0o600)
            try:
                fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            except BlockingIOError:
                raise ServiceError("Another local sync operation holds the service lock.")
            print(json.dumps(operate(directory, args.action, args.image)))
    except ServiceError as error:
        print(str(error), file=sys.stderr)
        return 1
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError):
        print("Local sync operation failed; private diagnostics were suppressed.", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
