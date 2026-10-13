#!/usr/bin/env bats
# tools/mg self-tests, run in the dev container by `tools/mg selftest`.
# Each test runs a fresh copy of tools/ and docker/ inside a throwaway git
# repo, so its results never land in the real build/ directory. Host-only
# behavior (macOS bash, real doctor, Docker from the host) is covered by
# mg_host_selftest.sh.

bats_require_minimum_version 1.5.0

REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"

setup_file() {
  local t="$BATS_FILE_TMPDIR/template"
  mkdir -p "$t/core"
  cp -R "$REPO_ROOT/tools" "$t/tools"
  cp -R "$REPO_ROOT/docker" "$t/docker"
  cp "$REPO_ROOT/core/rust-toolchain.toml" "$t/core/"
  cp "$REPO_ROOT/.gitignore" "$t/"
  git -C "$t" init -q
  git -C "$t" add -A
  git -C "$t" -c user.name=selftest -c user.email=selftest@localhost commit -qm template
}

setup() {
  SB="$BATS_TEST_TMPDIR/repo"
  cp -R "$BATS_FILE_TMPDIR/template" "$SB"
  export CARGO_TARGET_DIR="$BATS_TEST_TMPDIR/target" CARGO_NET_OFFLINE=true
}

# mg <args>: run the sandbox's tools/mg. Sets status, output (stdout), stderr,
# lines, and result (the last stdout line).
mg() {
  run --separate-stderr "$SB/tools/mg" "$@"
  result="${lines[${#lines[@]} - 1]:-}"
}

field() { jq -r "$@" "$result"; }

validate() { python3 "$SB/tools/tests/validate_result.py" "$@"; }

add_fixture_jobs() {
  cat >"$SB/tools/mg.d/zz-fixture.sh" <<'EOF'
mg_job fx-pass host fx_pass "fixture: pass with counts, artifact, detail"
mg_job fx-warn host fx_warn "fixture: warn"
mg_job fx-fail host fx_fail "fixture: fail"
mg_job fx-error host fx_error "fixture: error"
mg_job fx-crash host fx_crash "fixture: non-zero exit, no status"
mg_job fx-pass-exit host fx_pass_exit "fixture: pass then non-zero exit"
mg_job fx-warn-exit host fx_warn_exit "fixture: warn then non-zero exit"
mg_job fx-failure-only host fx_failure_only "fixture: failure, no status"
mg_job fx-args host fx_args "fixture: echo args"
fx_pass() {
  echo "hello from fx-pass"
  : >"$MG_RUN_DIR/shot.png"
  mg_artifact png "$MG_RUN_DIR/shot.png" screenshot
  mg_count total 2
  mg_count passed 2
  mg_detail answer 42
  mg_message "fixture passed"
  mg_status pass
}
fx_warn() { mg_status warn; }
fx_fail() { mg_failure broken "it broke"; mg_status fail; }
fx_error() { mg_status error; }
fx_crash() { echo "crashing" >&2; return 3; }
fx_pass_exit() { mg_status pass; return 7; }
fx_warn_exit() { mg_status warn; return 7; }
fx_failure_only() { mg_failure broken "it broke" "tools/mg fx-failure-only --again"; }
fx_args() { mg_detail args "$(mg_json_array "$@")"; mg_status pass; }
EOF
}

# Host command stubs for doctor. Every call is logged to $STUB_LOG. Commands
# that could change system state are stubbed too, so a call to them shows up
# in the log instead of failing silently with "command not found".
make_stubs() {
  STUBS="$BATS_TEST_TMPDIR/stubs"
  mkdir -p "$STUBS"
  export STUB_LOG="$BATS_TEST_TMPDIR/stub.log"
  : >"$STUB_LOG"
  cat >"$STUBS/_stub" <<'EOF'
#!/usr/bin/env bash
name="$(basename "$0")"
printf '%s\n' "$name $*" >>"$STUB_LOG"
case "$name $*" in
  "uname -s") echo Darwin ;;
  "uname -m") echo arm64 ;;
  "uname -sm") echo "Darwin arm64" ;;
  "sw_vers -productVersion") echo 27.0 ;;
  "xcode-select -p") echo /Applications/Xcode.app/Contents/Developer ;;
  "xcodebuild -version") printf 'Xcode 27.0\nBuild version 27A266a\n' ;;
  "xcodebuild -license check") exit "${STUB_LICENSE_RC:-0}" ;;
  "xcrun simctl list runtimes --json")
    if [ "${STUB_NO_IOS27:-0}" = 1 ]; then
      echo '{"runtimes":[{"identifier":"com.apple.CoreSimulator.SimRuntime.iOS-18-2","version":"18.2","buildversion":"22C150","isAvailable":true,"platform":"iOS"}]}'
    else
      echo '{"runtimes":[{"identifier":"com.apple.CoreSimulator.SimRuntime.iOS-27-0","version":"27.0","buildversion":"24A434","isAvailable":true,"platform":"iOS"}]}'
    fi ;;
  "defaults read com.apple.dt.Xcode IDEProvisioningTeamByIdentifier")
    if [ "${STUB_ACCOUNT:-1}" = 1 ]; then
      printf '{\n    "A1" = (\n        {\n            teamID = DNQ34W43XX;\n        }\n    );\n}\n'
    else
      echo "The domain/default pair does not exist" >&2
      exit 1
    fi ;;
  "defaults read com.apple.dt.Xcode IDEProvisioningTeams")
    echo "The domain/default pair does not exist" >&2
    exit 1 ;;
  "docker info --format {{json .}}")
    # Like the real CLI: with the daemon down it still prints the client-side
    # JSON, with an empty ServerVersion, and exits 1.
    if [ "${STUB_DOCKER:-1}" = 1 ]; then
      echo '{"ServerVersion":"29.8.2","NCPU":16,"MemTotal":46278615040}'
    else
      echo '{"ServerVersion":"","NCPU":0,"MemTotal":0}'
      exit 1
    fi ;;
  "docker compose version --short") echo 5.5.0 ;;
  "rustup toolchain list") echo 1.99.0-aarch64-apple-darwin ;;
  "rustup +1.99.0 target list --installed")
    printf '%s\n' ${STUB_RUST_TARGETS-aarch64-apple-darwin aarch64-apple-ios aarch64-apple-ios-sim} ;;
  "rustup +1.99.0 component list --installed")
    printf 'cargo-aarch64-apple-darwin\nclippy-aarch64-apple-darwin\nrustfmt-aarch64-apple-darwin\n' ;;
  "df -Pk "*)
    printf 'Filesystem 1024-blocks Used Available Capacity Mounted on\n/dev/disk3s1s1 971350180 13631488 251658240 6%% /\n' ;;
  *) exit 97 ;;
esac
EOF
  chmod +x "$STUBS/_stub"
  local c
  for c in uname sw_vers xcode-select xcodebuild xcrun defaults docker rustup df \
    sudo launchctl open softwareupdate security tccutil systemsetup networksetup spctl pmset devicectl; do
    ln -s _stub "$STUBS/$c"
  done
  export PATH="$STUBS:$PATH"
}

# Doctor may only issue these read-only host commands.
assert_read_only_calls() {
  [ -s "$STUB_LOG" ]
  local bad
  bad="$(grep -Ev \
    -e '^uname -[a-z]+$' \
    -e '^sw_vers -productVersion$' \
    -e '^xcode-select -p$' \
    -e '^xcodebuild -(version|license check)$' \
    -e '^xcrun simctl list runtimes --json$' \
    -e '^defaults read com\.apple\.dt\.Xcode [A-Za-z]+$' \
    -e '^docker info --format \{\{json \.\}\}$' \
    -e '^docker compose version --short$' \
    -e '^rustup (toolchain list|\+[0-9.]+ (target|component) list --installed)$' \
    -e '^df -Pk /' \
    "$STUB_LOG" || true)"
  if [ -n "$bad" ]; then
    echo "doctor issued commands outside the read-only allowlist:"
    echo "$bad"
    return 1
  fi
}

write_crate() { # write_crate <name> <lib.rs contents>
  mkdir -p "$SB/core/crates/$1/src"
  printf '[package]\nname = "%s"\nversion = "0.0.0"\nedition = "2024"\npublish = false\n' "$1" >"$SB/core/crates/$1/Cargo.toml"
  printf '%s\n' "$2" >"$SB/core/crates/$1/src/lib.rs"
}

write_workspace() {
  printf '[workspace]\nresolver = "3"\nmembers = ["crates/*"]\n' >"$SB/core/Cargo.toml"
}

# --- dispatcher ------------------------------------------------------------

@test "unknown job exits non-zero with an error result listing the known jobs" {
  mg nosuchjob
  [ "$status" -eq 2 ]
  [ "${#lines[@]}" -eq 1 ]
  [[ "$result" == "$SB/build/mg/_dispatch/"*/result.json ]]
  [ "$(field .status)" = error ]
  [ "$(field .exit_code)" = 2 ]
  [ "$(field .job)" = nosuchjob ]
  [ "$(field '.failures[0].id')" = unknown-job ]
  jq -e '.details.known_jobs | (index("doctor") and index("core-test") and index("selftest") and index("dev-image"))' "$result"
  validate "$result"
}

@test "every registered job's dry-run result validates against the schema" {
  run "$SB/tools/mg" --list
  [ "$status" -eq 0 ]
  local listing="$output" name placement _desc expected
  for expected in doctor core-test selftest selftest-host dev-image; do
    cut -f1 <<<"$listing" | grep -qx "$expected"
  done
  local results=()
  while IFS=$'\t' read -r name placement _desc; do
    mg "$name" --dry-run
    [ "$status" -eq 0 ] || { echo "$name --dry-run exited $status"; return 1; }
    [ "$(field .job)" = "$name" ]
    [ "$(field .dry_run)" = true ]
    [ "$(field .status)" = pass ]
    [ "$(field .details.job.placement)" = "$placement" ]
    results+=("$result")
  done <<<"$listing"
  [ "${#results[@]}" -ge 5 ]
  validate "${results[@]}"
}

@test "results record the commit and whether the tree was dirty" {
  mg doctor --dry-run
  [ "$(field .commit.sha)" = "$(git -C "$SB" rev-parse HEAD)" ]
  [ "$(field .commit.dirty)" = false ]
  echo "# local edit" >>"$SB/docker/compose.yml"
  mg doctor --dry-run
  [ "$(field .commit.dirty)" = true ]
}

@test "a job family dropped into tools/mg.d is reachable without editing the dispatcher" {
  add_fixture_jobs
  run "$SB/tools/mg" --list
  cut -f1 <<<"$output" | grep -qx fx-pass
  mg fx-pass
  [ "$status" -eq 0 ]
  [ "$(field .job)" = fx-pass ]
}

@test "exit code mirrors the job status" {
  add_fixture_jobs
  local results=()
  mg fx-pass
  [ "$status" -eq 0 ] && [ "$(field .status)" = pass ] && results+=("$result")
  mg fx-warn
  [ "$status" -eq 0 ] && [ "$(field .status)" = warn ] && results+=("$result")
  mg fx-fail
  [ "$status" -eq 1 ] && [ "$(field .status)" = fail ] && results+=("$result")
  [ "$(field '.failures[0].repro')" = "tools/mg fx-fail" ]
  mg fx-error
  [ "$status" -eq 2 ] && [ "$(field .status)" = error ] && results+=("$result")
  mg fx-crash
  [ "$status" -eq 2 ] && [ "$(field .status)" = error ] && results+=("$result")
  [ "$(field '.failures[0].id')" = job-exit ]
  mg fx-failure-only
  [ "$status" -eq 1 ] && [ "$(field .status)" = fail ] && results+=("$result")
  [ "$(field '.failures[0].repro')" = "tools/mg fx-failure-only --again" ]
  [ "${#results[@]}" -eq 6 ]
  validate "${results[@]}"
}

@test "a run writes counts, artifacts, details and its log into its run directory" {
  add_fixture_jobs
  mg fx-pass
  local run_dir
  run_dir="$(dirname "$result")"
  [[ "$run_dir" == "$SB/build/mg/fx-pass/"* ]]
  [ "${#lines[@]}" -eq 1 ]
  [[ "$stderr" == *"hello from fx-pass"* ]]
  grep -q "hello from fx-pass" "$run_dir/log.txt"
  [ "$(field -c .summary)" = '{"total":2,"passed":2,"failed":0,"skipped":0}' ]
  jq -e --arg p "$run_dir/shot.png" '[.artifacts[] | select(.kind == "png" and .path == $p and .label == "screenshot")] | length == 1' "$result"
  jq -e --arg p "$run_dir/log.txt" '[.artifacts[] | select(.kind == "log" and .path == $p)] | length == 1' "$result"
  [ "$(field .details.answer)" = 42 ]
  [ "$(field .message)" = "fixture passed" ]
  [ "$(field .placement)" = host ]
  [ "$(field .service)" = null ]
  [ ! -e "$run_dir/.mg" ]
  [ "$(readlink "$SB/build/mg/fx-pass/latest")" = "$(basename "$run_dir")" ]
  validate "$result"
}

@test "a job reporting pass then exiting non-zero reports an error" {
  add_fixture_jobs
  mg fx-pass-exit
  [ "$status" -eq 2 ]
  [ "$(field .status)" = error ]
  [ "$(field .exit_code)" -eq 2 ]
  [ "$(field '.failures[0].id')" = job-exit ]
  [[ "$(field '.failures[0].message')" == *"reported pass but exited with code 7"* ]]
  validate "$result"
}

@test "a job reporting warn then exiting non-zero reports an error" {
  add_fixture_jobs
  mg fx-warn-exit
  [ "$status" -eq 2 ]
  [ "$(field .status)" = error ]
  [ "$(field .exit_code)" -eq 2 ]
  [ "$(field '.failures[0].id')" = job-exit ]
  [[ "$(field '.failures[0].message')" == *"reported warn but exited with code 7"* ]]
  validate "$result"
}

@test "dry runs do not move latest" {
  add_fixture_jobs
  mg fx-pass
  local real
  real="$(basename "$(dirname "$result")")"
  mg fx-pass --dry-run
  [ "$(basename "$(dirname "$result")")" != "$real" ]
  [ "$(readlink "$SB/build/mg/fx-pass/latest")" = "$real" ]
}

@test "arguments reach the job intact; --dry-run after -- is passed on" {
  add_fixture_jobs
  mg fx-args -E "two words" -c "it's" -- --dry-run
  [ "$status" -eq 0 ]
  [ "$(field .dry_run)" = false ]
  [ "$(field -c .details.args)" = '["-E","two words","-c","it'"'"'s","--","--dry-run"]' ]
  [ "$(field -c .args)" = "$(field -c .details.args)" ]
  [ "$(field .message)" = "fx-args: pass" ]
}

@test "a job registered twice makes every run an error" {
  echo 'mg_job doctor host true "duplicate"' >"$SB/tools/mg.d/zz-dup.sh"
  mg doctor --dry-run
  [ "$status" -eq 2 ]
  [ "$(field .status)" = error ]
  [ "$(field '.failures[0].id')" = registry ]
  [[ "$(field '.failures[0].message')" == *"registered twice"* ]]
  validate "$result"
}

@test "a docker job run outside any container needs Docker" {
  export MG_CONTAINER=""
  command -v docker && skip "this container has a docker CLI"
  mg core-test
  [ "$status" -eq 2 ]
  [ "$(field .status)" = error ]
  [ "$(field .placement)" = docker ]
  [ "$(field '.failures[0].id')" = docker-unreachable ]
}

@test "a docker job is refused inside a different container" {
  export MG_CONTAINER=oracle
  mg core-test
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = wrong-container ]
}

# --- doctor (host commands stubbed) ----------------------------------------

@test "doctor without an Xcode account warns, lists the account step and only reads host state" {
  make_stubs
  export STUB_ACCOUNT=0
  mg doctor
  [ "$status" -eq 0 ]
  [ "$(field .status)" = warn ]
  jq -e '.user_steps[] | select(.id == "xcode-account" and .state == "missing" and .required == true)' "$result"
  jq -e '.details.checks[] | select(.id == "xcode-account" and .status == "warn")' "$result"
  [ "$(field '[.details.checks[] | select(.status == "fail")] | length')" = 0 ]
  [ "$(field .details.environment.xcode_version)" = 27.0 ]
  [ "$(field .details.environment.xcode_build)" = 27A266a ]
  [[ "$stderr" == *"xcode-account"* ]]
  assert_read_only_calls
  validate "$result"
}

@test "doctor on a fully set-up Mac passes and still lists the steps it cannot verify" {
  make_stubs
  mg doctor
  [ "$status" -eq 0 ]
  [ "$(field .status)" = pass ]
  [ "$(field '[.user_steps[] | select(.state == "missing" and .required)] | length')" = 0 ]
  jq -e '.user_steps[] | select(.id == "uitest-automation" and .state == "unverified")' "$result"
  jq -e '.user_steps[] | select(.id == "xcode-27-1" and .state == "missing" and .required == false)' "$result"
  [ "$(field .summary.total)" = "$(field '.details.checks | length')" ]
  assert_read_only_calls
  validate "$result"
}

@test "doctor fails and lists the step when the Xcode license is not accepted" {
  make_stubs
  export STUB_LICENSE_RC=1
  mg doctor
  [ "$status" -eq 1 ]
  [ "$(field .status)" = fail ]
  jq -e '.user_steps[] | select(.id == "xcode-license" and .state == "missing")' "$result"
  jq -e '.failures[] | select(.id == "xcode-license" and .message == "the Xcode license is not accepted" and .repro == "tools/mg doctor")' "$result"
  assert_read_only_calls
  validate "$result"
}

@test "doctor fails and lists the step without an iOS 27 simulator runtime" {
  make_stubs
  export STUB_NO_IOS27=1
  mg doctor
  [ "$status" -eq 1 ]
  jq -e '.user_steps[] | select(.id == "ios-runtime" and .state == "missing")' "$result"
  jq -e '.details.ios_runtimes | length == 1' "$result"
  jq -e '.failures[] | select(.id == "ios-runtime" and .message == "no available iOS 27 simulator runtime" and .repro == "tools/mg doctor")' "$result"
  assert_read_only_calls
  validate "$result"
}

@test "doctor fails when Docker is unreachable" {
  make_stubs
  export STUB_DOCKER=0
  mg doctor
  [ "$status" -eq 1 ]
  jq -e '.details.checks[] | select(.id == "docker" and .status == "fail")' "$result"
  jq -e '.failures[] | select(.id == "docker" and .message == "Docker is not reachable; start Docker Desktop" and .repro == "tools/mg doctor")' "$result"
  # A stopped daemon is not a resource problem.
  [ "$(field '[.details.checks[] | select(.id == "docker-resources")] | length')" = 0 ]
  [ "$(field '[.user_steps[] | select(.id == "docker-resources")] | length')" = 0 ]
  assert_read_only_calls
  validate "$result"
}

@test "doctor warns about missing Rust targets without making it a user step" {
  make_stubs
  export STUB_RUST_TARGETS="aarch64-apple-darwin"
  mg doctor
  [ "$status" -eq 0 ]
  [ "$(field .status)" = warn ]
  jq -e '.details.checks[] | select(.id == "rust" and .status == "warn" and (.message | contains("aarch64-apple-ios-sim")))' "$result"
  [ "$(field '[.user_steps[] | select(.state == "missing" and .required)] | length')" = 0 ]
  assert_read_only_calls
}

# --- core-test (runs cargo in this container) -------------------------------

@test "core-test with no Cargo workspace reports zero tests and passes inside the dev container" {
  [ "${MG_CONTAINER:-}" = dev ] || skip "needs the dev container"
  mg core-test
  [ "$status" -eq 0 ]
  [ "$(field .status)" = pass ]
  [ "$(field .placement)" = docker ]
  [ "$(field .service)" = dev ]
  [ "$(field .summary.total)" = 0 ]
  [ "$(field .details.environment.container)" = dev ]
  [[ "$(field .details.environment.os)" == Linux* ]]
  validate "$result"
}

@test "core-test with an empty workspace reports zero tests and passes" {
  [ "${MG_CONTAINER:-}" = dev ] || skip "needs the dev container"
  write_workspace
  write_crate empty ""
  mg core-test
  [ "$status" -eq 0 ]
  [ "$(field .status)" = pass ]
  [ "$(field -c .summary)" = '{"total":0,"passed":0,"failed":0,"skipped":0}' ]
  validate "$result"
}

@test "core-test reports each failing test with a repro command that reruns it" {
  [ "${MG_CONTAINER:-}" = dev ] || skip "needs the dev container"
  write_workspace
  write_crate fx '#[cfg(test)]
mod tests {
    #[test]
    fn adds() { assert_eq!(1 + 1, 2); }
    #[test]
    fn breaks() { assert_eq!(1 + 1, 3, "deliberate"); }
    #[test]
    #[ignore]
    fn later() {}
}'
  mg core-test
  [ "$status" -eq 1 ]
  [ "$(field .status)" = fail ]
  [ "$(field -c .summary)" = '{"total":3,"passed":1,"failed":1,"skipped":1}' ]
  [ "$(field '.failures | length')" = 1 ]
  [[ "$(field '.failures[0].id')" == *'$tests::breaks' ]]
  [[ "$(field '.failures[0].message')" == *deliberate* ]]
  local repro
  repro="$(field '.failures[0].repro')"
  [ "$repro" = "tools/mg core-test -- -E 'package(fx) & test(=tests::breaks)'" ]
  jq -e '[.artifacts[] | select(.kind == "nextest-jsonl")] | length == 1' "$result"
  validate "$result"

  run --separate-stderr bash -c "cd '$SB' && $repro"
  [ "$status" -eq 1 ]
  result="${lines[${#lines[@]} - 1]}"
  [ "$(field -c .summary)" = '{"total":1,"passed":0,"failed":1,"skipped":0}' ]
}

@test "core-test fails when a filter matches no tests" {
  [ "${MG_CONTAINER:-}" = dev ] || skip "needs the dev container"
  write_workspace
  write_crate fx '#[cfg(test)] mod tests { #[test] fn adds() { assert_eq!(1 + 1, 2); } }'
  mg core-test -- -E 'package(fx) & test(=tests::renamed)'
  [ "$status" -ne 0 ]
  [ "$(field .status)" != pass ]
  [ "$(field .summary.total)" -eq 0 ]
  [ "$(field '.failures[0].id')" = no-tests-matched ]
  validate "$result"
}

@test "core-test reports a build failure as a failed run" {
  [ "${MG_CONTAINER:-}" = dev ] || skip "needs the dev container"
  write_workspace
  write_crate broken 'pub fn f() -> u32 { "not a number" }'
  mg core-test
  [ "$status" -eq 1 ]
  [ "$(field .status)" = fail ]
  [[ "$(field '.failures[0].id')" == nextest-exit-* ]]
  validate "$result"
}

# --- images ----------------------------------------------------------------

@test "dev-image reports an error when image inspection fails or returns an empty ID" {
  local stubs="$BATS_TEST_TMPDIR/image-stubs" image mode
  mkdir -p "$stubs"
  cat >"$stubs/docker" <<'EOF'
#!/usr/bin/env bash
case "$*" in
  info | compose*" build dev" | build*) exit 0 ;;
  "image inspect "*)
    if [[ "$3" == *"-$STUB_BAD_IMAGE" ]]; then
      [ "$STUB_INSPECT_MODE" = empty ] && exit 0
      echo sha256:fixture
      echo "image inspection failed" >&2
      exit 1
    fi
    echo sha256:fixture ;;
  *) exit 97 ;;
esac
EOF
  chmod +x "$stubs/docker"
  export PATH="$stubs:$PATH"
  for image in dev fuzz; do
    for mode in fail empty; do
      export STUB_BAD_IMAGE="$image" STUB_INSPECT_MODE="$mode"
      mg dev-image
      [ "$status" -eq 2 ]
      [ "$(field .status)" = error ]
      [ "$(field .exit_code)" -eq 2 ]
      [ "$(field '.failures[0].id')" = image-inspect ]
      validate "$result"
    done
  done
}

@test "every Docker base image is pinned by digest and the dev image matches the Rust toolchain file" {
  local f line image stages
  for f in "$SB"/docker/*.Dockerfile; do
    stages=" "
    while read -r line; do
      image="$(awk '{print $2}' <<<"$line")"
      if [[ "$stages" != *" $image "* ]] && [ "$image" != scratch ]; then
        [[ "$image" =~ @sha256:[0-9a-f]{64}$ ]] || { echo "$f: unpinned base image $image"; return 1; }
      fi
      [[ "$line" =~ [[:space:]][Aa][Ss][[:space:]]+([^[:space:]]+) ]] && stages="$stages${BASH_REMATCH[1]} "
    done < <(grep -E '^FROM[[:space:]]' "$f")
  done
  local base channel
  base="$(sed -n 's/^FROM rust:\([0-9.]*\)-slim@.*/\1/p' "$SB/docker/dev.Dockerfile" | head -n 1)"
  channel="$(sed -n 's/^channel = "\(.*\)"/\1/p' "$SB/core/rust-toolchain.toml")"
  [ -n "$base" ]
  [ "${channel%.*}" = "$base" ]
}

@test "every Dockerfile's build context is an allowlist that starts by excluding everything" {
  local f ignore first
  for f in "$SB"/docker/*.Dockerfile; do
    ignore="$f.dockerignore"
    [ -f "$ignore" ] || { echo "$f has no $(basename "$ignore"); the repo root holds local-data/"; return 1; }
    first="$(grep -Ev '^[[:space:]]*(#|$)' "$ignore" | head -n 1)"
    [ "$first" = "*" ] || { echo "$ignore must start with '*', found '$first'"; return 1; }
    if grep -Eq '^!.*(local-data|refs)' "$ignore"; then
      echo "$ignore re-includes a private path"
      return 1
    fi
  done
}
