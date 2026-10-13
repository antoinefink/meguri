#!/bin/bash
# Host self-tests for tools/mg, run by `tools/mg selftest-host`; prints TAP.
# They cover what the container suite cannot: macOS /bin/bash 3.2 and BSD
# tools, the real doctor on this Mac, a docker job started (and killed) from
# the host, cargo's lock shared between dev containers, and a dev image
# rebuild that reuses every layer.
#
# usage: mg_host_selftest.sh <sandbox dir>
# The sandbox is a throwaway copy of tools/ and docker/ in its own git repo
# and its own compose project; its image tags, volume and network are removed
# at exit.

set -u

REPO="$(cd "$(dirname "$0")/../.." && pwd -P)"
mkdir -p "$1"
SB="$(cd "$1" && pwd -P)"
MG="$SB/tools/mg"
RESULTS=""
# Same naming rule as mg_compose_project in tools/mg; t_dev_image checks they agree.
PROJECT="meguri-$(basename "$SB" | tr 'A-Z' 'a-z' | tr -c 'a-z0-9\n' '-')-$(printf '%s' "$SB" | cksum | cut -d' ' -f1)"

setup_sandbox() {
  mkdir -p "$SB/core"
  cp -R "$REPO/tools" "$SB/tools"
  cp -R "$REPO/docker" "$SB/docker"
  cp "$REPO/core/rust-toolchain.toml" "$SB/core/"
  cp "$REPO/.gitignore" "$SB/"
  git -C "$SB" init -q &&
    git -C "$SB" add -A &&
    git -C "$SB" -c user.name=selftest -c user.email=selftest@localhost commit -qm sandbox
}

# Compose for the sandbox project, the way mg_compose drives it.
sbcompose() {
  BUILDX_NO_DEFAULT_ATTESTATIONS=1 docker compose -p "$PROJECT" -f "$SB/docker/compose.yml" "$@"
}

project_containers() { # running containers of the sandbox project
  docker ps -q --filter "label=com.docker.compose.project=$PROJECT"
}

remove_project_containers() {
  local c
  for c in $(docker ps -aq --filter "label=com.docker.compose.project=$PROJECT"); do
    docker rm -f "$c" >/dev/null 2>&1
  done
}

cleanup() {
  remove_project_containers
  sbcompose down >/dev/null 2>&1
  docker image rm -f "$PROJECT-dev" "$PROJECT-fuzz" >/dev/null 2>&1
  docker volume rm -f "${PROJECT}_cargo-target" >/dev/null 2>&1
}
trap cleanup EXIT

# mg <args>: run the sandbox dispatcher with macOS /bin/bash. Sets RC, OUT
# (stdout), RESULT (last stdout line) and records RESULT for schema validation.
mg() {
  OUT="$(/bin/bash "$MG" "$@" 2>>"$SB/stderr.log")"
  RC=$?
  RESULT="$(printf '%s\n' "$OUT" | tail -n 1)"
  [ -f "$RESULT" ] && RESULTS="$RESULTS ${RESULT#"$SB"/}"
  return 0
}

field() { jq -r "$@" "$RESULT"; }

expect() { # expect <description> <actual> <expected>
  if [ "$2" != "$3" ]; then
    echo "$1: got '$2', want '$3'"
    return 1
  fi
}

t_unknown_job() {
  mg nosuchjob
  expect "exit code" "$RC" 2 &&
    expect "stdout lines" "$(printf '%s\n' "$OUT" | wc -l | tr -d ' ')" 1 &&
    expect "status" "$(field .status)" error &&
    expect "known jobs include doctor" "$(field '.details.known_jobs | index("doctor") != null')" true
}

t_dry_run_every_job() {
  local listing name placement _desc n=0
  listing="$(/bin/bash "$MG" --list)" || return 1
  while IFS="$(printf '\t')" read -r name placement _desc; do
    mg "$name" --dry-run
    expect "$name --dry-run exit" "$RC" 0 &&
      expect "$name dry_run" "$(field .dry_run)" true &&
      expect "$name placement" "$(field .details.job.placement)" "$placement" || return 1
    n=$((n + 1))
  done <<EOF
$listing
EOF
  [ "$n" -ge 5 ] || { echo "only $n jobs listed"; return 1; }
}

t_doctor_on_this_mac() {
  mg doctor
  jq -c '{status, message, missing: [.user_steps[] | select(.state == "missing") | .id]}' "$RESULT"
  expect "exit code" "$RC" 0 &&
    case "$(field .status)" in pass | warn) true ;; *) echo "status $(field .status)"; false ;; esac &&
    expect "failed checks" "$(field '[.details.checks[] | select(.status == "fail")] | length')" 0 &&
    expect "xcode build recorded" "$(field '.details.environment.xcode_build | type')" string &&
    expect "user_steps listed" "$(field '.user_steps | length > 0')" true
}

t_core_test_runs_in_dev_container() {
  mg core-test
  expect "exit code" "$RC" 0 &&
    expect "status" "$(field .status)" pass &&
    expect "placement" "$(field .placement)" docker &&
    expect "service" "$(field .service)" dev &&
    expect "container" "$(field .details.environment.container)" dev &&
    expect "os" "$(field '.details.environment.os | startswith("Linux")')" true &&
    expect "total tests" "$(field .summary.total)" 0 &&
    expect "latest" "$(readlink "$SB/build/mg/core-test/latest")" "$(field .run_id)"
}

# An orchestrator timeout sends SIGTERM to the process group of tools/mg. The
# docker job's container must stop with it instead of running to completion.
t_killing_mg_stops_the_docker_job_container() {
  local fixture="$SB/tools/mg.d/zz-sleep.sh" pid
  printf '%s\n' 'mg_job fx-sleep docker:dev fx_sleep "fixture: sleep in the dev container"' \
    'fx_sleep() { echo "fx-sleep started"; sleep 120; mg_status pass; }' >"$fixture"
  set -m # the run gets its own process group
  /bin/bash "$MG" fx-sleep >/dev/null 2>>"$SB/stderr.log" &
  pid=$!
  set +m
  for _ in $(seq 120); do
    grep -qs "fx-sleep started" "$SB"/build/mg/fx-sleep/*/log.txt && break
    sleep 1
  done
  rm -f "$fixture"
  if [ -z "$(project_containers)" ]; then
    echo "the fx-sleep container did not start"
    kill -TERM -- "-$pid"
    return 1
  fi
  kill -TERM -- "-$pid"
  for _ in $(seq 20); do
    [ -z "$(project_containers)" ] && return 0
    sleep 1
  done
  echo "the fx-sleep container still runs 20 s after SIGTERM:"
  docker ps --filter "label=com.docker.compose.project=$PROJECT" --format '{{.Names}} {{.Status}}'
  remove_project_containers
  return 1
}

# Cargo guards downloads into the shared registry with lock files in
# CARGO_HOME. Every dev container must see the same lock: cargo in one
# container waits while another container holds it.
t_dev_containers_share_the_cargo_package_cache_lock() {
  local holder="$SB/lock-holder.out" waiter="$SB/lock-waiter.out" pid rc
  # shellcheck disable=SC2016 # $CARGO_HOME expands in the container
  sbcompose run --rm -T --no-deps dev \
    sh -c 'flock "$CARGO_HOME/.package-cache" sh -c "echo locked; sleep 20"' >"$holder" 2>&1 </dev/null &
  pid=$!
  for _ in $(seq 60); do
    grep -qx locked "$holder" && break
    sleep 1
  done
  if ! grep -qx locked "$holder"; then
    echo "the lock holder did not start:"
    cat "$holder"
    remove_project_containers
    return 1
  fi
  sbcompose run --rm -T --no-deps dev sh -c '
    mkdir -p /tmp/probe/src && : >/tmp/probe/src/lib.rs &&
    printf "[package]\nname = \"probe\"\nversion = \"0.0.0\"\nedition = \"2024\"\n" >/tmp/probe/Cargo.toml &&
    timeout 5 cargo metadata --offline --format-version 1 --manifest-path /tmp/probe/Cargo.toml >/dev/null' \
    >"$waiter" 2>&1 </dev/null
  rc=$?
  remove_project_containers
  wait "$pid" 2>/dev/null
  cat "$waiter"
  expect "cargo metadata exit while another container holds the lock" "$rc" 124 &&
    grep -q "Blocking waiting for file lock on package cache" "$waiter"
}

# Without an init process, bash runs as PID 1 and the kernel drops SIGTERM.
t_every_compose_service_runs_an_init_process() {
  local cfg
  cfg="$(sbcompose --profile '*' config --format json)" || return 1
  expect "dev service defined" "$(jq '.services | has("dev")' <<<"$cfg")" true &&
    expect "services without init: true" \
      "$(jq -r '[.services | to_entries[] | select(.value.init != true) | .key] | join(" ")' <<<"$cfg")" ""
}

t_dev_image_rebuild_reuses_layers() {
  mg dev-image
  expect "first build exit" "$RC" 0 || return 1
  local first="$RESULT"
  expect "dev image tag" "$(field .details.images.dev.tag)" "$PROJECT-dev" || return 1
  mg dev-image
  expect "second build exit" "$RC" 0 &&
    expect "dev image id" "$(field .details.images.dev.id)" "$(jq -r .details.images.dev.id "$first")" &&
    expect "fuzz image id" "$(field .details.images.fuzz.id)" "$(jq -r .details.images.fuzz.id "$first")" &&
    expect "dev steps executed" "$(field .details.images.dev.executed)" 0 &&
    expect "fuzz steps executed" "$(field .details.images.fuzz.executed)" 0 &&
    expect "dev steps cached" "$(field '.details.images.dev.cached > 0')" true
}

t_results_validate_against_schema() {
  [ -n "$RESULTS" ] || { echo "no results collected"; return 1; }
  # shellcheck disable=SC2086 # RESULTS is a space-separated list of relative paths
  sbcompose run --rm -T --no-deps dev python3 tools/tests/validate_result.py $RESULTS </dev/null
}

TESTS="t_unknown_job t_dry_run_every_job t_doctor_on_this_mac t_core_test_runs_in_dev_container
t_killing_mg_stops_the_docker_job_container t_dev_containers_share_the_cargo_package_cache_lock
t_every_compose_service_runs_an_init_process t_dev_image_rebuild_reuses_layers
t_results_validate_against_schema"

echo "1..$(echo "$TESTS" | wc -w | tr -d ' ')"
if ! setup_sandbox >"$SB/setup.log" 2>&1; then
  echo "Bail out! could not create the sandbox in $SB (see setup.log)"
  exit 1
fi
n=0
failed=0
for t in $TESTS; do
  n=$((n + 1))
  name="$(printf '%s' "${t#t_}" | tr '_' ' ')"
  if "$t" >"$SB/diag.txt" 2>&1; then
    echo "ok $n $name"
  else
    failed=$((failed + 1))
    echo "not ok $n $name"
    sed 's/^/# /' "$SB/diag.txt"
  fi
done
[ "$failed" -eq 0 ]
