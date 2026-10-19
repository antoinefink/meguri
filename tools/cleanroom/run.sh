#!/bin/bash
# No real checkout, caches, history, Docker socket or host home is mounted.
set -euo pipefail
export PATH="/opt/codex/bin:/opt/codex/codex-path:/usr/local/cargo/bin:$PATH"
export CODEX_HOME=/codex-run/codex-home GIT_DIR=/git GIT_WORK_TREE=/work
python3 /runner/probe.py
[ ! -d /work/.git ] || rmdir /work/.git
[ "$1" != --check ] || exit 0
git init -q
git config user.email codex@container
git config user.name Codex
git add -f -A
git commit -qm 'Permitted source baseline; no upstream history'
baseline="$(git rev-parse HEAD)"
codex exec --strict-config -m gpt-6.1-sol -c "model_reasoning_effort=\"$2\"" \
  --skip-git-repo-check -C /work -o /codex-run/codex.out - \
  </codex-run/prompt.md >/codex-run/codex.log 2>&1
[ ! -d /work/.git ] || rmdir /work/.git
python3 /runner/source_view.py validate /work
git add -f -A
git diff --cached --binary --no-ext-diff --no-textconv "$baseline" > /codex-run/changes.patch
