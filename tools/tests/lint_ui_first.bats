#!/usr/bin/env bats
# mg_lint_ui_first's CI hand-off: on a GitHub runner without Docker, a job
# that names the hosted check linting the same commit skips lint-ui and says
# so in its receipt; everywhere else lint-ui still runs first.
bats_require_minimum_version 1.5.0

REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"

setup_file() {
  local t="$BATS_FILE_TMPDIR/template"
  mkdir -p "$t"
  cp -R "$REPO_ROOT/tools" "$t/tools"
  cp -R "$REPO_ROOT/docker" "$t/docker"
  cp "$REPO_ROOT/.gitignore" "$t/"
  cat >"$t/tools/mg.d/zz-lint-first-fixture.sh" <<'EOF'
mg_job fx-lint-first host mg_fx_lint_first "fixture: a host job that lints first"
mg_fx_lint_first() {
  mg_lint_ui_first || return 0
  mg_message "fx-lint-first: built"
  mg_status pass
}
EOF
  git -C "$t" init -q
  git -C "$t" add -A
  git -C "$t" -c user.name=selftest -c user.email=selftest@localhost commit -qm template
}

setup() {
  SB="$BATS_TEST_TMPDIR/repo"
  cp -R "$BATS_FILE_TMPDIR/template" "$SB"
  # A PATH with no docker on it, and one with a docker that cannot reach a
  # daemon.
  NODOCKER="$BATS_TEST_TMPDIR/nodocker"
  WITHDOCKER="$BATS_TEST_TMPDIR/withdocker"
  mkdir -p "$NODOCKER" "$WITHDOCKER"
  local dir tool
  for tool in bash env jq git sed awk grep cat date mkdir rm mv cp ln ls tr cut head tail sort uniq wc dirname basename pwd \
              printf id uname mktemp touch chmod find xargs tee sleep python3 perl stat readlink realpath flock shasum sha256sum; do
    dir="$(command -v "$tool" 2>/dev/null)" || continue
    ln -sf "$dir" "$NODOCKER/$tool"
  done
  printf '#!/bin/bash\nexit 1\n' >"$WITHDOCKER/docker"
  chmod +x "$WITHDOCKER/docker"
}

fx() {
  run --separate-stderr env -u MEGURI_CI_LINT_UI_ELSEWHERE -u GITHUB_ACTIONS "$@" "$SB/tools/mg" fx-lint-first
  result="${lines[${#lines[@]} - 1]:-}"
}

@test "a Docker-less GitHub runner that names the linting check skips lint-ui and records the hand-off" {
  fx PATH="$NODOCKER" GITHUB_ACTIONS=true "MEGURI_CI_LINT_UI_ELSEWHERE=Linux · checks"
  [ "$status" -eq 0 ]
  jq -e '.status == "pass" and .details.lint_ui == {ran: false, left_to: "Linux · checks", reason: "no Docker on this hosted runner"}' "$result"
  [[ "$stderr" == *"leaves lint-ui to Linux · checks"* ]]
}

@test "lint-ui still runs first without GitHub Actions, without the named check, or with Docker present" {
  local env_args
  for env_args in "PATH=$NODOCKER MEGURI_CI_LINT_UI_ELSEWHERE=Linux·checks" \
                  "PATH=$NODOCKER GITHUB_ACTIONS=true" \
                  "PATH=$NODOCKER GITHUB_ACTIONS=false MEGURI_CI_LINT_UI_ELSEWHERE=Linux·checks" \
                  "PATH=$WITHDOCKER:$NODOCKER GITHUB_ACTIONS=true MEGURI_CI_LINT_UI_ELSEWHERE=Linux·checks"; do
    # shellcheck disable=SC2086 # each word is one VAR=value
    fx $env_args
    [ "$status" -ne 0 ]
    jq -e '.status == "error" and (.details | has("lint_ui") | not) and any(.failures[]; .id == "lint-ui")' "$result"
    [[ "$stderr" == *"fx-lint-first runs lint-ui first"* ]]
  done
}
