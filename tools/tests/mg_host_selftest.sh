#!/bin/bash
# Host self-tests for tools/mg, run by `tools/mg selftest-host`; prints TAP.
# They cover what the container suite cannot: macOS /bin/bash 3.2 and BSD
# tools, the real doctor on this Mac, a docker job started (and killed) from
# the host, cargo's lock shared between dev containers, a dev image rebuild
# that reuses every layer, the simulator pool's locks (with a fake simctl) and
# host jobs whose container tests run only under bash 5 (fixtures-check,
# bench-mac).
#
# usage: mg_host_selftest.sh <sandbox dir>
# The sandbox is a throwaway copy of tools/ and docker/ in its own git repo
# and its own compose project; its image tags, volume and network are removed
# at exit.

set -u

# Similarity checks must use the sandbox's synthetic corpus.
unset MG_SIMILARITY_REFS

REPO="$(cd "$(dirname "$0")/../.." && pwd -P)"
mkdir -p "$1"
SB="$(cd "$1" && pwd -P)"
MG="$SB/tools/mg"
RESULTS=""
RIG_IMAGE="" # the stand-in oracle image t_sync_rig_* builds
# Same naming rule as mg_compose_project in tools/mg; t_dev_image checks they agree.
PROJECT="meguri-$(basename "$SB" | tr 'A-Z' 'a-z' | tr -c 'a-z0-9\n' '-')-$(printf '%s' "$SB" | cksum | cut -d' ' -f1)"

setup_sandbox() {
  mkdir -p "$SB/core" "$SB/docs/process" "$SB/oracle/driver"
  cp -R "$REPO/tools" "$SB/tools"
  cp -R "$REPO/docker" "$SB/docker"
  cp -R "$REPO/.claude" "$SB/.claude"
  cp -R "$REPO/docs/process/roles" "$REPO/docs/process/provenance" "$SB/docs/process/"
  cp "$REPO/core/rust-toolchain.toml" "$SB/core/"
  cp "$REPO/.gitignore" "$REPO/.gitattributes" "$SB/"
  echo "stand-in for the AGPL driver" >"$SB/oracle/driver/run.py"
  git -C "$SB" init -q &&
    git -C "$SB" config user.name selftest &&
    git -C "$SB" config user.email selftest@localhost &&
    git -C "$SB" add -A &&
    git -C "$SB" commit -qm sandbox
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
  sbcompose --profile sync-rig --profile spec-record --profile oracle-sync --profile oracle-record down >/dev/null 2>&1
  [ -z "$RIG_IMAGE" ] || docker image rm -f "$RIG_IMAGE" >/dev/null 2>&1
  docker image rm -f "$PROJECT-dev" "$PROJECT-fuzz" "$PROJECT-similarity" >/dev/null 2>&1
  rm -rf "$SB-wt"
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

. "$REPO/tools/tests/doctor_result.sh"

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
  assert_doctor_result "$RESULT" "$RC"
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

# An orchestrator timeout sends SIGTERM to the process group of tools/mg;
# Ctrl-C sends it SIGINT. Either way the docker job's container must be
# stopped and removed instead of running to completion, and the run must
# still end with an error result saying it was cancelled.
signal_docker_job() { # <signal name without SIG>
  local sig="$1" fixture="$SB/tools/mg.d/zz-sleep.sh" out="$SB/fx-sleep-$1.out" err="$SB/fx-sleep-$1.err"
  local pid rc=0 gone=false
  printf '%s\n' 'mg_job fx-sleep docker:dev fx_sleep "fixture: sleep in the dev container"' \
    'fx_sleep() { echo "fx-sleep started"; sleep 120; mg_status pass; }' >"$fixture"
  # Under `tools/mg selftest-host` this script is a background job, and macOS
  # bash 3.2 gives background jobs an ignored SIGINT that their children
  # inherit and cannot trap. A terminal starts tools/mg with SIGINT at its
  # default, so do that.
  set -m # the run gets its own process group
  /usr/bin/perl -e '$SIG{INT} = "DEFAULT"; exec @ARGV or die "exec: $!"' \
    /bin/bash "$MG" fx-sleep >"$out" 2>"$err" &
  pid=$!
  set +m
  for _ in $(seq 120); do
    grep -qs "fx-sleep started" "$err" && break
    sleep 1
  done
  rm -f "$fixture"
  if [ -z "$(project_containers)" ]; then
    echo "the fx-sleep container did not start:"
    cat "$err"
    kill -TERM -- "-$pid"
    return 1
  fi
  kill -"$sig" -- "-$pid"
  for _ in $(seq 30); do
    kill -0 "$pid" 2>/dev/null || break
    sleep 1
  done
  if kill -0 "$pid" 2>/dev/null; then
    echo "tools/mg still runs 30 s after SIG$sig"
    kill -KILL -- "-$pid"
    remove_project_containers
    return 1
  fi
  wait "$pid" || rc=$?
  cat "$err" >>"$SB/stderr.log"
  for _ in $(seq 20); do
    [ -z "$(docker ps -aq --filter "label=com.docker.compose.project=$PROJECT")" ] && gone=true && break
    sleep 1
  done
  if [ "$gone" = false ]; then
    echo "the fx-sleep container is still there 20 s after SIG$sig:"
    docker ps -a --filter "label=com.docker.compose.project=$PROJECT" --format '{{.Names}} {{.Status}}'
    remove_project_containers
    return 1
  fi
  RESULT="$(tail -n 1 "$out")"
  if [ ! -f "$RESULT" ]; then
    echo "the interrupted run wrote no result.json (tools/mg exit $rc); run directories:"
    ls -la "$SB"/build/mg/fx-sleep/*/
    return 1
  fi
  RESULTS="$RESULTS ${RESULT#"$SB"/}"
  jq -c '{status, exit_code, message, failures}' "$RESULT"
  # shellcheck disable=SC2016 # $s is a jq variable
  expect "exit code" "$rc" 2 &&
    expect "status" "$(field .status)" error &&
    expect "exit_code" "$(field .exit_code)" 2 &&
    expect "placement" "$(field .placement)" docker &&
    expect "cancelled failure" \
      "$(field --arg s "SIG$sig" '[.failures[] | select(.id == "cancelled") | "\(.repro) \(.message | contains($s))"] | join(",")')" \
      "tools/mg fx-sleep true" &&
    expect "state directory" "$([ -e "$(dirname "$RESULT")/.mg" ] && echo present || echo removed)" removed
}

t_killing_mg_stops_the_docker_job_container() { signal_docker_job TERM; }

t_interrupting_mg_stops_the_docker_job_container() { signal_docker_job INT; }

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

# A host job that starts a container labels it with the run id; cancelling the
# run must remove that container too.
t_cancelling_a_host_job_removes_its_container() {
  local fixture="$SB/tools/mg.d/zz-host-sleep.sh" out="$SB/fx-host.out" err="$SB/fx-host.err" pid rc=0 gone=false
  # shellcheck disable=SC2016 # $MG_RUN_ID expands when the fixture job runs
  printf '%s\n' 'mg_job fx-host-sleep host fx_host_sleep "fixture: a host job running a container"' \
    'fx_host_sleep() { echo "fx-host-sleep started" >&2; mg_compose run --rm -T --no-deps --label "meguri.mg.run-id=$MG_RUN_ID" dev sleep 120 </dev/null; mg_status pass; }' >"$fixture"
  set -m
  /usr/bin/perl -e '$SIG{INT} = "DEFAULT"; exec @ARGV or die "exec: $!"' \
    /bin/bash "$MG" fx-host-sleep >"$out" 2>"$err" &
  pid=$!
  set +m
  for _ in $(seq 120); do
    [ -n "$(project_containers)" ] && break
    sleep 1
  done
  rm -f "$fixture"
  if [ -z "$(project_containers)" ]; then
    echo "the fx-host-sleep container did not start:"
    cat "$err"
    kill -TERM -- "-$pid"
    return 1
  fi
  kill -TERM -- "-$pid"
  for _ in $(seq 30); do
    kill -0 "$pid" 2>/dev/null || break
    sleep 1
  done
  wait "$pid" || rc=$?
  cat "$err" >>"$SB/stderr.log"
  for _ in $(seq 20); do
    [ -z "$(docker ps -aq --filter "label=com.docker.compose.project=$PROJECT")" ] && gone=true && break
    sleep 1
  done
  if [ "$gone" = false ]; then
    echo "the host job's container is still there 20 s after SIGTERM"
    remove_project_containers
    return 1
  fi
  RESULT="$(tail -n 1 "$out")"
  RESULTS="$RESULTS ${RESULT#"$SB"/}"
  expect "exit code" "$rc" 2 &&
    expect "placement" "$(field .placement)" host &&
    expect "cancelled" "$(field '[.failures[].id] | index("cancelled") != null')" true
}

# Worktree creation and the commit-msg hook under macOS /bin/bash 3.2 and
# BSD tools.
t_implementer_worktree_and_commit_hook_on_macos() {
  mg worktree new --role implementer hw1
  expect "worktree exit" "$RC" 0 || { cat "$SB/stderr.log" | tail -20; return 1; }
  local wt="$SB-wt/hw1" id
  expect "oracle/ absent" "$([ -e "$wt/oracle" ] && echo present || echo absent)" absent &&
    expect "tools present" "$([ -f "$wt/tools/mg" ] && echo yes || echo no)" yes &&
    expect "deny rule for refs/" \
      "$(jq --arg p "Read(/$SB/refs/**)" '.permissions.deny | index($p) != null' "$wt/.claude/settings.local.json")" true &&
    expect "profile hash" "$(jq -r .deny_profile.sha256 "$(git -C "$wt" rev-parse --absolute-git-dir)/meguri-session.json")" \
      "$(shasum -a 256 "$wt/.claude/settings.local.json" | cut -d' ' -f1)" || return 1
  id="$(jq -r .session "$(git -C "$wt" rev-parse --absolute-git-dir)/meguri-session.json")"
  expect "record committed on the branch" "$(git -C "$wt" log -1 --format=%s)" "Record session $id (implementer, hw1)" &&
    expect "record in the branch" "$(jq -r .session "$wt/docs/process/provenance/sessions/$id.json")" "$id" &&
    expect "main checkout's ledger untouched" "$(git -C "$SB" status --porcelain -- docs/process/provenance)" "" &&
    expect "hook installed" "$([ -x "$SB/.git/hooks/commit-msg" ] && echo yes || echo no)" yes || return 1
  mg worktree new --role "implementer fixer" hw2
  expect "compound role exit" "$RC" 2 &&
    expect "compound role failure" "$(field '.failures[0].id')" bad-role || return 1
  mg hooks install
  expect "hooks exit" "$RC" 0 || return 1
  mkdir -p "$wt/core/crates/meguri-search/src"
  echo "pub fn parse() {}" >"$wt/core/crates/meguri-search/src/lib.rs"
  git -C "$wt" add core/crates/meguri-search/src/lib.rs
  if git -C "$wt" -c user.name=t -c user.email=t@t commit -q -m "Add parser" \
    -m "Role: implementer
Spec: search/grammar#G3" 2>"$SB/hook.err"; then
    echo "a commit without Session: was accepted"
    return 1
  fi
  grep -q "Session: $id" "$SB/hook.err" || { cat "$SB/hook.err"; return 1; }
  git -C "$wt" -c user.name=t -c user.email=t@t commit -q -m "Add parser" \
    -m "Role: implementer
Spec: search/grammar#G3
Session: $id" || return 1
  git -C "$SB" worktree remove --force "$wt"
}

# Parallel session records in one checkout under macOS /bin/bash 3.2: each
# waits for the ledger lock, so all pass with one commit each; a run cancelled
# while it waits leaves the lock it never took.
t_parallel_session_records_on_macos() {
  local n=6 i head0 pids="" pid rc=0 lock="$SB/.git/meguri-session.lock"
  head0="$(git -C "$SB" rev-parse HEAD)"
  for i in 1 2 3 4 5 6; do
    /bin/bash "$MG" session record --role critic --name "h$i" >"$SB/rec.$i" 2>>"$SB/stderr.log" &
    pids="$pids $!"
  done
  for pid in $pids; do wait "$pid" || rc=$?; done
  expect "every run exits 0" "$rc" 0 || return 1
  for i in 1 2 3 4 5 6; do
    RESULT="$(tail -n 1 "$SB/rec.$i")"
    RESULTS="$RESULTS ${RESULT#"$SB"/}"
    expect "record $i" "$(field .status)" pass || { field .failures; return 1; }
  done
  expect "one commit per record" "$(git -C "$SB" rev-list --count "$head0..HEAD")" "$n" &&
    expect "ledger committed" "$(git -C "$SB" status --porcelain -- docs/process/provenance)" "" &&
    expect "lines in HEAD" "$(git -C "$SB" diff "$head0" HEAD -- docs/process/provenance | grep -c '^+{')" "$n" &&
    expect "lock released" "$([ -e "$lock" ] && echo held || echo free)" free || return 1
  mkdir "$lock"
  /bin/bash "$MG" session record --role critic >"$SB/rec.cancel" 2>>"$SB/stderr.log" &
  pid=$!
  sleep 2
  kill -TERM "$pid"
  wait "$pid"
  rc=$?
  RESULT="$(tail -n 1 "$SB/rec.cancel")"
  RESULTS="$RESULTS ${RESULT#"$SB"/}"
  expect "cancelled exit" "$rc" 2 &&
    expect "cancelled" "$(field '[.failures[].id] | index("cancelled") != null')" true &&
    expect "another holder's lock kept" "$([ -d "$lock" ] && echo held || echo free)" held &&
    expect "nothing committed" "$(git -C "$SB" rev-list --count "$head0..HEAD")" "$n" || return 1
  rmdir "$lock"
}

similarity_corpus() { # a stand-in Anki repository with one release tag
  local repo="$SB/refs/anki"
  mkdir -p "$repo/rslib/src"
  cat >"$repo/rslib/src/queue.rs" <<'RS'
pub fn rebalance(queues: &mut [Vec<u64>], max_len: usize) -> usize {
    let mut moved = 0;
    loop {
        let (mut longest, mut shortest) = (0, 0);
        for (i, q) in queues.iter().enumerate() {
            if q.len() > queues[longest].len() { longest = i; }
            if q.len() < queues[shortest].len() { shortest = i; }
        }
        if queues[longest].len() <= max_len || longest == shortest { break; }
        if let Some(item) = queues[longest].pop() {
            queues[shortest].insert(0, item);
            moved += 1;
        } else {
            break;
        }
    }
    moved
}
RS
  git -C "$repo" init -q &&
    git -C "$repo" add -A &&
    git -C "$repo" -c user.name=t -c user.email=t@t commit -qm corpus &&
    git -C "$repo" tag 2.1.51 &&
    # Pin the sandbox's scanner config to this one-tag corpus.
    sed -i '' 's/^  tags: {min: .*/  tags: {min: "2.1.51", max: "2.1.51", count: 1}/' "$SB/tools/similarity/config.yaml" &&
    grep -q 'count: 1}' "$SB/tools/similarity/config.yaml"
}

t_similarity_flags_a_copy_through_tools_mg() {
  similarity_corpus || return 1
  cp "$SB/refs/anki/rslib/src/queue.rs" "$SB/core/copied.rs"
  mg similarity -- core/copied.rs
  jq -c '{status, message, summary, failures: [.failures[].id]}' "$RESULT"
  expect "exit code" "$RC" 1 &&
    expect "status" "$(field .status)" fail &&
    expect "placement" "$(field .placement)" host &&
    expect "struct failure" "$(field '[.failures[].id] | index("similarity:core/copied.rs:struct") != null')" true &&
    expect "failures name no corpus path" "$(field '[.failures[].message | contains("rslib")] | any')" false &&
    expect "repro" "$(field '.failures[0].repro')" "tools/mg similarity -- 'core/copied.rs'" &&
    expect "corpus tags" "$(field .details.corpus.tags)" 1 &&
    expect "reviewer view names the corpus path" \
      "$(jq '[.files[].flags[]?.regions[].anki[].path] | index("rslib/src/queue.rs") != null' "$(field '.artifacts[] | select(.kind == "similarity-reviewer-json") | .path')")" true &&
    expect "implementer view has no corpus path" \
      "$(grep -c rslib "$(field '.artifacts[] | select(.kind == "similarity-implementer-json") | .path')")" 0
}

t_similarity_scans_changed_files_and_commits() {
  rm -f "$SB/core/copied.rs"
  printf 'pub fn total(xs: &[u32]) -> u64 {\n    xs.iter().map(|&x| u64::from(x)).sum()\n}\n' >"$SB/core/own.rs"
  # A name git quotes unless asked for NUL-separated output.
  local other
  other="$(printf 'core/r\303\251sum\303\251.rs')"
  printf 'pub fn half(x: u32) -> u32 {\n    x / 2\n}\n' >"$SB/$other"
  mg similarity --base HEAD
  expect "changed-files exit" "$RC" 0 &&
    expect "changed-files status" "$(field .status)" pass &&
    expect "changed files scanned" "$(field .summary.total)" 2 || return 1
  git -C "$SB" add core/own.rs "$other" &&
    git -C "$SB" -c user.name=t -c user.email=t@t -c core.hooksPath=/dev/null commit -qm own
  rm -f "$SB/core/own.rs" "$SB/$other"
  mg similarity --rev HEAD --base HEAD~1
  expect "rev exit" "$RC" 0 &&
    expect "rev files scanned" "$(field .summary.total)" 2 &&
    expect "index reused" "$(field .details.corpus.built)" false &&
    expect "rev tree removed" "$([ -e "$(dirname "$RESULT")/tree" ] && echo present || echo absent)" absent || return 1
  local before
  before="$(git -C "$SB" status --porcelain)"
  mg similarity --rev HEAD -- core/own.rs core/not-there.rs tools/similarity/meguri_similarity/scan.py
  expect "rev paths exit" "$RC" 0 &&
    expect "rev paths status" "$(field .status)" warn &&
    expect "rev paths missing" "$(field '[.failures[].id] | index("similarity:missing") != null')" true &&
    expect "rev paths scanned" "$(field .summary.total)" 2 || return 1
  mg similarity --rev HEAD
  expect "full rev exit" "$RC" 0 &&
    expect "full rev scans the scanner too" "$(field '.summary.total > 10')" true &&
    expect "checkout untouched" "$(git -C "$SB" status --porcelain)" "$before" &&
    expect "no oracle/ copy left" "$(find "$SB/build/mg/similarity" -path '*/oracle/*' | wc -l | tr -d ' ')" 0
}

t_similarity_nightly_runs_jplag() {
  # This synthetic corpus has one release; every selected reference is required.
  sed -i '' 's/^  tags: \[.*/  tags: [latest]/' "$SB/tools/similarity/config.yaml" || return 1
  grep -q '^  tags: \[latest\]$' "$SB/tools/similarity/config.yaml" || return 1
  cp "$SB/refs/anki/rslib/src/queue.rs" "$SB/core/copied.rs"
  mg similarity-nightly
  rm -f "$SB/core/copied.rs"
  jq -c '{status, message}' "$RESULT"
  expect "exit code" "$RC" 0 &&
    expect "status" "$(field .status)" warn &&
    expect "tags" "$(field '.details.jplag.tags | join(" ")')" 2.1.51 &&
    expect "rust max similarity" "$(field '.details.jplag.languages.rust.max_similarity > 0.9')" true &&
    expect "typescript skipped" "$(field '.details.jplag.languages.typescript.skipped')" "no Meguri files in this language" &&
    expect "report" "$([ -f "$(field '.artifacts[] | select(.kind == "jplag-report") | .path')" ] && echo yes || echo no)" yes
}

# The sandbox has no oracle/, so sync-rig builds a stand-in oracle image: the
# dev image plus a stand-in for Anki's sync server that answers a GET with 404,
# echoes a POST's body back, and closes the connection without a reply to
# POST /abort. Everything else is the real rig: compose, networks, toxiproxy,
# mitmdump, sync-edge.
standin_oracle_tree() {
  local o="$SB/oracle"
  mkdir -p "$o/driver/anki"
  printf 'FROM %s-dev\nRUN ln -sf /usr/bin/python3 /usr/local/bin/python\nCOPY driver /opt/driver\nENV PYTHONPATH=/opt/driver\n' \
    "$PROJECT" >"$o/Dockerfile"
  printf '*\n!driver\n' >"$o/Dockerfile.dockerignore"
  printf 'anki==0.0.0.selftest\n' >"$o/requirements.lock"
  : >"$o/entrypoint.sh"
  : >"$o/driver/anki/__init__.py"
  # The project name makes the image tag this sandbox's own.
  printf '%s\n' "$PROJECT" >"$o/driver/sandbox.txt"
  cat >"$o/driver/anki/syncserver.py" <<'PY'
import http.server
import os


class Handler(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        self.send_response(404)
        self.end_headers()

    def do_POST(self):
        body = self.rfile.read(int(self.headers.get("content-length") or 0))
        if self.path == "/abort":
            self.close_connection = True
            return
        self.send_response(200)
        self.send_header("content-type", "application/octet-stream")
        self.send_header("content-length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


http.server.ThreadingHTTPServer((os.environ["SYNC_HOST"], int(os.environ["SYNC_PORT"])), Handler).serve_forever()
PY
}

# tools/mg sync-rig up: only sync-edge publishes, every port on 127.0.0.1;
# nothing answers on the Mac's LAN address; the server and toxiproxy are only
# on internal networks; the users are those the server was started with
# (docker/syncserver.env overrides one). down removes the rig.
t_sync_rig_publishes_on_loopback_only_and_down_removes_it() {
  local ps nets lan
  docker image inspect "$PROJECT-dev" >/dev/null 2>&1 || mg dev-image
  standin_oracle_tree
  printf 'SYNC_USER1=alice:pw\n' >"$SB/docker/syncserver.env"
  mg sync-rig up
  RIG_IMAGE="$(field -r '.details.image.tag // empty')"
  jq -c '{status, message, failures: [.failures[].id], probes: .details.probes}' "$RESULT"
  expect "up exit" "$RC" 0 &&
    expect "up status" "$(field .status)" pass &&
    expect "endpoints on 127.0.0.1" \
      "$(field '[.details.endpoints[] | select(. != null) | startswith("http://127.0.0.1:")] | (length == 3 and all)')" true &&
    expect "users" "$(field -c .details.users)" '["alice","meguri-b","meguri-c","meguri-d"]' || return 1
  lan="$(field -r '.details.probes.lan_address // empty')"
  if [ -n "$lan" ]; then
    expect "every endpoint refused on the LAN address $lan" \
      "$(field '[.details.probes.endpoints[].lan_curl_exit | IN(7, 28)] | (length == 3 and all)')" true || return 1
  fi
  ps="$(sbcompose --profile sync-rig ps --format json)" || return 1
  expect "published ports (service host:port)" \
    "$(jq -rs '[.[] | (if type == "array" then .[] else . end) | .Service as $s | (.Publishers // [])[]
      | select(.PublishedPort > 0) | "\($s) \(.URL)"] | unique | join(",")' <<<"$ps")" "sync-edge 127.0.0.1" || return 1
  nets="$(for c in $(sbcompose --profile sync-rig ps -q syncserver toxiproxy); do
    docker inspect --format '{{range $n, $_ := .NetworkSettings.Networks}}{{println $n}}{{end}}' "$c"
  done | sort -u | grep . | xargs docker network inspect --format '{{.Name}} {{.Internal}}')"
  expect "networks of syncserver and toxiproxy" "$nets" "${PROJECT}_sync-rig true" || return 1
  mg sync-rig down
  expect "down exit" "$RC" 0 &&
    expect "down status" "$(field .status)" pass &&
    expect "rig containers left" "$(sbcompose --profile sync-rig --profile spec-record ps -aq | wc -l | tr -d ' ')" 0 &&
    expect "rig networks left" "$(docker network ls -q --filter "name=^${PROJECT}_sync-rig" | wc -l | tr -d ' ')" 0
}

# A proxy in the environment never answers the rig's probes. With http_proxy
# and the other proxy variables pointing at a loopback proxy that answers every
# request with 200 and a Date of 2001, status still gets the stand-in server's
# 404 and Date, finds nothing on the LAN address, and leaves the rig running.
t_sync_rig_probes_ignore_proxy_variables() {
  local pid port lan v
  mg sync-rig up
  expect "up status" "$(field .status)" pass || return 1
  : >"$SB/proxy.port"
  /usr/bin/perl -MIO::Socket::INET -e '
    my $s = IO::Socket::INET->new(LocalAddr => "127.0.0.1", LocalPort => 0, Listen => 16, ReuseAddr => 1) or die "listen: $!";
    print $s->sockport, "\n"; STDOUT->flush;
    while (my $c = $s->accept) {
      while (defined(my $l = <$c>)) { last if $l eq "\r\n" }
      print $c "HTTP/1.1 200 OK\r\nDate: Mon, 01 Jan 2001 00:00:00 GMT\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
      close $c;
    }' >"$SB/proxy.port" 2>>"$SB/stderr.log" &
  pid=$!
  for _ in $(seq 50); do
    [ -s "$SB/proxy.port" ] && break
    sleep 0.1
  done
  port="$(cat "$SB/proxy.port")"
  [ -n "$port" ] || {
    echo "the stand-in proxy did not start"
    kill "$pid"
    wait "$pid" 2>/dev/null
    return 1
  }
  for v in http_proxy HTTP_PROXY https_proxy HTTPS_PROXY all_proxy ALL_PROXY; do
    export "$v=http://127.0.0.1:$port"
  done
  mg sync-rig status
  unset http_proxy HTTP_PROXY https_proxy HTTPS_PROXY all_proxy ALL_PROXY
  kill "$pid" 2>/dev/null
  wait "$pid" 2>/dev/null
  jq -c '{status, message, failures: [.failures[].id], probes: .details.probes, clock: .details.clock}' "$RESULT"
  expect "status exit" "$RC" 0 &&
    expect "status" "$(field .status)" pass &&
    expect "the server, not the proxy, answered" "$(field .details.probes.endpoints.syncserver.loopback_http)" 404 &&
    expect "the server's clock, not the proxy's" "$(field '.details.clock.skew_secs | (if . < 0 then -. else . end) <= 300')" true || return 1
  lan="$(field -r '.details.probes.lan_address // empty')"
  if [ -n "$lan" ]; then
    expect "every endpoint refused on the LAN address $lan" \
      "$(field '[.details.probes.endpoints[].lan_curl_exit | IN(7, 28)] | (length == 3 and all)')" true || return 1
  fi
  expect "rig still running" "$(sbcompose --profile sync-rig ps -q --status running syncserver sync-edge | wc -l | tr -d ' ')" 2 || return 1
  mg sync-rig down
  expect "down status" "$(field .status)" pass
}

# tools/mg sync-rig up --record: the recorder writes one decoded line per
# exchange (a zstd JSON body, the anki-sync header parsed, the reply decoded)
# and a line without a response for a flow that fails, but none for the job's
# own probes. A line it cannot write makes status fail.
t_sync_rig_records_decoded_exchanges() {
  local rec="$SB/conformance/recordings/sync/selftest" url code
  mg sync-rig up --record selftest
  jq -c '{status, message, failures: [.failures[].id], recorder: .details.recorder}' "$RESULT"
  expect "up exit" "$RC" 0 &&
    expect "recording" "$(field -r .details.recording)" conformance/recordings/sync/selftest/exchanges.jsonl &&
    expect "recorder health" "$(field -c '.details.recorder | {ok, written, errors}')" '{"ok":true,"written":0,"errors":0}' || return 1
  url="$(field -r .details.endpoints.recorder)"
  sbcompose --profile spec-record exec -T mitmdump python3 -c \
    'import sys, zstandard; sys.stdout.buffer.write(zstandard.ZstdCompressor().compress(b"{\"v\":11}"))' \
    >"$SB/meta.zst" </dev/null || return 1
  code="$(curl --noproxy '*' -s -o "$SB/meta.reply" -w '%{http_code}' --max-time 10 \
    -H 'content-type: application/octet-stream' -H 'anki-sync: {"v":11,"k":"","c":"selftest","s":"s1"}' \
    --data-binary @"$SB/meta.zst" "${url}sync/meta")"
  expect "reply through the recorder" "$code" 200 &&
    expect "reply body" "$(cmp -s "$SB/meta.zst" "$SB/meta.reply" && echo echoed)" echoed &&
    expect "lines after one exchange" "$(wc -l <"$rec/exchanges.jsonl" | tr -d ' ')" 1 &&
    expect "exchange" "$(jq -cS '{seq, method, path, error,
        request: {sync: .request.headers["anki-sync"], body: .request.body},
        response: {status: .response.status, body: .response.body}}' "$rec/exchanges.jsonl")" \
      '{"error":null,"method":"POST","path":"/sync/meta","request":{"body":{"json":{"v":11},"zstd":true},"sync":{"c":"selftest","k":"","s":"s1","v":11}},"response":{"body":{"json":{"v":11},"zstd":true},"status":200},"seq":1}' ||
    return 1
  code="$(curl --noproxy '*' -s -o /dev/null -w '%{http_code}' --max-time 10 --data-binary x "${url}abort")"
  expect "reply to the aborted exchange" "$code" 502 &&
    expect "lines after the aborted exchange" "$(wc -l <"$rec/exchanges.jsonl" | tr -d ' ')" 2 &&
    expect "aborted exchange" "$(tail -n 1 "$rec/exchanges.jsonl" | jq -c '{seq, response, error: (.error | type)}')" \
      '{"seq":2,"response":null,"error":"string"}' || return 1
  mg sync-rig status
  expect "status" "$(field .status)" pass &&
    expect "recorder health" "$(field -c '.details.recorder | {ok, written, errors}')" '{"ok":true,"written":2,"errors":0}' &&
    expect "lines after status" "$(wc -l <"$rec/exchanges.jsonl" | tr -d ' ')" 2 || return 1
  # The recording becomes a directory, so the next exchange cannot be written.
  # The swap runs in the container: one made on the Mac reaches the container's
  # view of the bind mount up to a few hundred ms later, and an exchange in
  # between still lands in the renamed file.
  sbcompose --profile spec-record exec -T mitmdump sh -c \
    'mv /recordings/exchanges.jsonl /recordings/exchanges.done && mkdir /recordings/exchanges.jsonl' </dev/null ||
    return 1
  code="$(curl --noproxy '*' -s -o /dev/null -w '%{http_code}' --max-time 10 --data-binary @"$SB/meta.zst" "${url}sync/meta")"
  mg sync-rig status
  jq -c '{status, failures, recorder: .details.recorder}' "$RESULT"
  expect "reply while the recording is unwritable" "$code" 200 &&
    expect "lines in the renamed recording" "$(wc -l <"$rec/exchanges.done" | tr -d ' ')" 2 &&
    expect "status exit" "$RC" 1 &&
    expect "status" "$(field .status)" fail &&
    expect "recorder failure" "$(field '[.failures[].id] | index("recorder-unhealthy") != null')" true &&
    expect "recorder health" "$(field -c '.details.recorder | {ok, written, errors}')" '{"ok":false,"written":2,"errors":1}' || return 1
  mg sync-rig down
  expect "down status" "$(field .status)" pass
}

# A recorder that cannot write its recording fails up, and status afterwards,
# instead of proxying without recording. Here the recording is mounted
# read-only.
t_sync_rig_up_fails_when_the_recording_is_unwritable() {
  local compose="$SB/docker/compose.yml"
  cp "$compose" "$SB/compose.yml.orig" || return 1
  sed -i '' 's|:/recordings$|:/recordings:ro|' "$compose"
  if ! grep -q ':/recordings:ro$' "$compose"; then
    cp "$SB/compose.yml.orig" "$compose"
    echo "compose.yml has no recordings mount to make read-only"
    return 1
  fi
  mg sync-rig up --record readonly
  cp "$SB/compose.yml.orig" "$compose"
  jq -c '{status, failures}' "$RESULT"
  expect "up exit" "$RC" 2 &&
    expect "up status" "$(field .status)" error &&
    expect "recorder failure names the cause" \
      "$(field '[.failures[] | select(.id == "recorder-unhealthy") | .message | test("[Rr]ead-only file system")] | any')" true || return 1
  mg sync-rig status
  jq -c '{status, failures}' "$RESULT"
  expect "status exit" "$RC" 1 &&
    expect "stopped recorder" "$(field '[.failures[].id] | index("recorder-stopped") != null')" true || return 1
  mg sync-rig down
  expect "down status" "$(field .status)" pass
}

# The fault proxy and the recorder of actor scenarios (sync-ops.md S8.2): only
# on the internal oracle-internal network, nothing published, pinned by the
# rig's digests, never started by a plain `compose up`.
t_actor_scenario_proxies_are_internal_only() {
  local cfg
  cfg="$(sbcompose --profile '*' config --format json)" || return 1
  expect "networks" "$(jq -c '[.services["sync-faults", "sync-recorder"] | .networks | keys] | unique' <<<"$cfg")" \
    '[["oracle-internal"]]' &&
    expect "oracle-internal is internal" "$(jq '.networks["oracle-internal"].internal' <<<"$cfg")" true &&
    expect "published ports" "$(jq '[.services["sync-faults", "sync-recorder"] | .ports // [] | length] | add' <<<"$cfg")" 0 &&
    expect "toxiproxy pin" "$(jq -r '.services["sync-faults"].image == .services.toxiproxy.image' <<<"$cfg")" true &&
    expect "mitmdump pin" "$(jq -r '.services["sync-recorder"].image == .services.mitmdump.image' <<<"$cfg")" true &&
    expect "profiles" "$(jq -c '[.services["sync-faults", "sync-recorder"] | .profiles]' <<<"$cfg")" \
      '[["oracle-sync"],["oracle-record"]]'
}

# record_sync.py's actor and step attribution (sync-ops.md S9): its unit tests
# (docker/mitm/tests) with mitmproxy's own test helpers, in the sync-recorder
# image.
t_recorder_attributes_exchanges_to_actors_and_steps() {
  sbcompose --profile oracle-record run --rm -T --no-deps --entrypoint python3 -e PYTHONDONTWRITEBYTECODE=1 \
    sync-recorder -B -m unittest discover -v -s /opt/mitm/tests </dev/null
}

# The simulator pool under macOS /bin/bash 3.2 and BSD tools, against the fake
# `xcrun simctl` (sim_pool_fake_xcrun.sh): no real simulator is touched.
t_sim_pool_on_macos_bash() {
  local dir="$SB/sim-pool" u u2 i check
  local PATH="$dir/bin:$PATH" STUB_LOG="$dir/stub.log" FAKE_SIM_DB="$dir/sims.json"
  local MEGURI_SIM_POOL_DIR="$dir/pool" MEGURI_SIM_DEVICES_DIR="$dir/Devices"
  local MEGURI_SIM_POOL_SETTLE_SECONDS=0 MEGURI_SIM_POOL_POLL_SECONDS=1 MEGURI_SIM_POOL_WAIT_SECONDS=3
  # The pool on this Mac (U8c moved it to the test VM): never the real VM.
  local MEGURI_MAC_VM_CONFIG="$dir/no-mac-vm.json" MEGURI_SIM_ALLOW_HOST=1
  export PATH STUB_LOG FAKE_SIM_DB MEGURI_SIM_POOL_DIR MEGURI_SIM_DEVICES_DIR MEGURI_MAC_VM_CONFIG MEGURI_SIM_ALLOW_HOST
  export MEGURI_SIM_POOL_SETTLE_SECONDS MEGURI_SIM_POOL_POLL_SECONDS MEGURI_SIM_POOL_WAIT_SECONDS
  mkdir -p "$dir/bin" "$MEGURI_SIM_DEVICES_DIR" && : >"$STUB_LOG" || return 1
  ln -s "$SB/tools/tests/sim_pool_fake_xcrun.sh" "$dir/bin/xcrun" || return 1
  [ "$(command -v xcrun)" = "$dir/bin/xcrun" ] || { echo "the fake xcrun is not first on PATH"; return 1; }

  mg sim acquire iphone --holder host-test
  u="$(field .details.udid)"
  expect "acquire exit" "$RC" 0 &&
    expect "lease holder" "$(field .details.lease.holder)" host-test &&
    expect "device state" "$(xcrun simctl __state "$u")" Booted || return 1
  mg sim release "$u"
  expect "release exit" "$RC" 0 || return 1
  : >"$STUB_LOG"
  mg sim acquire iphone
  expect "reacquire exit" "$RC" 0 &&
    expect "reused device" "$(field .details.udid)" "$u" &&
    expect "no boot or create on reuse" "$(grep -Ec '^xcrun simctl (boot|create) ' "$STUB_LOG")" 0 || return 1

  # Parallel acquires with mkdir locks: two more fit under the cap of three.
  for i in 1 2 3; do
    "$SB/tools/scripts/sim-pool.sh" acquire iphone >"$dir/out$i" 2>"$dir/err$i" &
  done
  wait
  expect "parallel leases" "$(cat "$dir"/out1 "$dir"/out2 "$dir"/out3 | sort -u | wc -l | tr -d ' ')" 2 &&
    expect "pool devices" "$(jq '[.devices[] | select(.name | startswith("Meguri-Pool-iphone-"))] | length' "$FAKE_SIM_DB")" 3 || return 1

  check="$(xcrun simctl __add Meguri-check-1-iPhone17Pro Booted)"
  touch -t 202601010000 "$MEGURI_SIM_DEVICES_DIR/$check" "$MEGURI_SIM_DEVICES_DIR/$u"
  mg sim gc --list
  expect "gc --list targets" "$(field '[.details.targets[].name] | join(" ")')" Meguri-check-1-iPhone17Pro &&
    expect "gc --list deletes nothing" "$(grep -Ec '^xcrun simctl (shutdown|delete) ' "$STUB_LOG")" 0 || return 1
  mg sim gc
  expect "gc deleted" "$(field '.details.deleted | join(" ")')" "$check" &&
    expect "only the leftover was shut down and deleted" \
      "$(grep -E '^xcrun simctl (shutdown|delete|erase) ' "$STUB_LOG" | tr '\n' ' ')" \
      "xcrun simctl shutdown $check xcrun simctl delete $check " || return 1
  mg sim status
  expect "status devices" "$(field '.details.devices | length')" 3 &&
    expect "status leased" "$(field '[.details.devices[] | select(.leased)] | length')" 3
}

# Host lease, Mac UI gate and sim-setup (U8) under macOS bash 3.2 and BSD
# tools, with caffeinate and the real session check reading a fixed session:
# fake ioreg and system_profiler say unlocked with a display awake, or locked
# with FAKE_SESSION_LOCKED set. This Mac's own session must not decide the
# result: the gate runs this suite unattended, often while it is locked.
t_host_lease_gate_and_sim_setup_on_macos_bash() {
  local dir="$SB/host-lease" first u
  local MEGURI_HOST_LEASE_DIR="$dir/host.lock" MEGURI_HOST_LEASE_POLL_SECONDS=1 MEGURI_MAC_UI_DIR="$dir/mac-ui" FX_LOG="$dir/fx.log"
  local PATH="$dir/bin:$PATH" FAKE_SESSION_LOCKED=""
  export MEGURI_HOST_LEASE_DIR MEGURI_HOST_LEASE_POLL_SECONDS MEGURI_MAC_UI_DIR FX_LOG PATH FAKE_SESSION_LOCKED
  mkdir -p "$dir/bin" && : >"$FX_LOG" || return 1
  cat >"$dir/bin/ioreg" <<'EOF'
#!/bin/sh
echo '+-o Root  <class IORegistryEntry>'
echo "  \"IOConsoleUsers\" = ({\"kCGSSessionOnConsoleKey\"=Yes${FAKE_SESSION_LOCKED:+,\"CGSSessionScreenIsLocked\"=Yes})"
EOF
  cat >"$dir/bin/system_profiler" <<'EOF'
#!/bin/sh
echo '{"SPDisplaysDataType": [{"spdisplays_ndrvs": [{"_name": "Display", "spdisplays_asleep": "spdisplays_no"}]}]}'
EOF
  chmod +x "$dir/bin/ioreg" "$dir/bin/system_profiler" || return 1
  cat >"$SB/tools/mg.d/zz-host-fixture.sh" <<'EOF'
mg_job fx-lease host fx_lease "fixture: hold the host lease for FX_HOLD seconds"
mg_job fx-gate host fx_gate "fixture: take the Mac UI gate"
fx_lease() { mg_host_lease || return 0; echo "start $1" >>"$FX_LOG"; sleep "${FX_HOLD:-1}"; echo "end $1" >>"$FX_LOG"; mg_status pass; }
fx_gate() { mg_mac_ui_gate || return 0; echo "allowed ${MEGURI_ALLOW_MAC_UI:-unset}" >>"$FX_LOG"; mg_status pass; }
EOF
  FX_HOLD=2 /bin/bash "$MG" fx-lease a >/dev/null 2>>"$SB/stderr.log" &
  first=$!
  for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20; do [ -s "$FX_LOG" ] && break; sleep 0.2; done
  FX_HOLD=0 mg fx-lease b
  wait "$first"
  expect "second lease exit" "$RC" 0 &&
    expect "leases in turn" "$(tr '\n' ' ' <"$FX_LOG")" "start a end a start b end b " &&
    expect "lease released" "$([ -e "$MEGURI_HOST_LEASE_DIR" ] && echo held || echo free)" free || return 1
  # A locked session queues the job after it takes the lease, and frees it.
  : >"$FX_LOG"
  FAKE_SESSION_LOCKED=1 mg fx-lease c
  expect "locked session exit" "$RC" 0 &&
    expect "locked session result" "$(field .status) $(field .details.queued) $(field '.details.queued_parts[0].user_step') $(field .details.host_session)" \
      "warn true host-session locked" &&
    expect "locked session user step" "$(field '[.user_steps[] | select(.id == "host-session")] | length')" 1 &&
    expect "the job did not run" "$(cat "$FX_LOG")" "" &&
    expect "lease released after the block" "$([ -e "$MEGURI_HOST_LEASE_DIR" ] && echo held || echo free)" free || return 1
  mg fx-gate
  expect "gate allows the first run" "$(field .status) $(cat "$FX_LOG")" "pass allowed 1" || return 1
  mg fx-gate
  expect "gate queues the next" "$(field .status) $(field .details.queued)" "warn true" &&
    expect "queued reason" "$(field .message | grep -c 'the next may start at 20')" 1 || return 1
  rm -f "$SB/tools/mg.d/zz-host-fixture.sh"

  local STUB_LOG="$dir/stub.log" FAKE_SIM_DB="$dir/sims.json"
  local MEGURI_SIM_POOL_DIR="$dir/pool" MEGURI_SIM_DEVICES_DIR="$dir/Devices" MEGURI_SIM_POOL_SETTLE_SECONDS=0
  local MEGURI_MAC_VM_CONFIG="$dir/no-mac-vm.json" MEGURI_SIM_ALLOW_HOST=1
  export STUB_LOG FAKE_SIM_DB MEGURI_SIM_POOL_DIR MEGURI_SIM_DEVICES_DIR MEGURI_SIM_POOL_SETTLE_SECONDS
  export MEGURI_MAC_VM_CONFIG MEGURI_SIM_ALLOW_HOST
  mkdir -p "$MEGURI_SIM_DEVICES_DIR" && : >"$STUB_LOG" || return 1
  ln -s "$SB/tools/tests/sim_pool_fake_xcrun.sh" "$dir/bin/xcrun" || return 1
  mg sim-setup
  u="$(jq -r '.devices.iphone.udids[0]' "$SB/build/sim-devices.local.json")"
  expect "sim-setup exit" "$RC" 0 &&
    expect "created kinds" "$(grep -c '^xcrun simctl create ' "$STUB_LOG")" 4 &&
    expect "foldable lane" "$(field '.details.foldable.status')" unavailable || return 1
  : >"$STUB_LOG"
  mg sim-setup
  expect "rerun creates nothing" "$(grep -c '^xcrun simctl create ' "$STUB_LOG")" 0 &&
    expect "same iPhone" "$(jq -r '.devices.iphone.udids[0]' "$SB/build/sim-devices.local.json")" "$u" || return 1

  # A recording under macOS bash and BSD ps: the worker holds the lease, is
  # known by its pid and start time, and releases the lease (under the
  # lease's mutex) when record stop ends it.
  local state worker
  u="$(cd "$SB" && tools/scripts/sim-pool.sh acquire iphone --holder selftest 2>/dev/null)"
  mg record start --device "$u"
  state="$SB/build/mg/record/active/$u.json"
  worker="$(jq -r .pid "$state" 2>/dev/null)"
  expect "record start" "$RC $(field .status)" "0 pass" &&
    expect "worker start time" "$(jq -r .pid_started "$state")" "$(ps -o lstart= -p "$worker" | sed 's/[[:space:]]*$//')" &&
    expect "the worker holds the lease" "$(jq -r '"\(.pid) \(.kind)"' "$MEGURI_HOST_LEASE_DIR/holder.json")" "$worker recording" || return 1
  mg record stop --device "$u"
  "$SB/tools/scripts/sim-pool.sh" release "$u" >/dev/null 2>&1
  expect "record stop" "$RC $(field .status) $(field .details.recording.reason)" "0 pass stopped" &&
    expect "the worker ended" "$(kill -0 "$worker" 2>/dev/null && echo running || echo ended)" ended &&
    expect "the lease is free" "$([ -e "$MEGURI_HOST_LEASE_DIR" ] && echo held || echo free)" free
}

# fixtures-check (U11), a host job, under macOS bash 3.2 with the fixtures and
# oracle jobs and the recipes' launch run stubbed: collections that need no
# extra oracle argument (a size, a recipe) and one that does (the due deck),
# and the recipe's launch check.
t_fixtures_check_on_macos_bash() {
  local dir="$SB/fixtures-check" testkit="$SB/core/crates/meguri-testkit"
  local MG_CONTAINER=dev STUB_DIR="$SB/fixtures-check" STUB_LOG="$SB/fixtures-check/stub.log"
  export MG_CONTAINER STUB_DIR STUB_LOG
  mkdir -p "$dir" "$testkit/profiles/worst-cases" "$testkit/fixtures/recipes" && : >"$STUB_LOG" || return 1
  printf '{"totals": {"span_days": 500}}\n' >"$testkit/profiles/user-shape.json"
  printf '{"due_deck": {"review_limit": 9999}}\n' >"$testkit/profiles/worst-cases/due.json"
  cat >"$SB/tools/mg.d/zz-fixtures-stub.sh" <<'EOF'
mg_fixtures() { echo "fixtures $*" >>"$STUB_LOG"; mg_detail fixtures "$(jq -sc . "$STUB_DIR/summaries.jsonl")"; mg_status pass; }
mg_oracle() { echo "oracle $*" >>"$STUB_LOG"; mg_detail check "$(jq -c . "$STUB_DIR/$(basename "$2").json")"; mg_status pass; }
mg__fixtures_launch_run() { echo "launch $1" >>"$STUB_LOG"; printf '{"result": "Passed", "passed": 1, "failed": 0, "skipped": 0, "messages": []}\n' >"$2.json"; }
EOF
  local m='{"counts": {"cards": 10, "decks": 2, "notes": 8, "notetypes": 1, "presets": 2, "revlog": 40, "tags": 3}}'
  local report='{"changed_tables": ["col", "config"], "check_db": {"card_properties": 0}, "counts": {"cards": 10, "decks": 2, "graves": 0, "notes": 8, "notetypes": 1, "presets": 2, "revlog": 40, "tags": 3}, "decks": {"Due": {"learning": 0, "new": 0, "review": 10}}, "render": {}, "timing": {"days_elapsed": 500}}'
  {
    jq -nc --argjson m "$m" '{target: "10k", kind: "size", dir: "build/fixtures/s-1-10k", manifest: $m}'
    jq -nc --argjson m "$m" '{target: "due", kind: "worst-case", dir: "build/fixtures/s-1-due", manifest: $m}'
    jq -nc --argjson m "$m" '{target: "r", kind: "recipe", dir: "build/fixtures/recipes/r-1", manifest: $m}'
  } >"$dir/summaries.jsonl"
  printf '%s\n' "$report" >"$dir/s-1-10k.json"
  printf '%s\n' "$report" >"$dir/s-1-due.json"
  printf '%s\n' "$report" | jq -c '.timing.days_elapsed = 120' >"$dir/r-1.json"
  mg fixtures-check
  rm -f "$SB/tools/mg.d/zz-fixtures-stub.sh"
  expect "exit" "$RC" 0 &&
    expect "status" "$(field .status) $(field .summary.passed) $(field '.failures | length')" "pass 4 0" &&
    expect "default targets" "$(head -n 1 "$STUB_LOG")" "fixtures 10k 100k worst-cases recipes --seed 1" &&
    expect "oracle checks" "$(grep '^oracle ' "$STUB_LOG" | tr '\n' '|')" \
      "oracle check $SB/build/fixtures/s-1-10k|oracle check $SB/build/fixtures/s-1-due --deck Due|oracle check $SB/build/fixtures/recipes/r-1|" &&
    expect "launch run" "$(grep '^launch ' "$STUB_LOG")" "launch $SB/build/fixtures/recipes/r-1"
}

# bench-mac (U12), a host job, under macOS bash 3.2 with cargo, the nested
# fixtures job and validation stubbed: the default sizes, the build before
# the run, and the host lease taken without the session check (it passes
# on a locked Mac) and released.
t_bench_mac_on_macos_bash() {
  local dir="$SB/bench-mac"
  local PATH="$dir/bin:$PATH" STUB_LOG="$dir/stub.log" MEGURI_HOST_LEASE_DIR="$dir/host.lock" FX_RESULT="$dir/open.json"
  export PATH STUB_LOG MEGURI_HOST_LEASE_DIR FX_RESULT
  mkdir -p "$dir/bin" && : >"$STUB_LOG" || return 1
  cat >"$dir/bin/cargo" <<'EOF'
#!/bin/bash
echo "cargo $* sizes=${MEGURI_BENCH_SIZES:-}" >>"$STUB_LOG"
case "$1" in
  metadata) printf '{"packages": [{"name": "meguri-testkit", "targets": [{"name": "open", "kind": ["bench"], "src_path": "/x/benches/open.rs"}, {"name": "icount_open", "kind": ["bench"], "src_path": "/x/benches/icount/open.rs"}]}]}\n' ;;
  bench) case " $* " in *" --no-run "*) ;; *) cp "$FX_RESULT" "$MEGURI_BENCH_RESULTS/open.json" ;; esac ;;
esac
EOF
  chmod +x "$dir/bin/cargo"
  printf '{"suite": "open"}\n' >"$FX_RESULT"
  cat >"$SB/tools/mg.d/zz-bench-stub.sh" <<'EOF'
mg__bench_mac_fixtures() { echo "fixtures $*" >>"$STUB_LOG"; }
mg__perf_validate() { echo "validate $(basename "$1") lease=$(jq -r .job "$MEGURI_HOST_LEASE_DIR/holder.json" 2>/dev/null)" >>"$STUB_LOG"; }
EOF
  mg bench-mac
  rm -f "$SB/tools/mg.d/zz-bench-stub.sh"
  expect "exit" "$RC" 0 &&
    expect "status" "$(field .status) $(field .details.host_lease.job) $(field .details.blocked)" "pass bench-mac null" &&
    expect "calls" "$(grep -v '^cargo metadata' "$STUB_LOG" | tr '\n' '|')" \
      "fixtures 10k 100k 150k 500k|cargo bench -p meguri-testkit --bench open --no-run sizes=|cargo bench -p meguri-testkit --bench open -- sizes=10k,100k,150k,500k|validate open.json lease=bench-mac|" &&
    expect "lease released" "$([ -e "$MEGURI_HOST_LEASE_DIR" ] && echo held || echo free)" free
}

t_catch_up_reuses_passing_results_on_macos_bash() {
  local dir="$SB/reuse-check" proof caught
  mkdir -p "$dir/tools" || return 1
  cp -R "$REPO/tools/." "$dir/tools/" || return 1
  cp "$REPO/.gitignore" "$dir/" || return 1
  export TEST_REUSE_LOG="$SB/reuse-count.log"
  : >"$TEST_REUSE_LOG"
  cat >"$dir/tools/mg.d/zz-reuse-test.sh" <<'EOF'
mg_job reuse-proof host t_reuse_proof "passing result reuse test"
t_reuse_proof() { echo called >>"$TEST_REUSE_LOG"; mg_count passed 1; mg_status pass; }
mg_doctor() { mg_user_step host-session missing true "locked" "test"; mg_status pass; }
EOF
  git -C "$dir" init -q &&
    git -C "$dir" config user.name selftest &&
    git -C "$dir" config user.email selftest@localhost &&
    git -C "$dir" add -A && git -C "$dir" commit -qm 'reuse test' || return 1
  proof="$(/bin/bash "$dir/tools/mg" reuse-proof 'argument with spaces')" || return 1
  mkdir -p "$dir/build/mg/catch-up"
  jq -nc '{id:"host-reuse",state:"queued",job:"reuse-proof",args:["argument with spaces"],unit:"U19",user_steps:["host-session"]}' \
    >"$dir/build/mg/catch-up/queue.jsonl"
  caught="$(/bin/bash "$dir/tools/mg" catch-up)" || return 1
  expect "only initial proof ran" "$(wc -l <"$TEST_REUSE_LOG" | tr -d ' ')" 1 &&
    expect "reuse counted" "$(jq -r .summary.reused "$caught")" 1 &&
    expect "same proof" "$(jq -r '.details.runs[0].result' "$caught")" "$proof" &&
    expect "no waiting" "$(jq -r .summary.waiting "$caught")" 0 &&
    expect "queue done" "$(tail -n 1 "$dir/build/mg/catch-up/queue.jsonl" | jq -r .state)" done
}

# Exercise the Mac wrapper's empty -only-testing array and TEST_RUNNER_
# forwarding under the host Bash, with the existing VM runner boundary
# stubbed. Full SSH/lease/result transport is covered by sim_vm.bats.
t_snapshot_vm_dispatch_on_macos_bash() {
  local dir="$SB/snapshot-vm-bash"
  mkdir -p "$dir/bin"
  cat >"$dir/bin/xcrun" <<'EOF'
#!/bin/bash
case "$*" in
  'xcresulttool get test-results summary '*) echo '{"result":"Passed","passedTests":1,"failedTests":0,"skippedTests":0}' ;;
  'xcresulttool get test-results tests '*) echo '{"testNodes":[]}' ;;
  *) exit 97 ;;
esac
EOF
  cat >"$dir/bin/xcodebuild" <<'EOF'
#!/bin/bash
exit 97
EOF
  chmod +x "$dir/bin/xcrun" "$dir/bin/xcodebuild"
  cat >"$dir/dispatch.sh" <<'EOF'
#!/bin/bash
set -u
MG_TOOLS="$1/tools" MG_RUN_DIR="$2" MG_MAC_VM_LOST="" MG_MAC_VM_LOST_UNREACHABLE=""
MG_MAC_VM_RUN_ERROR="" MG_MAC_VM_RUN_QUEUED="" MG_SNAP_BROKEN=0 MG_SIM_RUNNER=vm
mg_job() { :; }
mg_artifact() { :; }
mg_failure() { echo "unexpected failure: $*" >&2; exit 1; }
mg_vm_xcodebuild_test() {
  printf '%s\n' "$* env=${TEST_RUNNER_MEGURI_SNAPSHOT_OUT:-missing}" >>"$MG_RUN_DIR/calls"
  mkdir -p "$1.xcresult"
  : >"$1.log"
}
mg_sim_fetch() { echo "fetch $*" >>"$MG_RUN_DIR/calls"; }
. "$MG_TOOLS/mg.d/snapshots.sh"
mg_snapshots__xcodebuild "$MG_RUN_DIR/mac" platform=macOS,arch=arm64 all TEST_RUNNER_MEGURI_SNAPSHOT_OUT=/guest/container || exit 1
[ -z "${TEST_RUNNER_MEGURI_SNAPSHOT_OUT+x}" ] || exit 1
mg_snapshots__xcodebuild "$MG_RUN_DIR/ipad" id=FAKE-IPAD "$MG_SNAP_SUITES" TEST_RUNNER_MEGURI_SNAPSHOT_OUT=/guest/ipad || exit 1
[ -z "${TEST_RUNNER_MEGURI_SNAPSHOT_OUT+x}" ]
EOF
  PATH="$dir/bin:$PATH" /bin/bash "$dir/dispatch.sh" "$SB" "$dir" || return 1
  expect "VM dispatch and fetch" "$(cat "$dir/calls")" "$dir/mac Snapshot macosx platform=macOS,arch=arm64 Debug env=/guest/container
$dir/ipad Snapshot iphonesimulator id=FAKE-IPAD Debug -only-testing:MeguriSnapshotTests/GalleryComponentSnapshots -only-testing:MeguriSnapshotTests/ScreenSnapshots env=/guest/ipad
fetch $dir/ipad"
}

# Receipt classification must parse AND run on the actual Apple Bash. All
# utility/VM boundaries below are nonmutating doubles; no archive is executed.
t_manual_transfer_repair_receipts_on_macos_bash() {
  local dir="$SB/manual-transfer-receipts-bash"
  [ "$(uname -s)" = Darwin ] && /bin/bash -c \
    '[ "${BASH_VERSINFO[0]}" = 3 ] && [ "${BASH_VERSINFO[1]}" = 2 ]' || {
    echo "requires macOS /bin/bash 3.2; unsupported host is not coverage"
    return 1
  }
  /bin/bash -n "$SB/tools/mg.d/manual-transfer-check.sh" || return 1
  mkdir -p "$dir" || return 1
  cat >"$dir/classify.sh" <<'EOF'
#!/bin/bash
set -u
MG_TOOLS="$1/tools" MG_ROOT="$2"
mg_job() { :; }
. "$MG_TOOLS/mg.d/manual-transfer-check.sh" || exit 1
# No real provenance utility, SSH, VM, lease, or cleanup registration runs.
python3() {
  [ "$#" -eq 4 ] && [ "$1" = "$MG_TOOLS/scripts/manual-transfer-config.py" ] &&
    [ "$2" = repair-source ] && [ "$3" = "$MG_ROOT" ] &&
    [ "$4" = 20261023T234838Z-84101 ] || return 97
  printf '{}\n'
}
mg_mac_ui_begin() { MG_MAC_RUNNER=vm; MG_MAC_VMS='[{"name":"meguri-mac"}]'; }
mg_mac_vm_take() {
  [ "$#" -eq 1 ] && [ "$1" = --no-copy ] || return 97
  MG_MAC_VM='{"name":"meguri-mac"}' MG_LEASE_DIR="$MG_ROOT/fake-lease"
}
mvl_lease_held() { [ "$1" = "$MG_ROOT/fake-lease" ]; }
mvl_q() { printf '%q' "$1"; }
mg_mac_vm_copy_in() { [ "$1" = "$MG_RUN_DIR/repair-config.py" ]; }
mg_on_exit() { [ "$1" = mg_mt_cleanup_repair ]; }
mg_mac_vm_ssh_in() {
  [ "$1" = "/usr/bin/python3 $(mvl_q "$MG_RUN_DIR/repair-config.py") archive-stale-fixture-state" ] &&
    [ "$(cat)" = '{}' ] || return 97
  printf 'called\n' >>"$MG_RUN_DIR/utility-calls"
  # Only emit synthetic aggregate JSON; never invoke the archive utility.
  if [ "$reply_status" != missing ]; then
    jq -nc --arg status "$reply_status" --argjson archived "$reply_archived" \
      --arg purpose "$reply_purpose" --arg source "$reply_source" \
      '{purpose:$purpose,source_run:$source,status:$status,archived:$archived}' || return 97
  fi
  return "$reply_rc"
}
mg_detail() { printf '%s\n' "$2" >"$MG_RUN_DIR/$1.json"; }
mg_status() { printf '%s\n' "$1" >"$MG_RUN_DIR/status"; }
mg_failure() { printf '%s\n' "$1" >"$MG_RUN_DIR/failure"; }
mg_artifact() { printf '%s\n' "$1" >"$MG_RUN_DIR/artifact"; }
mg_word_in() { local word; for word in $2; do [ "$1" != "$word" ] || return 0; done; return 1; }

check() {
  local label="$1" outcome="$5"
  reply_rc="$2" reply_status="$3" reply_archived="$4"
  reply_purpose="${6:-environment-repair-not-M8}" reply_source="${7:-20261023T234838Z-84101}"
  MG_RUN_DIR="$MG_ROOT/$label"
  mkdir -p "$MG_RUN_DIR" || return 1
  mg_mt_archive_stale_fixture_state --archive-stale-fixture-state --platform mac --source-run 20261023T234838Z-84101 || return 1
  [ "$(cat "$MG_RUN_DIR/repair_ssh_exit_code.json")" = "$reply_rc" ] &&
    [ "$(cat "$MG_RUN_DIR/purpose.json")" = '"environment-repair-not-M8"' ] &&
    [ "$(cat "$MG_RUN_DIR/utility-calls")" = called ] &&
    [ ! -e "$MG_RUN_DIR/native_transfer_complete.json" ] || return 1
  if [ "$outcome" = unknown ]; then
    [ "$(cat "$MG_RUN_DIR/status")" = error ] &&
      [ "$(cat "$MG_RUN_DIR/failure")" = manual-transfer/repair-uncertain ] &&
      [ "$(cat "$MG_RUN_DIR/repair_status.json")" = '"preservation-unknown"' ] &&
      [ ! -e "$MG_RUN_DIR/environment_repair.json" ] && [ ! -e "$MG_RUN_DIR/artifact" ]
  else
    [ ! -e "$MG_RUN_DIR/repair_status.json" ] &&
      [ "$(cat "$MG_RUN_DIR/artifact")" = repair-receipt ] &&
      cmp -s "$MG_RUN_DIR/repair-receipt.json" "$MG_RUN_DIR/environment_repair.json" || return 1
    case "$outcome" in
      archived) [ "$(cat "$MG_RUN_DIR/status")" = pass ] && [ ! -e "$MG_RUN_DIR/failure" ] ;;
      *) [ "$(cat "$MG_RUN_DIR/status")" = error ] &&
        [ "$(cat "$MG_RUN_DIR/failure")" = "manual-transfer/$outcome" ] ;;
    esac
  fi
}
while read -r label rc status archived outcome; do
  check "$label" "$rc" "$status" "$archived" "$outcome" || { echo "classification failed: $label"; exit 1; }
done <<'CASES'
archived 0 archived true archived
blocked 3 blocked false repair-blocked
unconfirmed 4 preservation-unconfirmed false repair-preservation
status-mismatch-0 0 blocked false unknown
status-mismatch-3 3 archived true unknown
status-mismatch-4 4 archived true unknown
flag-mismatch-0 0 archived false unknown
flag-mismatch-3 3 blocked true unknown
flag-mismatch-4 4 preservation-unconfirmed true unknown
lost 255 archived true unknown
missing-0 0 missing false unknown
missing-3 3 missing false unknown
missing-4 4 missing false unknown
unexpected-rc 97 blocked false unknown
CASES
check wrong-purpose 0 archived true unknown other-purpose || exit 1
check wrong-source 0 archived true unknown environment-repair-not-M8 other-run || exit 1
EOF
  /bin/bash "$dir/classify.sh" "$SB" "$dir"
}

t_results_validate_against_schema() {
  [ -n "$RESULTS" ] || { echo "no results collected"; return 1; }
  # shellcheck disable=SC2086 # RESULTS is a space-separated list of relative paths
  sbcompose run --rm -T --no-deps dev python3 tools/tests/validate_result.py $RESULTS </dev/null
}

TESTS="t_unknown_job t_dry_run_every_job t_doctor_on_this_mac t_core_test_runs_in_dev_container
t_killing_mg_stops_the_docker_job_container t_interrupting_mg_stops_the_docker_job_container
t_dev_containers_share_the_cargo_package_cache_lock
t_every_compose_service_runs_an_init_process t_dev_image_rebuild_reuses_layers
t_cancelling_a_host_job_removes_its_container t_implementer_worktree_and_commit_hook_on_macos
t_parallel_session_records_on_macos t_similarity_flags_a_copy_through_tools_mg t_similarity_scans_changed_files_and_commits
t_similarity_nightly_runs_jplag t_sync_rig_publishes_on_loopback_only_and_down_removes_it
t_sync_rig_probes_ignore_proxy_variables t_sync_rig_records_decoded_exchanges
t_sync_rig_up_fails_when_the_recording_is_unwritable t_actor_scenario_proxies_are_internal_only
t_recorder_attributes_exchanges_to_actors_and_steps t_sim_pool_on_macos_bash t_host_lease_gate_and_sim_setup_on_macos_bash
t_fixtures_check_on_macos_bash t_bench_mac_on_macos_bash t_catch_up_reuses_passing_results_on_macos_bash t_snapshot_vm_dispatch_on_macos_bash
t_manual_transfer_repair_receipts_on_macos_bash t_results_validate_against_schema"

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
