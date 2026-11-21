#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# "Linux · similarity" of the hosted checks (docs/process/tiers.md, "Hosted
# checks"; docs/process/provenance.md, "Similarity scan"). Run from the
# checkout root.
#
#   similarity.sh select     which scans this change needs. Env: EVENT,
#                            BEFORE. Writes run=, base=, code= and areas= to
#                            $GITHUB_OUTPUT.
#   similarity.sh pin        the pinned "<tag> <commit>" pairs, one per line.
#   similarity.sh corpus <dir>
#                            fetch the pinned Anki release tags
#                            (tools/similarity/config.yaml, corpus.commits)
#                            from Anki's public repository into <dir>/anki,
#                            a directory outside the checkout, and check that
#                            each tag is the pinned commit and nothing else
#                            was fetched.
#   similarity.sh scan       run `tools/mg similarity` (the code scan, then the
#                            test-data scan of the touched areas) with the
#                            corpus in REFS, and write what `summary` prints to
#                            the log and $GITHUB_STEP_SUMMARY. Env: REFS, BASE,
#                            CODE, AREAS. Fails when a scan errs or flags a file.
#   similarity.sh summary <code|data> <result.json> <tracked> <changed>
#                            the output filter (tools/tests/hosted_similarity.bats).
#                            <tracked> and <changed> are NUL lists of paths.
#                            Exits 1 when the scan fails the check.
#
# Nothing from Anki may reach the checkout, a cache, an artifact or the log.
# The corpus lives outside the checkout and is mounted read-only into the
# scanner. The index cache holds hashes plus Anki paths and line numbers. The
# job uploads nothing. `tools/mg` output goes to files under $RUNNER_TEMP that
# are never printed: the log and the step summary get only what `summary`
# builds from typed fields of the scanner's reports. It prints numbers, tag
# names it checked, fixed messages, and Meguri paths that are tracked in this
# commit. Never the reviewer view, a scanner message it does not know, or a
# raw log line.

set -euo pipefail

ANKI_URL=https://github.com/ankitects/anki
CONFIG=tools/similarity/config.yaml

# pin: the pinned "<tag> <commit>" pairs of the config, one per line.
pin() {
  awk '
    /^[^ #]/ { corpus = ($0 ~ /^corpus:/); c = 0; next }
    corpus && /^  [A-Za-z_]+:/ { c = ($0 ~ /^  commits:/); next }
    corpus && c && /^    "/ {
      v = $0; gsub(/[" ]/, "", v); n = split(v, kv, ":")
      if (n == 2) print kv[1], kv[2]
    }' "$CONFIG"
}

# The directories the scanner never scans (the `<dir>/**` entries of
# scan.exclude), as tools/mg.d/similarity.sh reads them, one per line.
excluded_dirs() {
  echo oracle/
  awk '
    /^[^ #]/ { scan = ($0 ~ /^scan:/); ex = 0; next }
    scan && /^  [A-Za-z_]+:/ { ex = ($0 ~ /^  exclude:/); next }
    scan && ex && /^    - / {
      v = $0; sub(/^    - */, "", v); sub(/[ \t]+#.*$/, "", v); gsub(/["\047]/, "", v)
      if (v ~ /\/\*\*$/) print substr(v, 1, length(v) - 2)
    }' "$CONFIG"
}

# The file extensions the scanner knows (EXTENSION_LANGUAGE), one per line.
scanned_extensions() {
  awk '/^EXTENSION_LANGUAGE = \{/ { f = 1; next } f && /^\}/ { exit }
    f && match($0, /"\.[A-Za-z0-9]+"/) { print substr($0, RSTART + 1, RLENGTH - 2) }' \
    tools/similarity/meguri_similarity/lexer.py
}

# Areas with both a scenario and a golden directory at HEAD, one per line.
paired_areas() {
  comm -12 <(git ls-tree -d -z --name-only HEAD conformance/scenarios/ | tr '\0' '\n' | sed 's|.*/||' | sort) \
    <(git ls-tree -d -z --name-only HEAD conformance/goldens/ | tr '\0' '\n' | sed 's|.*/||' | sort) |
    grep -E '^[A-Za-z0-9][A-Za-z0-9_.-]*$' || true
}

# The thresholds, channels and scope of the config (its `channels:` and
# `scan:` sections) at a commit.
scan_settings() {
  git show "$1:$CONFIG" 2>/dev/null | awk '/^[^ #]/ { keep = ($0 ~ /^(channels|scan):/) } keep'
}

cmd_select() {
  local base="" code=no p ext d skip area
  case "${EVENT:-}" in
    # The checked-out merge commit's first parent is the base tip.
    pull_request) base="$(git rev-parse HEAD^1)" ;;
    push)
      if [[ -n "${BEFORE:-}" && ! "$BEFORE" =~ ^0+$ ]] &&
        { git cat-file -e "$BEFORE^{commit}" 2>/dev/null ||
          git fetch --quiet --no-tags --depth=1 origin "$BEFORE"; }; then
        base="$(git rev-parse "$BEFORE^{commit}")"
      fi
      ;;
  esac
  local -a excluded exts areas=()
  mapfile -t excluded < <(excluded_dirs)
  mapfile -t exts < <(scanned_extensions)
  ((${#exts[@]} > 0)) || { echo "::error::No scanner extensions found in lexer.py"; exit 1; }
  if [[ -z "$base" ]]; then
    # Nothing to compare with: the whole tree and every area.
    code=yes
    mapfile -t areas < <(paired_areas)
  else
    local -A touched=()
    local all_areas=""
    while IFS= read -r -d '' p; do
      case "$p" in
        # A scanner change runs both scans, the test data of every area.
        tools/similarity/*) code=yes all_areas=yes ;;
        # The job's other inputs: scan, so a change to it runs it.
        tools/mg | tools/mg.d/similarity.sh | docker/similarity.Dockerfile* | \
          docker/compose.yml | .github/similarity.sh | .github/workflows/meguri-verification.yml) code=yes ;;
      esac
      # A touched area without its other side is kept: the scan then fails
      # on the unpaired files instead of skipping them.
      if [[ "$p" =~ ^conformance/(scenarios|goldens)/([^/]+)/ ]]; then
        area="${BASH_REMATCH[2]}"
        [[ "$area" =~ ^[A-Za-z0-9][A-Za-z0-9_.-]*$ ]] || { echo "::error::Unexpected conformance area name"; exit 1; }
        touched[$area]=1
      fi
      skip=""
      for d in "${excluded[@]}"; do
        [[ "$p" != "$d"* ]] || { skip=yes; break; }
      done
      [[ -z "$skip" && "$p" == *.* ]] || continue
      ext=".${p##*.}"
      ext="${ext,,}"
      for d in "${exts[@]}"; do
        [[ "$ext" != "$d" ]] || { code=yes; break; }
      done
    done < <(git diff -z --no-renames --name-only --diff-filter=d "$base" HEAD)
    if [[ -n "$all_areas" ]]; then
      mapfile -t areas < <(paired_areas)
    elif ((${#touched[@]} > 0)); then
      mapfile -t areas < <(printf '%s\n' "${!touched[@]}" | sort)
    fi
  fi
  local run=no
  [[ "$code" == no && ${#areas[@]} -eq 0 ]] || run=yes
  echo "Base: ${base:-none}; code scan: $code; test-data areas: ${areas[*]:-none}"
  if [[ "$run" == no ]]; then
    printf '### Linux · similarity: not selected\n\nThis change adds or modifies no file the scanner reads.\n' >>"${GITHUB_STEP_SUMMARY:-/dev/null}"
  fi
  {
    echo "run=$run"
    echo "base=$base"
    echo "code=$code"
    echo "areas=${areas[*]:-}"
  } >>"$GITHUB_OUTPUT"
}

cmd_corpus() {
  local dir="$1" repo="$1/anki" tag commit got
  local -a tags=() commits=() refspecs=()
  while read -r tag commit; do
    [[ "$tag" =~ ^[0-9]+(\.[0-9]+)+$ && "$commit" =~ ^[0-9a-f]{40}$ ]] ||
      { echo "::error::Malformed corpus.commits line in $CONFIG"; exit 1; }
    tags+=("$tag") commits+=("$commit") refspecs+=("+refs/tags/$tag:refs/tags/$tag")
  done < <(pin)
  ((${#tags[@]} > 0)) || { echo "::error::$CONFIG pins no commits (corpus.commits)"; exit 1; }
  case "$(realpath -m "$dir")/" in
    "$(realpath "$PWD")"/*) echo "::error::The corpus must stay outside the checkout"; exit 1 ;;
  esac
  rm -rf "$dir"
  mkdir -p "$dir"
  # A fresh repository with no credentials: no global or system git config
  # (so no token header), no prompt, the tags only, at depth 1.
  export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1 GIT_TERMINAL_PROMPT=0
  git init --quiet "$repo"
  git -C "$repo" fetch --quiet --no-tags --depth=1 "$ANKI_URL" "${refspecs[@]}"
  local i
  for i in "${!tags[@]}"; do
    got="$(git -C "$repo" rev-parse --verify --quiet "refs/tags/${tags[$i]}^{commit}" || true)"
    [[ "$got" == "${commits[$i]}" ]] ||
      { echo "::error::Anki tag ${tags[$i]} is ${got:-missing}, pinned ${commits[$i]}"; exit 1; }
  done
  local refs
  refs="$(git -C "$repo" for-each-ref --format='%(refname)' | wc -l)"
  ((refs == ${#tags[@]})) || { echo "::error::Fetched $refs refs, pinned ${#tags[@]} tags"; exit 1; }
  echo "Fetched ${#tags[@]} pinned Anki release tags (${tags[0]}..${tags[-1]}), each at its pinned commit;" \
    "$(du -sm "$repo/.git" | cut -f1) MB, outside the checkout."
}

# The output filter. Everything printed comes from typed fields: numbers,
# checked tag names and index keys, fixed text, and paths that are tracked
# in this commit and spelled plainly.
cmd_summary() {
  local kind="$1" result="$2" tracked="$3" changed="$4" dir
  dir="$(dirname "$result")"
  local summary="$dir/scan/summary.json" implementer="$dir/scan/implementer.json"
  local empty='{}' files='{"files": []}' out
  out="$(mktemp)"
  jq -n -r \
    --arg kind "$kind" \
    --slurpfile result <(jq -c . "$result" 2>/dev/null || echo "$empty") \
    --slurpfile summary <(jq -c . "$summary" 2>/dev/null || echo "$empty") \
    --slurpfile view <(jq -c . "$implementer" 2>/dev/null || echo "$files") \
    --rawfile tracked "$tracked" --rawfile changed "$changed" '
    def set: split("\u0000") | map(select(length > 0)) | map({key: ., value: true}) | from_entries;
    ($tracked | set) as $known | ($changed | set) as $mine |
    def obj: if type == "object" then . else {} end;
    ($result[0] | obj) as $r | ($summary[0] | obj) as $s | ($view[0] | obj) as $v |
    def num: if type == "number" then . else null end;
    def count($k): ($s.counts // {})[$k] | num // 0;
    def plain: type == "string" and test("^[A-Za-z0-9._@+-]+(/[A-Za-z0-9._@+-]+)*$") and ($known[.] // false);
    def shown: if plain then "`" + . + "`" else "(a path withheld from the log)" end;
    def tag: if type == "string" and test("^[0-9]+(\\.[0-9]+)+$") then . else "?" end;
    def channel: . as $x | if type == "string" and (["struct", "lex", "com", "text"] | index([$x])) then . else null end;
    def messages: [
      "test-data input is outside the conformance scenario and golden areas",
      "test-data files must be inside an area directory",
      "test-data requires regular files without links",
      "unsupported test-data file type",
      "test-data file exceeds the scanner size bound",
      "test-data changed while being read",
      "test-data is not UTF-8",
      "test-data input is missing, linked or unreadable",
      "test-data selection is empty",
      "test-data has duplicate scenario or golden names",
      "test-data requires a scenario and golden of the same area for every selected name",
      "invalid or unsupported test-data document"];
    def classify:
      . as $x |
      if type != "string" then "the scanner failed without a reason"
      elif (messages | index([$x])) then .
      elif startswith("the Anki corpus is incomplete") then "the Anki corpus lacks a pinned release tag"
      elif startswith("the Anki corpus has ") then "the Anki corpus has more release tags than the pin"
      elif startswith("the Anki corpus differs from the pinned commits") then "the Anki corpus differs from the pinned commits"
      elif startswith("no Anki repository") then "no Anki repository in the corpus directory"
      elif startswith("paths outside the scanned tree") then "a listed path is outside the scanned tree"
      elif startswith("cannot parse ") then "a scanned file could not be parsed"
      else "a scanner error (its message is withheld from the hosted log)" end;
    def score($c): ($c.score | num) as $x | ($c.matched | num) as $m | ($c.fingerprints | num) as $f |
      if $x != null then "\($x * 100 | floor)% (\($m) of \($f) fingerprints)" else "\($m) matching runs" end;
    ($r.status // "error") as $status |
    [($v.files // [])[] | select(type == "object" and .status == "flagged")] as $flagged |
    [$flagged[] | select(.path | type == "string" and ($mine[.] // false))] as $here |
    (if $kind == "data" then ($here | length) else ($flagged | length) end) as $blocking |
    ($status == "error" or ($s | has("error")) or count("errors") > 0 or count("missing") > 0
      or $blocking > 0 or ($status == "fail" and ($flagged | length) == 0)
      or ($status | IN("pass", "warn", "fail") | not)) as $failed |
    (if $kind == "data" then "Test data (conformance scenarios and goldens of the touched areas)" else "Code" end) as $title |
    "#### \($title): \(if $failed then "**failed**" else "passed" end)",
    "",
    "\(count("scanned")) files scanned, \(count("clean") + count("allowlisted")) passed (\(count("allowlisted")) allowlisted), \(count("flagged")) flagged, \(count("skipped")) skipped (outside the scanner'"'"'s scope), \(count("errors")) errors, \(count("missing")) listed but missing.",
    (if $kind == "data" and ($s.test_data | type) == "object" then
      "\($s.test_data.pairs | num // 0) scenario and golden pairs; \(($s.test_data.limited // []) | length) files are below the fingerprint limits and were not meaningfully checked."
     else empty end),
    (($s.corpus // {}) as $c | if ($c.tags | num) then
      "Corpus: \($c.tags) Anki release tags \($c.first_tag | tag)..\($c.last_tag | tag), index \(if ($c.key | type) == "string" and ($c.key | test("^[0-9a-f]{20}$")) then $c.key else "?" end) (\(if $c.built == true then "built in \($c.build_seconds | num // 0) s" else "restored" end))."
     else empty end),
    (if ($s | has("error")) then "Scanner error: \($s.error | classify)." else empty end),
    (if count("errors") > 0 then "\(count("errors")) files could not be scanned; run the scan locally for the reasons." else empty end),
    (if count("missing") > 0 then "Listed but not checked: " + ([($s.missing // [])[] | shown] | join(", ")) + "." else empty end),
    ([($r.failures // [])[] | .id | select(type == "string" and test("^([a-z-]+|similarity:(error|missing))$"))] | unique) as $ids |
    (if ($ids | length) > 0 and ($flagged | length) == 0 then "tools/mg failures: " + ($ids | map("`" + . + "`") | join(", ")) + "." else empty end),
    (if ($flagged | length) > 0 then
      "",
      "| File | Channel | Score | Changed here |",
      "| --- | --- | --- | --- |",
      ($flagged[] as $f | ($f.flags // [])[] | (.channel | channel) as $ch | select($ch != null) |
        "| \($f.path | shown) | \($ch) | \(score(($f.channels // {})[$ch] // {})) | \(if ($f.path | type == "string" and ($mine[.] // false)) then "yes" else "no" end) |")
     else empty end),
    (if $kind == "data" and ($flagged | length) > ($here | length) then
      "",
      "\(($flagged | length) - ($here | length)) flagged files are not changed here: they are findings on the base, reported but not blocking."
     else empty end),
    "",
    (if $failed then "FAILED" else "PASSED" end)
  ' >"$out" 2>/dev/null || {
    # jq's own messages can quote the values it read: never print them.
    printf '#### %s: **failed**\n\nThe summary filter could not read the scanner reports.\n' "$kind"
    rm -f "$out"
    return 1
  }
  local verdict
  verdict="$(tail -n 1 "$out")"
  sed '$d' "$out"
  rm -f "$out"
  [[ "$verdict" == PASSED ]]
}

# run_mg <kind> <args...>: one tools/mg similarity run; prints its summary.
run_mg() {
  local kind="$1" out="$RUNNER_TEMP/similarity-mg-$1" receipt rc=0
  shift
  MG_SIMILARITY_REFS="${REFS:?the corpus directory}" tools/mg similarity "$@" >"$out.stdout" 2>"$out.stderr" || rc=$?
  receipt="$(tail -n 1 "$out.stdout")"
  if [[ ! "$receipt" =~ ^"$PWD"/build/mg/similarity/[0-9]{8}T[0-9]{6}Z-[0-9]+/result\.json$ || ! -f "$receipt" ]]; then
    printf '#### %s: **failed**\n\ntools/mg similarity left no receipt (exit %s); run it locally.\n\n' "$kind" "$rc"
    return 1
  fi
  cmd_summary "$kind" "$receipt" "$RUNNER_TEMP/similarity-tracked" "$RUNNER_TEMP/similarity-changed"
}

cmd_scan() {
  local rc=0 area settings=""
  local -a allow=() paths=() areas=()
  git ls-files -z >"$RUNNER_TEMP/similarity-tracked"
  if [[ -n "${BASE:-}" ]]; then
    git diff -z --no-renames --name-only --diff-filter=d "$BASE" HEAD >"$RUNNER_TEMP/similarity-changed"
    # A pull request is scanned with its base's allowlist: an entry exempts
    # nothing until it is on master. A master push uses master's own.
    [[ "${EVENT:-}" != pull_request ]] || allow=(--allowlist-rev "$BASE")
    # Nor may a change loosen the scan it is checked with: thresholds,
    # channels and exclusions changed against the base fail the check.
    [[ "$(scan_settings "$BASE")" == "$(scan_settings HEAD)" ]] || settings=changed
  else
    cp "$RUNNER_TEMP/similarity-tracked" "$RUNNER_TEMP/similarity-changed"
  fi
  local report="$RUNNER_TEMP/similarity-report.md"
  {
    echo "### Linux · similarity"
    echo
    if [[ "${CODE:-no}" == yes ]]; then
      if [[ -n "${BASE:-}" ]]; then
        echo "Code: the files this change adds or modifies, \`tools/mg similarity --base ${BASE:0:12} --rev HEAD\`."
        run_mg code --base "$BASE" --rev HEAD "${allow[@]}" || rc=1
      else
        echo "Code: every tracked file, \`tools/mg similarity --rev HEAD\`."
        run_mg code --rev HEAD || rc=1
      fi
      echo
    fi
    if [[ -n "$settings" ]]; then
      # shellcheck disable=SC2016 # the backticks are Markdown
      printf '#### Scanner settings: **changed**\n\nThis change edits the `channels:` or `scan:` section of %s (thresholds, channels or exclusions), so it would be scanned with settings it chose. A human reviews the change and merges over this check.\n\n' "$CONFIG"
      rc=1
    fi
    read -r -a areas <<<"${AREAS:-}"
    if ((${#areas[@]} > 0)); then
      for area in "${areas[@]}"; do
        [[ "$area" =~ ^[A-Za-z0-9][A-Za-z0-9_.-]*$ ]] || { echo "Unexpected area name."; rc=1; continue; }
        # Only the sides that exist: an unpaired area fails the scan.
        [[ ! -d "conformance/scenarios/$area" ]] || paths+=("conformance/scenarios/$area")
        [[ ! -d "conformance/goldens/$area" ]] || paths+=("conformance/goldens/$area")
      done
      echo "Test data: areas ${areas[*]}, \`tools/mg similarity --test-data\`. Only flags in files this change adds or modifies fail the check."
      if ((${#paths[@]} > 0)); then
        run_mg data --test-data "${allow[@]}" -- "${paths[@]}" || rc=1
      fi
      echo
    fi
    echo "Only file names, scores and counts are shown here; the reviewer view stays local (\`tools/mg similarity\` in the main checkout)."
  } >"$report"
  cat "$report"
  cat "$report" >>"$GITHUB_STEP_SUMMARY"
  return "$rc"
}

case "${1:-}" in
  select) cmd_select ;;
  pin) pin ;;
  corpus) cmd_corpus "${2:?corpus directory}" ;;
  scan) cmd_scan ;;
  summary) cmd_summary "${2:?kind}" "${3:?result.json}" "${4:?tracked list}" "${5:?changed list}" ;;
  *) echo "usage: similarity.sh select | pin | corpus <dir> | scan | summary <code|data> <result.json> <tracked> <changed>" >&2; exit 2 ;;
esac
