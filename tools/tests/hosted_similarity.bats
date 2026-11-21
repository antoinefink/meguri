#!/usr/bin/env bats
# The hosted similarity job's script (.github/similarity.sh): its output filter
# never prints corpus text or anything it has not checked, its verdict, its
# selection of what to scan, and the pin it fetches.

bats_require_minimum_version 1.5.0

REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"
SCRIPT="$REPO_ROOT/.github/similarity.sh"
# Stands in for corpus text: it must never reach what the job logs.
MARKER=CORPUS-TEXT-MARKER-5e1f

setup() {
  RUN="$BATS_TEST_TMPDIR/build/mg/similarity/20261121T000000Z-1"
  mkdir -p "$RUN/scan"
  export RUNNER_TEMP="$BATS_TEST_TMPDIR/tmp"
  mkdir -p "$RUNNER_TEMP"
  printf '%s\0' core/a.rs core/b.rs conformance/scenarios/sched/one.yaml conformance/goldens/sched/one.json \
    >"$BATS_TEST_TMPDIR/tracked"
}

# A scan whose reports carry the marker everywhere a scanner or a corrupted
# report could put text: messages, reasons, region keys, the reviewer view's
# Anki sources, unknown fields, a path under the corpus, the log.
write_reports() {
  local status="$1" a="$2" b="$3"
  cat >"$RUN/result.json" <<EOF
{"status": "$status", "message": "similarity: /refs/anki/rslib/x.rs $MARKER",
 "failures": [{"id": "similarity:$a:struct", "message": "$MARKER"}, {"id": "$MARKER"},
              {"id": "similarity:error", "message": "$MARKER"}],
 "details": {"corpus": {"path": "/refs/anki $MARKER"}}}
EOF
  cat >"$RUN/scan/summary.json" <<EOF
{"status": "$status", "note": "$MARKER",
 "counts": {"scanned": 3, "clean": 1, "allowlisted": 0, "flagged": 2, "errors": 0, "skipped": 4, "missing": 0},
 "corpus": {"key": "0123456789abcdef0123", "tags": 54, "first_tag": "2.1.50", "last_tag": "26.09.3 $MARKER",
            "built": false, "missing_sources": ["$MARKER"]},
 "flags": [{"path": "$a", "channel": "struct", "reasons": ["$MARKER"], "regions": [[1, 9]]}],
 "test_data": {"pairs": 1, "limited": ["$b"], "limits": "$MARKER"}}
EOF
  cat >"$RUN/scan/implementer.json" <<EOF
{"view": "implementer", "files": [
  {"path": "$a", "status": "flagged", "note": "$MARKER",
   "channels": {"struct": {"score": 0.4231, "matched": 22, "fingerprints": 52, "extra": "$MARKER"}},
   "flags": [{"channel": "struct", "reasons": ["$MARKER"],
              "regions": [{"lines": [1, 9], "keys": ["$MARKER"], "anki": [{"path": "/refs/anki/x.rs", "text": "$MARKER"}]}]}]},
  {"path": "/refs/anki/rslib/$MARKER.rs", "status": "flagged",
   "channels": {"lex": {"score": "$MARKER"}}, "flags": [{"channel": "lex"}]},
  {"path": "$b", "status": "flagged", "channels": {"text": {"matched": 2, "fingerprints": "$MARKER"}},
   "flags": [{"channel": "$MARKER"}, {"channel": "text"}]},
  {"path": "core/clean.rs", "status": "clean", "channels": {}}]}
EOF
  printf '{"view": "reviewer", "files": [{"anki": "/refs/anki/rslib/src/x.rs:12 %s"}]}\n' "$MARKER" >"$RUN/scan/reviewer.json"
  printf -- '- Anki anki 2.1.50 rslib/src/x.rs:1-9\n/refs/anki/rslib/src/x.rs: %s\n' "$MARKER" >"$RUN/scan/reviewer.md"
  printf '/refs/anki/rslib/src/x.rs:12: %s\nTraceback: KeyError(%s)\n' "$MARKER" "$MARKER" >"$RUN/log.txt"
}

summary() {
  run --separate-stderr bash "$SCRIPT" summary "$1" "$RUN/result.json" "$BATS_TEST_TMPDIR/tracked" "$2"
  printf '%s\n%s\n' "$output" "$stderr" >"$BATS_TEST_TMPDIR/printed"
}

# never_printed: neither stdout nor stderr of the last summary holds the
# marker or a corpus path (it keeps $output for the checks that follow).
never_printed() {
  local needle
  for needle in "$MARKER" /refs/ rslib; do
    if grep -qF -- "$needle" "$BATS_TEST_TMPDIR/printed"; then
      echo "printed: $needle" >&2
      return 1
    fi
  done
}

changed() {
  printf '%s\0' "$@" >"$BATS_TEST_TMPDIR/changed"
  echo "$BATS_TEST_TMPDIR/changed"
}

@test "the summary prints counts, scores and tracked paths, never corpus text" {
  write_reports fail core/a.rs core/b.rs
  summary code "$(changed core/a.rs core/b.rs)"
  [ "$status" -eq 1 ]
  never_printed
  [[ "$output" == *"3 files scanned, 1 passed (0 allowlisted, 0 recorded oracle output), 2 flagged, 4 skipped"* ]]
  [[ "$output" == *"Corpus: 54 Anki release tags 2.1.50..?, index 0123456789abcdef0123 (restored)."* ]]
  grep -qF '| `core/a.rs` | struct | 42% (22 of 52 fingerprints) | yes |' <<<"$output"
  grep -qF '| `core/b.rs` | text | 2 matching runs | yes |' <<<"$output"
  grep -qF '| (a path withheld from the log) | lex |' <<<"$output"
  [[ "$output" == *"Code: **failed**"* ]]
}

@test "scanner errors print fixed text only" {
  write_reports error core/a.rs core/b.rs
  jq --arg m "git ls-tree failed in /refs/anki: $MARKER" '. + {error: $m}' "$RUN/scan/summary.json" >"$RUN/x" &&
    mv "$RUN/x" "$RUN/scan/summary.json"
  summary code "$(changed core/a.rs)"
  [ "$status" -eq 1 ]
  never_printed
  [[ "$output" == *"Scanner error: a scanner error (its message is withheld from the hosted log)."* ]]
  jq '.error = "test-data selection is empty"' "$RUN/scan/summary.json" >"$RUN/x" && mv "$RUN/x" "$RUN/scan/summary.json"
  summary data "$(changed core/a.rs)"
  [ "$status" -eq 1 ]
  [[ "$output" == *"Scanner error: test-data selection is empty."* ]]
}

@test "reports the filter cannot read fail without quoting them" {
  write_reports fail core/a.rs core/b.rs
  printf '{"files": "%s"}\n' "$MARKER" >"$RUN/scan/implementer.json"
  summary code "$(changed core/a.rs)"
  [ "$status" -eq 1 ]
  never_printed
  printf 'not json %s\n' "$MARKER" >"$RUN/scan/implementer.json"
  rm "$RUN/scan/summary.json"
  summary code "$(changed core/a.rs)"
  [ "$status" -eq 1 ]
  never_printed
  [[ "$output" == *"failed"* ]]
}

clean_reports() {
  echo '{"status": "pass", "failures": []}' >"$RUN/result.json"
  echo '{"status": "pass", "counts": {"scanned": 1, "clean": 1, "allowlisted": 0, "flagged": 0, "errors": 0, "skipped": 0, "missing": 0}}' \
    >"$RUN/scan/summary.json"
  echo '{"files": [{"path": "core/a.rs", "status": "clean"}]}' >"$RUN/scan/implementer.json"
}

@test "a passing receipt without readable reports fails" {
  local report
  clean_reports
  summary code "$(changed core/a.rs)"
  [ "$status" -eq 0 ]
  for report in implementer summary; do
    clean_reports
    printf 'not json\n' >"$RUN/scan/$report.json"
    summary code "$(changed core/a.rs)"
    [ "$status" -eq 1 ]
    [[ "$output" == *"A scanner report is missing or unreadable."* ]]
    rm "$RUN/scan/$report.json"
    summary code "$(changed core/a.rs)"
    [ "$status" -eq 1 ]
  done
  # A valid document followed by garbage is not one report.
  clean_reports
  printf '{"files": []}\nnot json\n' >"$RUN/scan/implementer.json"
  summary code "$(changed core/a.rs)"
  [ "$status" -eq 1 ]
  clean_reports
  printf '{"files": []}\n{"files": []}\n' >"$RUN/scan/implementer.json"
  summary code "$(changed core/a.rs)"
  [ "$status" -eq 1 ]
  clean_reports
  echo '{"files": {}}' >"$RUN/scan/implementer.json"
  summary code "$(changed core/a.rs)"
  [ "$status" -eq 1 ]
}

@test "a clean scan passes and a missing file fails" {
  cat >"$RUN/result.json" <<'EOF'
{"status": "pass", "failures": []}
EOF
  cat >"$RUN/scan/summary.json" <<'EOF'
{"status": "pass", "counts": {"scanned": 1, "clean": 1, "allowlisted": 0, "flagged": 0, "errors": 0, "skipped": 0, "missing": 0}}
EOF
  echo '{"files": [{"path": "core/a.rs", "status": "clean"}]}' >"$RUN/scan/implementer.json"
  summary code "$(changed core/a.rs)"
  [ "$status" -eq 0 ]
  [[ "$output" == *"Code: passed"* ]]
  jq '.status = "warn" | .counts.missing = 1 | .missing = ["core/a.rs"]' "$RUN/scan/summary.json" >"$RUN/x" &&
    mv "$RUN/x" "$RUN/scan/summary.json"
  echo '{"status": "warn", "failures": []}' >"$RUN/result.json"
  summary code "$(changed core/a.rs)"
  [ "$status" -eq 1 ]
  [[ "$output" == *'Listed but not checked: `core/a.rs`.'* ]]
}

@test "test data fails only on flags in files the change touches" {
  local scenario=conformance/scenarios/sched/one.yaml golden=conformance/goldens/sched/one.json
  write_reports fail "$scenario" "$golden"
  summary data "$(changed core/x.rs)"
  [ "$status" -eq 0 ]
  never_printed
  [[ "$output" == *"Test data (conformance scenarios and goldens of the touched areas): passed"* ]]
  [[ "$output" == *"1 scenario and golden pairs; 1 files are below the fingerprint limits"* ]]
  [[ "$output" == *"3 flagged files are not changed here"* ]]
  summary data "$(changed "$golden")"
  [ "$status" -eq 1 ]
  grep -qF "| \`$golden\` | text | 2 matching runs | yes |" <<<"$output"
}

# A small repository with the scanner's config and lexer, for `select`.
select_repo() {
  local r="$BATS_TEST_TMPDIR/repo"
  mkdir -p "$r/tools/similarity/meguri_similarity" "$r/conformance/scenarios/sched" "$r/conformance/goldens/sched" \
    "$r/conformance/scenarios/lonely" "$r/docs" "$r/core"
  cp "$REPO_ROOT/tools/similarity/config.yaml" "$r/tools/similarity/"
  cp "$REPO_ROOT/tools/similarity/meguri_similarity/lexer.py" "$r/tools/similarity/meguri_similarity/"
  echo a >"$r/conformance/scenarios/sched/one.yaml"
  echo a >"$r/conformance/goldens/sched/one.json"
  echo a >"$r/conformance/scenarios/lonely/one.yaml"
  echo a >"$r/docs/a.md"
  git -C "$r" init -q
  git -C "$r" add -A
  git -C "$r" -c user.name=t -c user.email=t@t commit -qm base
  echo "$r"
}

select_after() {
  local r="$1" p
  shift
  for p in "$@"; do
    mkdir -p "$r/$(dirname "$p")"
    echo "change $RANDOM" >>"$r/$p"
  done
  git -C "$r" add -A
  git -C "$r" -c user.name=t -c user.email=t@t commit -qm change
  export GITHUB_OUTPUT="$BATS_TEST_TMPDIR/out"
  : >"$GITHUB_OUTPUT"
  (cd "$r" && EVENT=push BEFORE="$(git rev-parse HEAD~1)" bash "$SCRIPT" select >/dev/null)
}

out() { sed -n "s/^$1=//p" "$GITHUB_OUTPUT"; }

@test "select runs only for files the scanner reads, and names the touched areas" {
  local r
  r="$(select_repo)"
  select_after "$r" docs/a.md specs/x/spec.md conformance/fixtures/a.rs core/README.md
  [ "$(out run)" = no ]
  [ "$(out code)" = no ]
  select_after "$r" core/src/LIB.RS
  [ "$(out run)" = yes ]
  [ "$(out code)" = yes ]
  [ "$(out areas)" = "" ]
  [ "$(out base)" = "$(git -C "$r" rev-parse HEAD~1)" ]
  select_after "$r" conformance/goldens/sched/one.json conformance/scenarios/lonely/one.yaml
  [ "$(out run)" = yes ]
  [ "$(out code)" = no ]
  # An area without its other side is kept, so its scan fails.
  [ "$(out areas)" = "lonely sched" ]
  select_after "$r" tools/similarity/config.yaml
  [ "$(out code)" = yes ]
  [ "$(out areas)" = sched ]
  select_after "$r" .github/similarity.sh
  [ "$(out code)" = yes ]
  [ "$(out areas)" = "" ]
  # A scanner change keeps the touched unpaired area beside every paired one.
  select_after "$r" tools/similarity/AGENTS.md conformance/scenarios/lonely/two.yaml
  [ "$(out areas)" = "lonely sched" ]
  # Deleting an area's last golden selects it: its scenario is now unpaired.
  git -C "$r" rm -q conformance/goldens/sched/one.json
  git -C "$r" -c user.name=t -c user.email=t@t commit -qm unpair
  : >"$GITHUB_OUTPUT"
  (cd "$r" && EVENT=push BEFORE="$(git rev-parse HEAD~1)" bash "$SCRIPT" select >/dev/null)
  [ "$(out run)" = yes ]
  [ "$(out areas)" = sched ]
}

@test "select without a base scans the whole tree and every paired area" {
  local r
  r="$(select_repo)"
  export GITHUB_OUTPUT="$BATS_TEST_TMPDIR/out"
  : >"$GITHUB_OUTPUT"
  (cd "$r" && EVENT=schedule bash "$SCRIPT" select >/dev/null)
  [ "$(out run)" = yes ]
  [ "$(out code)" = yes ]
  [ "$(out base)" = "" ]
  [ "$(out areas)" = sched ]
}

@test "the pin lists every pinned tag once, at a full commit id, boundaries included" {
  run --separate-stderr bash -c 'cd "$1" && bash .github/similarity.sh pin' _ "$REPO_ROOT"
  [ "$status" -eq 0 ]
  local pins="$output"
  [ "${#lines[@]}" -eq 54 ]
  [ "${lines[0]%% *}" = 2.1.50 ]
  [ "${lines[53]%% *}" = 26.09.3 ]
  [ "$(cut -d' ' -f1 <<<"$pins" | sort -u | wc -l)" -eq 54 ]
  run ! grep -vE '^[0-9]+(\.[0-9]+)+ [0-9a-f]{40}$' <<<"$pins"
}

@test "the corpus may not be fetched into, above or onto the checkout, nor over existing files" {
  local r="$BATS_TEST_TMPDIR/work/meguri" d
  mkdir -p "$r/.github" "$r/tools/similarity" "$BATS_TEST_TMPDIR/full"
  cp "$SCRIPT" "$r/.github/"
  cp "$REPO_ROOT/tools/similarity/config.yaml" "$r/tools/similarity/"
  git -C "$r" init -q
  echo keep >"$r/keep" && echo keep >"$BATS_TEST_TMPDIR/full/keep"
  ln -s "$BATS_TEST_TMPDIR/work" "$BATS_TEST_TMPDIR/link"
  for d in "$r/build/refs" "$r" "$BATS_TEST_TMPDIR/work" "$BATS_TEST_TMPDIR/link" "$r/../.." "$BATS_TEST_TMPDIR/full"; do
    run --separate-stderr bash -c 'cd "$1" && bash .github/similarity.sh corpus "$2"' _ "$r" "$d"
    [ "$status" -eq 1 ]
    [[ "$output" == *"::error::"* ]]
  done
  [ -f "$r/keep" ] && [ -f "$BATS_TEST_TMPDIR/full/keep" ]
  [ ! -e "$r/build/refs" ]
}
