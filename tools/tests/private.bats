#!/usr/bin/env bats
# Self-tests for the private collection vault's jobs and hook (U15, R1938–
# R1943): `tools/mg private status|copy|suite|scan-outputs` and
# tools/hooks/pre-commit. Docker is faked: the fake runs the private
# service's command here, with /vault, /work and meguri-cli mapped, and a
# fake meguri-cli whose "collection" is a candidates file (meguri-cli itself
# is tested in core/; the scanner's unit tests run from here, since they need
# git). The real copy
# through a read-only mount is a manual run (see the U15 review log). Every
# collection here is made up.

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
  SB="$(cd "$SB" && pwd -P)"
  git -C "$SB" config user.name selftest
  git -C "$SB" config user.email selftest@localhost
  export SB STUB_LOG="$BATS_TEST_TMPDIR/stub.log" FAKE_CLI="$BATS_TEST_TMPDIR/meguri-cli"
  : >"$STUB_LOG"
  for v in CI CONTINUOUS_INTEGRATION BUILDKITE BITBUCKET_BUILD_NUMBER CIRCLECI CI_XCODE_PROJECT CODEBUILD_BUILD_ID DRONE GITHUB_ACTIONS GITLAB_CI JENKINS_URL TEAMCITY_VERSION TF_BUILD TRAVIS; do
    unset "$v"
  done
  BIN="$BATS_TEST_TMPDIR/bin"
  mkdir -p "$BIN"
  fake_docker
  fake_cli
  export PATH="$BIN:$PATH"
}

teardown() {
  [ -z "${result:-}" ] || [ ! -f "$result" ] || jq -c '{status, message, failures}' "$result"
  [ -z "${stderr:-}" ] || printf '%s\n' "$stderr" | tail -n 20
}

mg() {
  run --separate-stderr "$SB/tools/mg" "$@"
  result="${lines[${#lines[@]} - 1]:-}"
}

field() { jq -r "$@" "$result"; }

# docker: logs every call; compose `run` of the private service runs its
# `sh -c` command here, /vault mapped to the -v source, /work to the sandbox
# and meguri-cli to the fake.
fake_docker() {
  cat >"$BIN/docker" <<'EOF'
#!/bin/bash
echo "docker $*" >>"$STUB_LOG"
[ "$1" = compose ] || exit 0
shift
while [ $# -gt 0 ]; do
  case "$1" in -p | -f | --progress) shift 2 ;; *) break ;; esac
done
sub="$1"
shift
[ "$sub" = run ] || exit 0
vault=none
while [ $# -gt 0 ]; do
  case "$1" in
    --rm | -T | --no-deps) shift ;;
    --label | -e) shift 2 ;;
    -v)
      case "$2" in *:/vault:ro) vault="${2%:/vault:ro}" ;; *) vault="BAD:$2" ;; esac
      shift 2
      ;;
    *) break ;;
  esac
done
service="$1"
shift
echo "service $service vault=$vault" >>"$STUB_LOG"
[ "$service" = private ] || exit 0
[ "$1" = sh ] && [ "$2" = -c ] || exit 90
script="$3"
shift 3
mapped=()
for a in "$@"; do
  case "$a" in
    /cargo-target/fixtures/meguri-cli) a="$FAKE_CLI" ;;
    /vault) a="$vault" ;;
    /work/*) a="$SB/${a#/work/}" ;;
  esac
  mapped+=("$a")
done
exec sh -c "$script" "${mapped[@]}"
EOF
  chmod +x "$BIN/docker"
}

# meguri-cli private: copy copies the collection file and media folder;
# tokens hands back the "collection" itself, a candidates file here; suite
# writes $FAKE_SUMMARY (default: no checks) and exits $FAKE_SUITE_RC.
fake_cli() {
  cat >"$FAKE_CLI" <<'EOF'
#!/bin/bash
echo "meguri-cli $*" >>"$STUB_LOG"
[ "$1" = private ] || exit 2
cmd="$2"
shift 2
while [ $# -gt 0 ]; do
  case "$1" in
    --source) src="$2" ;; --out) out="$2" ;; --access-log) access="$2" ;; --copy) copy="$2" ;; --log) log="$2" ;;
  esac
  shift 2
done
case "$cmd" in
  copy)
    mkdir "$out" && cp "$src/collection.anki2" "$out/" || exit 1
    [ ! -d "$src/collection.media" ] || cp -R "$src/collection.media" "$out/"
    printf 'collection.anki2\n' >"$access"
    printf '{"collection": {"bytes": %s, "sha256": "%s"}, "wal": null, "media": {"files": 1, "bytes": 3, "skipped": 0}, "opened": 3}\n' \
      "$(wc -c <"$src/collection.anki2" | tr -d ' ')" "$(sha256sum "$src/collection.anki2" | cut -c1-64)"
    ;;
  tokens) cp "$copy/collection.anki2" "$out" && echo '{"decks": 1, "tags": 0, "media": 0, "words": 2}' ;;
  suite)
    summary="${FAKE_SUMMARY:-}"
    [ -n "$summary" ] || summary='{"checks": {}, "report_version": 1}'
    printf '%s\n' "$summary" >"$out"
    : >"$log"
    exit "${FAKE_SUITE_RC:-0}"
    ;;
  *) exit 2 ;;
esac
EOF
  chmod +x "$FAKE_CLI"
}

# A made-up profile folder: its "collection" is the candidates the fake
# meguri-cli lists. Its strings carry a random part ($R), so no file of the
# sandbox repository holds them (this file is in it); `docker` is in the
# repository, so it is not private.
make_source() {
  R="$(LC_ALL=C tr -dc 'a-z' </dev/urandom | head -c 10)"
  DECK="Zeb${R}fink::Hör${R}"
  local src="$SB/local-data/anki"
  mkdir -p "$src/collection.media"
  printf '%s\n' \
    "{\"kind\": \"deck\", \"text\": \"$DECK\"}" \
    "{\"kind\": \"word\", \"text\": \"Quokka$R\"}" \
    '{"kind": "word", "text": "docker"}' >"$src/collection.anki2"
  printf 'img' >"$src/collection.media/a.jpg"
  printf 'secret' >"$src/prefs21.db"
}

no_private_text() { # <file>...: none holds a string of the made-up collection
  [ -n "${R:-}" ] && ! grep -qi "$R" "$@"
}

@test "private copy: copies through the network-less service with the source read-only and rebuilds the token set" {
  make_source
  local before
  before="$(sha256sum "$SB/local-data/anki/collection.anki2")"
  mg private copy
  [ "$status" -eq 0 ]
  [ "$(field .status)" = pass ]
  local run copy
  run="$(field .run_id)"
  copy="$SB/build/private/$run/copy"
  [ "$(field .details.copy.path)" = "build/private/$run/copy" ]
  cmp "$copy/collection.anki2" "$SB/local-data/anki/collection.anki2"
  [ -f "$copy/collection.media/a.jpg" ]
  [ ! -e "$copy/prefs21.db" ]
  [ "$(sha256sum "$SB/local-data/anki/collection.anki2")" = "$before" ]
  # Built in dev without the collection; run in private with it at /vault.
  grep -q "service dev vault=none" "$STUB_LOG"
  [ "$(grep -c "service private vault=$SB/local-data/anki$" "$STUB_LOG")" -eq 2 ]
  grep -q "compose .* build dev" "$STUB_LOG"
  grep -q "compose .* build private" "$STUB_LOG"
  ! grep -q 'BAD:' "$STUB_LOG"
  # The token set: the private strings, not what the repository holds.
  local tokens="$SB/build/private/tokens/tokens.txt"
  [ "$(cat "$tokens")" = "$(printf 'quokka%s\nzeb%sfink hör%s' "$R" "$R" "$R")" ]
  [ "$(field .details.token_set.tokens)" -eq 2 ]
  [ "$(field .details.token_set.in_repository)" -eq 1 ]
  [ ! -e "$SB/build/private/$run/candidates.jsonl" ]
  no_private_text "$result" "$SB/build/mg/private/$run/log.txt"
}

@test "private status, copy and suite refuse when a CI marker is set; scan-outputs does not" {
  make_source
  for marker in CI GITHUB_ACTIONS JENKINS_URL; do
    for command in status copy suite; do
      run --separate-stderr env "$marker=true" "$SB/tools/mg" private "$command"
      result="${lines[${#lines[@]} - 1]}"
      [ "$status" -eq 2 ]
      [ "$(field '.failures[0].id')" = ci-marker ]
      [[ "$(field '.failures[0].message')" == *"$marker is set"* ]]
    done
  done
  ! grep -q docker "$STUB_LOG"
  [ ! -d "$SB/build/private" ]
  CI=true mg private scan-outputs
  [ "$(field '.failures[0].id')" = no-token-set ]
}

@test "private refuses implementer and fixer worktrees and a checkout without the collection" {
  mg private copy
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = no-source ]
  make_source
  for role in implementer fixer; do
    printf '{"role": "%s"}\n' "$role" >"$SB/.git/meguri-session.json"
    mg private copy
    [ "$status" -eq 2 ]
    [ "$(field '.failures[0].id')" = implementer-worktree ]
  done
  ! grep -q docker "$STUB_LOG"
}

@test "private status reports the source, the copies and the token set without opening the collection" {
  make_source
  mg private status
  [ "$(field .status)" = pass ]
  [ "$(field .details.source.present)" = true ]
  [ "$(field .details.source.collection_bytes)" -eq "$(wc -c <"$SB/local-data/anki/collection.anki2")" ]
  [ "$(field .details.source.media_folder)" = true ]
  [ "$(field .details.token_set)" = null ]
  mg private copy
  local run
  run="$(field .run_id)"
  : >"$STUB_LOG"
  mg private status
  [ "$(field -c '.details.copies')" = "[\"build/private/$run\"]" ]
  [ "$(field .details.token_set.tokens)" -eq 2 ]
  # No container, so nothing opened the collection.
  [ ! -s "$STUB_LOG" ]
}

@test "private suite: records a scanned summary of pass counts and deletes its copy" {
  make_source
  mg private suite --record M1
  [ "$status" -eq 0 ]
  [ "$(field .status)" = warn ]
  [[ "$(field .message)" == *"no check"* ]]
  [ "$(jq -c . "$SB/conformance/reports/private/M1.json")" = '{"checks":{},"report_version":1}' ]
  [ ! -e "$SB/build/private/$(field .run_id)/copy" ]

  export FAKE_SUMMARY='{"checks": {"next-states": {"failed": 0, "passed": 12}, "render": {"failed": 2, "passed": 7}}, "report_version": 1}' FAKE_SUITE_RC=1
  mg private suite --keep
  [ "$(field .status)" = fail ]
  [ "$(field '[.failures[].id] | join(",")')" = "check:render" ]
  [ "$(field .details.summary.checks.render.failed)" -eq 2 ]
  [ -d "$SB/build/private/$(field .run_id)/copy" ]
}

@test "private suite: a summary holding a private token fails and nothing is recorded" {
  make_source
  export FAKE_SUMMARY="{\"checks\": {\"queues\": {\"failed\": 0, \"passed\": 1}}, \"deck\": \"$DECK\", \"report_version\": 1}"
  mg private suite --record M2
  [ "$(field .status)" = fail ]
  [ "$(field '.failures[0].id')" = summary-private-token ]
  [ ! -e "$SB/conformance/reports/private/M2.json" ]
  no_private_text "$result"
  mg private suite --record '../M3'
  [ "$(field '.failures[0].id')" = usage ]
}

@test "private scan-outputs: a result JSON naming a private deck fails, naming the file but not the token" {
  R=rverstehen # what no_private_text looks for
  mkdir -p "$SB/build/private/tokens" "$SB/out"
  printf 'hörverstehen\nzebrafink hörverstehen\n' >"$SB/build/private/tokens/tokens.txt"
  printf '{\n  "passed": 3,\n  "deck": "Zebrafink::Hörverstehen"\n}\n' >"$SB/out/result.json"
  printf '{"passed": 3}\n' >"$SB/out/clean.json"
  mg private scan-outputs out/result.json out/clean.json
  [ "$status" -eq 1 ]
  [ "$(field '.failures[0].id')" = "private-token:out/result.json" ]
  [[ "$(field '.failures[0].message')" == *"line 3"* ]]
  [ "$(field '.failures[0].repro')" = "tools/mg private scan-outputs out/result.json" ]
  [ "$(field .summary.total)" -eq 2 ]
  no_private_text "$result" "$SB/build/mg/private/$(field .run_id)/log.txt"
  mg private scan-outputs out/clean.json
  [ "$(field .status)" = pass ]
  # Without paths: the staged lines.
  git -C "$SB" add out/result.json
  mg private scan-outputs
  [ "$(field '.failures[0].id')" = "private-token:out/result.json" ]
}

@test "pre-commit hook: blocks a commit adding a private token, naming the file only; idle without a token set" {
  mg hooks install
  [ "$(field .status)" = pass ]
  grep -q 'tools/hooks/pre-commit' "$SB/.git/hooks/pre-commit"
  grep -q 'tools/hooks/commit-msg' "$SB/.git/hooks/commit-msg"

  printf 'Zebrafink::Hörverstehen\n' >"$SB/notes.txt"
  git -C "$SB" add notes.txt
  run --separate-stderr git -C "$SB" commit -qm "no token set yet"
  [ "$status" -eq 0 ]

  mkdir -p "$SB/build/private/tokens"
  printf 'quokkasprung\nzebrafink hörverstehen\n' >"$SB/build/private/tokens/tokens.txt"
  printf 'fine\nQuokkasprung!\n' >"$SB/report.json"
  git -C "$SB" add report.json
  run --separate-stderr git -C "$SB" commit -qm "add report"
  [ "$status" -ne 0 ]
  [[ "$stderr" == *"report.json (line 2)"* ]]
  ! grep -qi quokka <<<"$stderr"
  [ -z "$(git -C "$SB" log -1 --format=%s | grep 'add report')" ]

  # A linked worktree's commits are checked against the main checkout's set.
  git -C "$SB" reset -q
  git -C "$SB" worktree add -q "$BATS_TEST_TMPDIR/wt" -b other
  printf 'quokkasprung\n' >"$BATS_TEST_TMPDIR/wt/x.txt"
  git -C "$BATS_TEST_TMPDIR/wt" add x.txt
  run --separate-stderr git -C "$BATS_TEST_TMPDIR/wt" commit -qm "in a worktree"
  [ "$status" -ne 0 ]
  [[ "$stderr" == *"x.txt (line 1)"* ]]
  printf 'harmless\n' >"$BATS_TEST_TMPDIR/wt/x.txt"
  git -C "$BATS_TEST_TMPDIR/wt" add x.txt
  run --separate-stderr git -C "$BATS_TEST_TMPDIR/wt" commit -qm "in a worktree"
  [ "$status" -eq 0 ]
}

@test "hooks install refuses a foreign pre-commit hook" {
  printf '#!/bin/sh\nexit 0\n' >"$SB/.git/hooks/pre-commit"
  chmod +x "$SB/.git/hooks/pre-commit"
  mg hooks install
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = hook-exists ]
  [[ "$(field '.failures[0].message')" == *"/hooks/pre-commit exists"* ]]
}

@test "the job's CI markers are the vault's" {
  local rust bash
  rust="$(sed -n '/ci-markers:begin/,/ci-markers:end/p' "$REPO_ROOT/core/crates/meguri-testkit/src/private/vault.rs" |
    grep -oE '"[A-Z_]+"' | tr -d '"' | sort | tr '\n' ' ')"
  bash="$(sed -n 's/^MG_PRIVATE_CI_MARKERS="\(.*\)"$/\1/p' "$SB/tools/mg.d/private.sh" | tr ' ' '\n' | sort | tr '\n' ' ')"
  [ -n "$rust" ]
  [ "$rust" = "$bash" ]
}

@test "the private service has no network, a read-only target volume and local-data hidden" {
  python3 - "$SB/docker/compose.yml" <<'EOF'
import sys, yaml
s = yaml.safe_load(open(sys.argv[1]))["services"]["private"]
assert s["network_mode"] == "none", s
assert s["volumes"] == ["..:/work", "cargo-target:/cargo-target:ro"], s["volumes"]
assert any(t.split(":")[0] == "/work/local-data" for t in s["tmpfs"]), s["tmpfs"]
assert "ports" not in s and "networks" not in s, s
EOF
}

@test "the scanner's unit tests pass (tools/scripts/private-scan/tests)" {
  PYTHONDONTWRITEBYTECODE=1 run python3 -m unittest discover -s "$REPO_ROOT/tools/scripts/private-scan/tests" -t "$REPO_ROOT/tools/scripts/private-scan/tests"
  printf '%s\n' "$output" | tail -n 5
  [ "$status" -eq 0 ]
  [[ "$output" == *"OK"* ]]
}
