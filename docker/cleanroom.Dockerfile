# The implementation runner sees a filtered source copy, never this build context.
FROM debian:trixie-slim@sha256:a29215f6a35e51e22adffa17f89e9d2ef06214e64a2bad10d765c46aea49f11f
ARG CODEX_VERSION=0.163.0
ARG CODEX_SHA256=dc6e7c4d6de71c23a0f60d05e2577ff7eef615b76f22630d58ad384b84763522
RUN apt-get update \
 && apt-get install -y --no-install-recommends bubblewrap ca-certificates git python3 \
 && rm -rf /var/lib/apt/lists/*
RUN python3 - <<'PY'
import base64
import hashlib
import io
import json
import os
from pathlib import Path
import shutil
import tarfile
import urllib.request

version = os.environ['CODEX_VERSION'] + '-linux-arm64'
metadata = json.load(urllib.request.urlopen('https://registry.npmjs.org/@openai/codex/' + version))
assert metadata['name'] == '@openai/codex' and metadata['version'] == version
archive = urllib.request.urlopen(metadata['dist']['tarball']).read()
integrity = 'sha512-' + base64.b64encode(hashlib.sha512(archive).digest()).decode()
assert integrity == metadata['dist']['integrity'], 'Codex package integrity mismatch'
assert hashlib.sha256(archive).hexdigest() == os.environ['CODEX_SHA256'], 'Codex package pin changed'
print('Codex package SHA256:', hashlib.sha256(archive).hexdigest(), flush=True)
with tarfile.open(fileobj=io.BytesIO(archive), mode='r:gz') as package:
    assert all(not member.issym() and not member.islnk() for member in package.getmembers())
    package.extractall('/opt/codex-package', filter='data')
binaries = list(Path('/opt/codex-package').rglob('bin/codex'))
assert len(binaries) == 1, 'ambiguous Codex package layout'
shutil.move(str(binaries[0].parent.parent), '/opt/codex')
shutil.rmtree('/opt/codex-package')
PY
RUN /opt/codex/bin/codex --version
