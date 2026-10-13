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
    expect "record in the branch's ledger" "$(tail -n 1 "$wt/docs/process/provenance/sessions.jsonl" | jq -r .session)" "$id" &&
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
t_similarity_flags_a_copy_through_tools_mg t_similarity_scans_changed_files_and_commits
t_similarity_nightly_runs_jplag t_results_validate_against_schema"

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
