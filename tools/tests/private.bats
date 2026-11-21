#!/usr/bin/env bats
# Self-tests for the private collection vault's jobs and hook (U15, R1938–
# R1943): `tools/mg private status|copy|suite|scan-outputs|pins` and
# tools/hooks/pre-commit. Docker is faked: the fake runs the private
# service's command here, with /vault, /work and meguri-cli mapped, and a
# fake meguri-cli whose "collection" is a candidates file (meguri-cli itself
# is tested in core/; the scanner's unit tests run from here, since they need
# git). The real copy through the read-only mount is tools/mg
# private-selftest; the real anonymizer and oracle on a generated collection
# are tools/mg anonymize-check. Every collection here is made up.

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
# anonymize: fails with $FAKE_ANON_KIND, or writes a clone (the source's
# file) and $FAKE_ANON_REPORT (default: every check passes), and makes the
# salt file when it is missing.
if [ "$1" = anonymize ]; then
  shift
  while [ $# -gt 0 ]; do
    case "$1" in --copy) copy="$2" ;; --out) out="$2" ;; --salt-file) salt="$2" ;; esac
    shift 2
  done
  if [ -n "${FAKE_ANON_KIND:-}" ]; then
    echo "meguri-cli anonymize: $FAKE_ANON_KIND: made up" >&2
    exit 1
  fi
  [ -f "$copy/collection.anki2" ] || exit 3
  [ -f "$salt" ] || printf '%064d\n' 0 >"$salt"
  mkdir -p "$out" && cp "$copy/collection.anki2" "$out/collection.anki2" || exit 4
  report="${FAKE_ANON_REPORT:-}"
  [ -n "$report" ] || report='{"checks": {"leak": {"locations": [], "status": "pass"}, "media": "pass", "rows": "pass", "structure": "pass"}, "format": 1, "notes": 3}'
  printf '%s\n' "$report" >"$out/anonymize.json"
  echo "$out/anonymize.json"
  exit 0
fi
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
  # From now on a missing set blocks commits, and the set names its baseline.
  [ "$(git -C "$SB" config --get meguri.privateTokenSet)" = required ]
  [ "$(jq -r .head "$SB/build/private/tokens/baseline.json")" = "$(git -C "$SB" rev-parse HEAD)" ]
  [ "$(jq -r .by "$SB/build/private/tokens/baseline.json")" = build ]
  [ "$(jq -r .sha256 "$SB/build/private/tokens/baseline.json")" = "$(sha256sum "$tokens" | cut -c1-64)" ]
  no_private_text "$SB/build/private/tokens/baseline.json"
  # The hooks that check commits and merges against the token set.
  grep -q 'tools/hooks/pre-commit' "$SB/.git/hooks/pre-commit"
  grep -q 'tools/hooks/pre-merge-commit' "$SB/.git/hooks/pre-merge-commit"
  [ "$(field -c '[.details.hooks[].name]')" = '["commit-msg","pre-commit","pre-merge-commit"]' ]
  no_private_text "$result" "$SB/build/mg/private/$run/log.txt"
}

@test "private copy, suite and anonymize build no token set where the hooks that check commits cannot be installed" {
  make_source
  printf '#!/bin/sh\nexit 0\n' >"$SB/.git/hooks/pre-commit"
  chmod +x "$SB/.git/hooks/pre-commit"
  local command
  for command in "private copy" "private suite" "anonymize"; do
    # shellcheck disable=SC2086 # the job and its command, as two words
    mg $command
    [ "$status" -eq 2 ]
    [ "$(field '.failures[0].id')" = hook-exists ]
    [[ "$(field '.failures[0].message')" == *"/hooks/pre-commit exists"* ]]
    [ "$(field '.failures[0].repro')" = "tools/mg hooks install" ]
  done
  rm "$SB/.git/hooks/pre-commit"
  git -C "$SB" config core.hooksPath .githooks
  mg private copy
  [ "$(field '.failures[0].id')" = hooks-path ]
  ! grep -q docker "$STUB_LOG"
  [ ! -e "$SB/build/private/tokens/tokens.txt" ]
  # anonymize --source reads no private collection and builds no token set.
  git -C "$SB" config --unset core.hooksPath
  printf '#!/bin/sh\nexit 0\n' >"$SB/.git/hooks/pre-commit"
  anonymize_sandbox
  mg anonymize --source build/fixtures/s-1-10k
  [ "$(field .status)" = pass ]
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

@test "pre-commit hook: where a token set is expected, a missing, empty or unreadable one blocks the commit, in every worktree" {
  mg hooks install
  git -C "$SB" worktree add -q "$BATS_TEST_TMPDIR/wt" -b other
  local set="$SB/build/private/tokens/tokens.txt" n=0 where
  try_commit() { # <checkout>: stages one harmless new file and commits it
    n=$((n + 1))
    printf 'harmless %s\n' "$n" >"$1/harmless-$n.txt"
    git -C "$1" add "harmless-$n.txt"
    run --separate-stderr git -C "$1" commit -qm "harmless $n"
  }
  # A machine without the collection: nothing to check against.
  for where in "$SB" "$BATS_TEST_TMPDIR/wt"; do
    try_commit "$where"
    [ "$status" -eq 0 ]
  done
  # What `private copy` records: from then on a missing set is a refusal.
  git -C "$SB" config meguri.privateTokenSet required
  for where in "$SB" "$BATS_TEST_TMPDIR/wt"; do
    try_commit "$where"
    [ "$status" -ne 0 ]
    [[ "$stderr" == *"expected to have a private token set"* ]]
    [[ "$stderr" == *"$set"* ]]
    git -C "$where" reset -q
  done
  # The collection's folder in the main checkout means the same, unrecorded.
  git -C "$SB" config --unset meguri.privateTokenSet
  mkdir -p "$SB/local-data/anki"
  try_commit "$BATS_TEST_TMPDIR/wt"
  [ "$status" -ne 0 ]
  git -C "$BATS_TEST_TMPDIR/wt" reset -q
  rmdir "$SB/local-data/anki" "$SB/local-data"
  # A set that is there but empty, unreadable or no regular file: refused
  # even where none is expected.
  mkdir -p "$(dirname "$set")"
  : >"$set"
  try_commit "$BATS_TEST_TMPDIR/wt"
  [ "$status" -ne 0 ]
  [[ "$stderr" == *"not a readable, non-empty regular file"* ]]
  git -C "$BATS_TEST_TMPDIR/wt" reset -q
  printf 'quokkasprung\n' >"$set"
  if [ "$(id -u)" -ne 0 ]; then # root reads through any mode
    chmod 000 "$set"
    try_commit "$SB"
    [ "$status" -ne 0 ]
    [[ "$stderr" == *"not a readable, non-empty regular file"* ]]
    git -C "$SB" reset -q
    mg private scan-outputs tools/mg
    [ "$(field .status)" = error ]
    [ "$(field '.failures[0].id')" = token-set-unreadable ]
    chmod 600 "$set"
  fi
  mv "$set" "$set.real"
  ln -s "$set.real" "$set"
  try_commit "$SB"
  [ "$status" -ne 0 ]
  git -C "$SB" reset -q
  rm "$set"
  mv "$set.real" "$set"
  # Readable again: both checkouts commit, and both are scanned.
  for where in "$SB" "$BATS_TEST_TMPDIR/wt"; do
    try_commit "$where"
    [ "$status" -eq 0 ]
    printf 'Quokkasprung\n' >"$where/leak.txt"
    git -C "$where" add leak.txt
    run --separate-stderr git -C "$where" commit -qm leak
    [ "$status" -ne 0 ]
    [[ "$stderr" == *"leak.txt (line 1)"* ]]
    ! grep -qi quokka <<<"$stderr"
    git -C "$where" reset -q
  done
}

@test "a reviewed public file edited since its review: the same answer in a new and an old worktree, marked as stale, and listed by private pins" {
  mg hooks install
  # A made-up reviewed file and its pin, committed like a real admission.
  mkdir -p "$SB/docs" "$SB/build/private/tokens"
  printf 'Quokkasprung was reviewed as public\nsecond line\n' >"$SB/docs/reviewed.md"
  local pin
  pin="$(sha256sum "$SB/docs/reviewed.md" | cut -c1-64)"
  python3 - "$SB/tools/scripts/private-scan/private_scan.py" "$pin" <<'PY'
import sys
path, pin = sys.argv[1:]
source = open(path, encoding="utf-8").read()
marker = "PUBLIC_ARTIFACTS = {\n"
assert source.count(marker) == 1
open(path, "w", encoding="utf-8").write(source.replace(marker, marker + "    'docs/reviewed.md': '%s',\n" % pin))
PY
  git -C "$SB" branch old-base
  printf 'quokkasprung\n' >"$SB/build/private/tokens/tokens.txt"
  git -C "$SB" add -A
  git -C "$SB" commit -qm "admit the reviewed file"
  # (The sandbox's own tools/ files have pins too; only this one is asked.)
  stale() { jq -c '.stale | index("docs/reviewed.md") != null' "$SB/build/mg/private/$(field .run_id)/pins.json"; }
  mg private pins
  [ "$(stale)" = false ]
  [ "$(field .details.pins.commit)" = "$(git -C "$SB" rev-parse HEAD)" ]
  mg private scan-outputs docs/reviewed.md
  [ "$(field .status)" = pass ]
  # An ordinary edit to another line passes the hook and leaves the pin stale.
  printf 'third line\n' >>"$SB/docs/reviewed.md"
  git -C "$SB" add docs/reviewed.md
  git -C "$SB" commit -qm "edit another line"
  mg private pins
  [ "$(field .status)" = warn ]
  [ "$(stale)" = true ]
  # Whole-file scan, a worktree at the new head and one at the old base all
  # flag the reviewed line and say why; none passes silently.
  mg private scan-outputs docs/reviewed.md
  [ "$(field .status)" = fail ]
  [ "$(jq -c '.files[0].stale_public_artifact' "$SB/build/mg/private/$(field .run_id)/scan.json")" = true ]
  git -C "$SB" worktree add -q "$BATS_TEST_TMPDIR/new" -b new-base
  git -C "$SB" worktree add -q "$BATS_TEST_TMPDIR/old" old-base
  run --separate-stderr git -C "$BATS_TEST_TMPDIR/old" merge -q --no-edit --no-ff new-base
  [ "$status" -ne 0 ]
  [[ "$stderr" == *"docs/reviewed.md (line 1) [reviewed public file whose bytes changed since its review]"* ]]
  [[ "$stderr" == *"one-answer-in-every-checkout"* ]]
  ! grep -qi quokka <<<"$stderr"
  run --separate-stderr "$BATS_TEST_TMPDIR/new/tools/mg" private scan-outputs docs/reviewed.md
  [ "$(jq -r .status "${lines[${#lines[@]} - 1]}")" = fail ]
  # The documented repair: the owner drops what the head already publishes
  # from the one shared set; the old worktree's merge then passes unchanged.
  git -C "$BATS_TEST_TMPDIR/old" merge --abort 2>/dev/null || git -C "$BATS_TEST_TMPDIR/old" reset -q --hard
  chmod 700 "$SB/build/private"
  chmod 600 "$SB/build/private/tokens/tokens.txt"
  # A random word no file of the sandbox holds, so the set is not emptied.
  printf 'madeup%s\n' "$(LC_ALL=C tr -dc 'a-z' </dev/urandom | head -c 10)" >>"$SB/build/private/tokens/tokens.txt"
  mg private refresh-public --head "$(git -C "$SB" rev-parse HEAD)"
  [ "$(field .status)" = pass ]
  [ "$(field .details.refresh_public.removed)" -eq 1 ]
  [ "$(jq -r .head "$SB/build/private/tokens/baseline.json")" = "$(git -C "$SB" rev-parse HEAD)" ]
  [ "$(jq -r .by "$SB/build/private/tokens/baseline.json")" = refresh-public ]
  [ "$(jq -r .sha256 "$SB/build/private/tokens/baseline.json")" = "$(sha256sum "$SB/build/private/tokens/tokens.txt" | cut -c1-64)" ]
  run --separate-stderr git -C "$BATS_TEST_TMPDIR/old" merge -q --no-edit --no-ff new-base
  [ "$status" -eq 0 ]
  mg private scan-outputs docs/reviewed.md
  [ "$(field .status)" = pass ]
}

@test "pre-commit hook: a merge's conflict resolution and an automatic merge are scanned too" {
  mg hooks install
  grep -q 'tools/hooks/pre-merge-commit' "$SB/.git/hooks/pre-merge-commit"
  [ -x "$SB/.git/hooks/pre-merge-commit" ]
  mkdir -p "$SB/build/private/tokens"
  printf 'quokkasprung\n' >"$SB/build/private/tokens/tokens.txt"
  local main
  main="$(git -C "$SB" symbolic-ref --short HEAD)"
  printf 'base\n' >"$SB/m.txt"
  git -C "$SB" add m.txt
  git -C "$SB" commit -qm base
  git -C "$SB" checkout -qb side
  printf 'side\n' >"$SB/m.txt"
  git -C "$SB" commit -qam side
  git -C "$SB" checkout -q "$main"
  printf 'main\n' >"$SB/m.txt"
  git -C "$SB" commit -qam main

  # A conflict resolved with a line neither parent had.
  run git -C "$SB" merge --no-edit side
  [ "$status" -ne 0 ]
  printf 'Quokkasprung resolved\n' >"$SB/m.txt"
  git -C "$SB" add m.txt
  run --separate-stderr git -C "$SB" commit --no-edit
  [ "$status" -ne 0 ]
  [[ "$stderr" == *"m.txt (line 1)"* ]]
  ! grep -qi quokka <<<"$stderr"
  [ -f "$SB/.git/MERGE_HEAD" ]
  printf 'resolved\n' >"$SB/m.txt"
  git -C "$SB" add m.txt
  run --separate-stderr git -C "$SB" commit --no-edit
  [ "$status" -eq 0 ]
  [ "$(git -C "$SB" rev-list --parents -n 1 HEAD | wc -w)" -eq 3 ]

  # An automatic merge (git merge runs pre-merge-commit, not pre-commit)
  # bringing a line that was committed without the hook.
  git -C "$SB" checkout -qb side2
  printf 'Quokkasprung\n' >"$SB/n.txt"
  git -C "$SB" add n.txt
  git -C "$SB" commit -q --no-verify -m side2
  git -C "$SB" checkout -q "$main"
  printf 'other\n' >"$SB/o.txt"
  git -C "$SB" add o.txt
  git -C "$SB" commit -qm other
  local before
  before="$(git -C "$SB" rev-parse HEAD)"
  run --separate-stderr git -C "$SB" merge --no-edit side2
  [ "$status" -ne 0 ]
  [[ "$stderr" == *"n.txt (line 1)"* ]]
  ! grep -qi quokka <<<"$stderr"
  [ "$(git -C "$SB" rev-parse HEAD)" = "$before" ]
  git -C "$SB" merge --abort
  # Without the token, the same kind of merge goes through.
  git -C "$SB" checkout -q side2
  printf 'harmless\n' >"$SB/n.txt"
  git -C "$SB" commit -qam fixed
  git -C "$SB" checkout -q "$main"
  run --separate-stderr git -C "$SB" merge --no-edit side2
  [ "$status" -eq 0 ]
  [ "$(git -C "$SB" rev-list --parents -n 1 HEAD | wc -w)" -eq 3 ]
}

@test "hooks install refuses a foreign pre-commit hook" {
  printf '#!/bin/sh\nexit 0\n' >"$SB/.git/hooks/pre-commit"
  chmod +x "$SB/.git/hooks/pre-commit"
  mg hooks install
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = hook-exists ]
  [[ "$(field '.failures[0].message')" == *"/hooks/pre-commit exists"* ]]
}

# --- anonymize (meguri-cli and the oracle job faked) ------------------------

# anonymize_sandbox: a made-up fixture directory with a manifest, and a stub
# for the oracle job that reports $STUB_DIR/<basename of the dir>.json (or
# cannot run when there is none). Both reports start equal.
anonymize_sandbox() {
  export STUB_DIR="$BATS_TEST_TMPDIR/oracle"
  mkdir -p "$STUB_DIR"
  cat >"$SB/tools/mg.d/zz-stub.sh" <<'STUB'
mg_oracle() {
  echo "oracle $*" >>"$STUB_LOG"
  local report="$STUB_DIR/$(basename "$2").json"
  [ -f "$report" ] || { mg_failure oracle-tree-differs "made up: no oracle here"; mg_status error; return 0; }
  mg_detail check "$(jq -c . "$report")"
  # K9: a Check Database problem fails the job; the report is still there.
  if jq -e '.check_db | has("error") or any(.[]; . != 0)' "$report" >/dev/null; then
    mg_status fail
  else
    mg_status pass
  fi
}
STUB
  FIX="$SB/build/fixtures/s-1-10k"
  mkdir -p "$FIX/collection.media"
  printf 'a made-up collection' >"$FIX/collection.anki2"
  printf '{"now_ms": 1894701600123, "utc_offset_secs": 3600}\n' >"$FIX/manifest.json"
  REPORT='{"changed_tables": ["col", "config"], "check_db": {"future_ids": 0}, "counts": {"cards": 10}, "now": 1894701600, "timing": {"days_elapsed": 500}, "tz": "Etc/GMT-1", "schedule": {"cards": 10, "decks": {"1": {"cards": 4, "next_states_sha256": "a1"}, "77": {"cards": 6, "next_states_sha256": "b2"}}, "root": {"new": 1}, "sha256": "aaa"}}'
  printf '%s\n' "$REPORT" >"$STUB_DIR/s-1-10k.json"
  printf '%s\n' "$REPORT" >"$STUB_DIR/clone.json"
}

@test "anonymize: a fixture's clone through the network-less service, then V3 on both at the manifest's instant" {
  anonymize_sandbox
  mg anonymize --source build/fixtures/s-1-10k
  [ "$status" -eq 0 ]
  [ "$(field .status)" = pass ]
  local run clone
  run="$(field .run_id)"
  clone="$SB/build/private/$run/clone"
  [ -f "$clone/collection.anki2" ]
  [ "$(field .details.clone.path)" = "build/private/$run/clone" ]
  [ "$(field .details.report.notes)" -eq 3 ]
  [ "$(field .details.v3.equal)" = true ]
  [[ "$(field '[.artifacts[] | select(.kind == "clone") | .path] | first')" == *"build/private/$run/clone" ]]
  # The source read-only at /vault; the clone and the salt under build/.
  grep -q "service private vault=$FIX$" "$STUB_LOG"
  grep -q "meguri-cli anonymize --copy $FIX --out $clone --salt-file $SB/build/private/anonymize.salt" "$STUB_LOG"
  [ -f "$SB/build/private/anonymize.salt" ]
  # K1's defaults from the manifest, given to both checks.
  [ "$(grep -c '^oracle check .* --schedule --now 2030-01-15T10:00:00Z --tz Etc/GMT-1$' "$STUB_LOG")" -eq 2 ]
  grep -q "^oracle check $FIX " "$STUB_LOG"
  grep -q "^oracle check $clone " "$STUB_LOG"
  [ "$(field .summary.total)" -eq 5 ]
  [ "$(field .summary.failed)" -eq 0 ]
}

@test "anonymize: Check Database problems in the clone fail, even when the source has the same ones" {
  anonymize_sandbox
  jq -c '.check_db.future_ids = 2 | .check_db.card_properties = 1' <<<"$REPORT" >"$STUB_DIR/s-1-10k.json"
  cp "$STUB_DIR/s-1-10k.json" "$STUB_DIR/clone.json"
  mg anonymize --source build/fixtures/s-1-10k
  [ "$status" -eq 1 ]
  [ "$(field .status)" = fail ]
  [ "$(field '[.failures[].id] | join(",")')" = v3/clone_check_db ]
  [[ "$(field '.failures[0].message')" == *"card_properties 1, future_ids 2"* ]]
  # The reports agree; the clone alone is judged.
  [ "$(field .details.v3.equal)" = true ]
  [ "$(field -c .details.v3.clone_check_db)" = '{"card_properties":1,"future_ids":2}' ]
  [ "$(field .summary.failed)" -eq 1 ]

  jq -c '.check_db = {"error": "corrupt"} | .changed_tables = []' <<<"$REPORT" >"$STUB_DIR/s-1-10k.json"
  cp "$STUB_DIR/s-1-10k.json" "$STUB_DIR/clone.json"
  mg anonymize --source build/fixtures/s-1-10k
  [ "$(field .status)" = fail ]
  [ "$(field '[.failures[].id] | join(",")')" = v3/clone_check_db ]
  [[ "$(field '.failures[0].message')" == *"error corrupt"* ]]
}

@test "anonymize: a schedule or Check Database difference fails V3, naming the members and deck ids" {
  anonymize_sandbox
  jq -c '.schedule.sha256 = "bbb" | .schedule.decks["77"].next_states_sha256 = "zz" | .changed_tables += ["notes"]' \
    <<<"$REPORT" >"$STUB_DIR/clone.json"
  mg anonymize --source build/fixtures/s-1-10k --now 2030-01-15T11:00:00+01:00 --tz Europe/Paris
  [ "$status" -eq 1 ]
  [ "$(field .status)" = fail ]
  [ "$(field '[.failures[].id] | join(",")')" = "v3/schedule,v3/changed_notes" ]
  [[ "$(field '.failures[0].message')" == *"differ: 77" ]]
  [ "$(field -c .details.v3.decks)" = '["77"]' ]
  [ "$(grep -c '^oracle check .* --schedule --now 2030-01-15T11:00:00+01:00 --tz Europe/Paris$' "$STUB_LOG")" -eq 2 ]
}

@test "anonymize: V4 review items warn, a V4 failure fails, an oracle that cannot run is an error" {
  anonymize_sandbox
  export FAKE_ANON_REPORT='{"checks": {"leak": {"locations": [{"column": "val", "hits": 1, "key": "addonDecks", "table": "config"}], "status": "warn"}, "media": "pass", "rows": "pass", "structure": "pass"}, "notes": 3}'
  mg anonymize --source build/fixtures/s-1-10k
  [ "$(field .status)" = warn ]
  [[ "$(field .message)" == *"1 review items"* ]]
  export FAKE_ANON_REPORT='{"checks": {"leak": {"locations": [{"column": "name", "hits": 1, "key": 5, "table": "decks"}], "status": "fail"}, "media": "pass", "rows": "pass", "structure": "pass"}, "notes": 3}'
  mg anonymize --source build/fixtures/s-1-10k
  [ "$(field .status)" = fail ]
  [ "$(field '.failures[0].id')" = leak ]
  unset FAKE_ANON_REPORT
  rm "$STUB_DIR/clone.json"
  mg anonymize --source build/fixtures/s-1-10k
  [ "$status" -eq 2 ]
  [ "$(field .status)" = error ]
  [ "$(field '.failures[0].id')" = v3/clone ]
  [[ "$(field '.failures[0].message')" == *"oracle-tree-differs"* ]]
  [[ "$(field .message)" == *"V3 (the oracle) could not"* ]]
}

@test "anonymize: meguri-cli's error kind is the failure; nothing goes outside build/" {
  anonymize_sandbox
  FAKE_ANON_KIND=structure_changed mg anonymize --source build/fixtures/s-1-10k
  [ "$(field .status)" = error ]
  [ "$(field '.failures[0].id')" = structure_changed ]
  ! grep -q '^oracle' "$STUB_LOG"
  : >"$STUB_LOG"
  mg anonymize --source build/fixtures/s-1-10k --out "$BATS_TEST_TMPDIR/elsewhere"
  [ "$(field '.failures[0].id')" = usage ]
  mg anonymize --source build/fixtures/s-1-10k --out build/../x
  [ "$(field '.failures[0].id')" = usage ]
  mg anonymize --source build/fixtures/s-1-10k --salt-file /tmp/salt
  [ "$(field '.failures[0].id')" = usage ]
  mg anonymize --source build/fixtures/nothing
  [ "$(field '.failures[0].id')" = no_collection ]
  make_source
  mg anonymize --source "$SB/local-data/anki"
  [ "$(field '.failures[0].id')" = usage ]
  [[ "$(field '.failures[0].message')" == *"through the vault"* ]]
  ! grep -q 'compose' "$STUB_LOG"
}

@test "anonymize: by default a fresh vault copy, refused on CI and in implementer worktrees, deleted at the end" {
  anonymize_sandbox
  CI=true mg anonymize
  [ "$(field '.failures[0].id')" = ci-marker ]
  printf '{"role": "implementer"}\n' >"$SB/.git/meguri-session.json"
  mg anonymize
  [ "$(field '.failures[0].id')" = implementer-worktree ]
  rm "$SB/.git/meguri-session.json"
  ! grep -q docker "$STUB_LOG"
  mg anonymize --tz UTC
  [ "$(field '.failures[0].id')" = no-source ]
  make_source
  printf '%s\n' "$REPORT" >"$STUB_DIR/copy.json"
  mg anonymize --tz UTC
  [ "$(field .status)" = pass ]
  local run
  run="$(field .run_id)"
  grep -q "service private vault=$SB/local-data/anki$" "$STUB_LOG"
  grep -q "service private vault=$SB/build/private/$run/copy$" "$STUB_LOG"
  # 12:00 today in the zone given (no manifest).
  grep -q "^oracle check $SB/build/private/$run/copy --schedule --now $(date -u +%Y-%m-%d)T12:00:00+00:00 --tz UTC$" "$STUB_LOG"
  [ "$(field .details.clone.from_vault)" = true ]
  [ ! -e "$SB/build/private/$run/copy" ]
  [ -f "$SB/build/private/$run/clone/collection.anki2" ]
  no_private_text "$result" "$SB/build/mg/anonymize/$run/log.txt"
}

@test "anonymize-check: generates the collection, anonymizes it and passes only on V3 evidence" {
  anonymize_sandbox
  cat >>"$SB/tools/mg.d/zz-stub.sh" <<'STUB'
mg_fixtures() {
  echo "fixtures $*" >>"$STUB_LOG"
  mg_detail fixtures '[{"target": "10k", "kind": "size", "dir": "build/fixtures/s-1-10k"}]'
  mg_status pass
}
STUB
  mg anonymize-check
  [ "$status" -eq 0 ]
  [ "$(field .status)" = pass ]
  grep -q '^fixtures 10k --seed 1$' "$STUB_LOG"
  grep -q "meguri-cli anonymize --copy $FIX " "$STUB_LOG"
  [ "$(grep -c '^oracle check .* --schedule --now 2030-01-15T10:00:00Z --tz Etc/GMT-1$' "$STUB_LOG")" -eq 2 ]
  [ "$(field .details.anonymize.v3.equal)" = true ]
  [ "$(field .summary.passed)" -eq 5 ]

  # A schedule that differs fails it, with anonymize's failures.
  jq -c '.schedule.sha256 = "bbb"' <<<"$REPORT" >"$STUB_DIR/clone.json"
  mg anonymize-check 10k --seed 1
  [ "$status" -eq 1 ]
  [ "$(field '[.failures[].id] | join(",")')" = v3/schedule ]
  [ "$(field '.failures[0].repro')" = "tools/mg anonymize --source build/fixtures/s-1-10k" ]

  # Without the oracle there is no evidence: error, never pass.
  rm "$STUB_DIR/clone.json"
  mg anonymize-check
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = v3/clone ]

  mg anonymize-check 10k --seed x
  [ "$(field '.failures[0].id')" = usage ]
  mg anonymize-check notasize
  [ "$(field '.failures[0].id')" = usage ]
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
assert s["volumes"] == ["..:/work", "${MEGURI_DOCKER_CARGO_TARGET:-cargo-target}:/cargo-target:ro"], s["volumes"]
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

@test "private refresh-public: prunes existing synthetic tokens by pinned HEAD, without Docker or source access" {
  printf 'syntheticpublicword\n' >"$SB/public-baseline.txt"
  git -C "$SB" add public-baseline.txt
  git -C "$SB" commit -qm 'synthetic public baseline'
  local head unknown="syntheticunknown${BATS_TEST_NUMBER}word"
  head="$(git -C "$SB" rev-parse HEAD)"
  mkdir -p "$SB/build/private/tokens"
  chmod 700 "$SB/build/private"
  printf 'syntheticpublicword\n%s\n' "$unknown" >"$SB/build/private/tokens/tokens.txt"
  chmod 600 "$SB/build/private/tokens/tokens.txt"
  mg private refresh-public --head "$head"
  [ "$status" -eq 0 ]
  [ "$(field .details.refresh_public.removed)" -eq 1 ]
  [ "$(field .details.refresh_public.after)" -eq 1 ]
  [ "$(cat "$SB/build/private/tokens/tokens.txt")" = "$unknown" ]
  [ ! -s "$STUB_LOG" ]
  [ ! -e "$SB/local-data" ]
}

@test "private refresh-public: missing pin and CI marker refuse before token access" {
  mg private refresh-public
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = usage ]
  export CI=1
  mg private refresh-public --head "$(git -C "$SB" rev-parse HEAD)" --shared-main
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = ci-marker ]
  [ ! -s "$STUB_LOG" ]
  [ ! -e "$SB/build/private" ]
}
