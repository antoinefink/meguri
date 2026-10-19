# Code-similarity scan against Anki releases (U2, R1957). Sourced by tools/mg.
#
# Host jobs: they need refs/ of this checkout (the Anki corpus), so they run
# from the main checkout and refuse to run in an implementer worktree. Each one
# starts the `similarity` service itself with the corpus mounted read-only at
# /refs, labelled with the run id so a cancelled run's container is removed.
# MG_SIMILARITY_REFS overrides the corpus directory (scanner development only).

mg_job similarity host mg_similarity \
  "Similarity scan of Meguri files against every Anki tag since 2.1.50 (R1957); --base <ref> changed files only, --rev <ref> that commit's tree, -- <paths> given files"
mg_job similarity-nightly host mg_similarity_nightly \
  "JPlag comparison of Meguri with selected Anki tags (nightly second opinion); report stays under build/"
mg_job similarity-selftest host mg_similarity_selftest \
  "Similarity scanner tests and calibration against the Anki corpus (tools/similarity/tests)"

# mg__similarity_guard: records an error and returns 1 in an implementer or
# fixer worktree (R1955: their environment never holds Anki's source).
mg__similarity_guard() {
  local role
  role="$(mg_session_role)"
  case "$role" in
    implementer | fixer)
      mg_failure implementer-worktree "this is a $role worktree: the similarity scan runs only from the main checkout, which holds refs/; the orchestrator passes back its implementer view" "tools/mg $MG_JOB (from the main checkout)"
      mg_message "$MG_JOB: refused in a $role worktree (R1955)"
      mg_status error
      return 1
      ;;
  esac
}

# mg__similarity_corpus: prints the corpus directory (holding anki/), or
# nothing when there is none.
mg__similarity_corpus() {
  local refs="${MG_SIMILARITY_REFS:-$MG_ROOT/refs}"
  [ -d "$refs/anki" ] && (cd "$refs" && pwd -P)
  return 0
}

mg__similarity_no_corpus() {
  mg_failure no-corpus "no Anki repository at ${MG_SIMILARITY_REFS:-refs}/anki; run from the main checkout, whose refs/ holds the clone" "tools/mg $MG_JOB (from the main checkout)"
  mg_message "$MG_JOB: no Anki corpus"
  mg_status error
}

# mg__similarity_run <refs dir> <command...>: run a command in the similarity
# service with the corpus mounted.
mg__similarity_run() {
  local refs="$1"
  shift
  if ! command -v docker >/dev/null 2>&1 || ! docker info >/dev/null 2>&1; then
    mg_failure docker-unreachable "Docker is not reachable; start Docker Desktop" "tools/mg doctor"
    mg_status error
    return 125
  fi
  echo "tools/mg: building image for service 'similarity'" >&2
  if ! mg_compose --progress plain build similarity >&2; then
    mg_failure image-build "building the 'similarity' image failed; see log.txt" \
      "docker compose -p $(mg_compose_project) -f docker/compose.yml build similarity"
    mg_status error
    return 125
  fi
  mg_compose run --rm -T --no-deps --label "meguri.mg.run-id=$MG_RUN_ID" \
    -v "$refs:/refs:ro" similarity "$@" </dev/null
}

# File lists are NUL-separated end to end (git -z, scanner --null): git
# quotes names with non-ASCII, tab or newline characters otherwise, and the
# scanner would skip them.

# Files a full scan considers: tracked plus untracked, not ignored.
mg__similarity_all_files() {
  git -C "$1" ls-files -z --cached --others --exclude-standard
}

# Directories the scanner never scans: the `<dir>/**` entries of scan.exclude
# in tools/similarity/config.yaml (keep that a flat list of quoted globs, one
# per line), always including oracle/. One per line, with a trailing slash.
mg__similarity_excluded_dirs() {
  echo oracle/
  awk '
    /^[^ #]/ { scan = ($0 ~ /^scan:/); ex = 0; next }
    scan && /^  [A-Za-z_]+:/ { ex = ($0 ~ /^  exclude:/); next }
    scan && ex && /^    - / {
      v = $0; sub(/^    - */, "", v); sub(/[ \t]+#.*$/, "", v); gsub(/["\047]/, "", v)
      if (v ~ /\/\*\*$/) print substr(v, 1, length(v) - 2)
    }' "$MG_ROOT/tools/similarity/config.yaml"
}

# mg__similarity_materialize <rev> <NUL list> <dir> [check]: checks out, under
# <dir>, the listed files of <rev> outside the excluded directories (so oracle/
# never lands in build/), plus its interop vocabulary. With `check`, listed
# paths that <rev> lacks are left out (given paths, which mg_similarity has
# normalized for the excluded-directory check; checkout-index would fail on
# them). Uses a private index, so the checkout's own index and files stay
# untouched. Returns 1 after recording an error.
mg__similarity_materialize() {
  local rev="$1" list="$2" dir="$3" check="${4:-}" index="$MG_RUN_DIR/rev-index" p d skip
  local excluded=()
  while IFS= read -r d; do
    [ -n "$d" ] && excluded[${#excluded[@]}]="$d"
  done <<EOF
$(mg__similarity_excluded_dirs)
EOF
  rm -f "$index"
  if ! GIT_INDEX_FILE="$index" git -C "$MG_ROOT" read-tree "$rev"; then
    rm -f "$index"
    mg_failure bad-rev "git read-tree $rev failed"
    mg_status error
    return 1
  fi
  mkdir -p "$dir"
  {
    while IFS= read -r -d '' p; do
      skip=""
      for d in "${excluded[@]}"; do
        case "$p" in "$d"*) skip=yes; break ;; esac
      done
      [ -z "$skip" ] || continue
      [ -z "$check" ] || git -C "$MG_ROOT" cat-file -e "$rev:$p" 2>/dev/null || continue
      printf '%s\0' "$p"
    done <"$list"
    GIT_INDEX_FILE="$index" git -C "$MG_ROOT" ls-files -z -- 'specs/*/interop-vocab.txt'
  } | GIT_INDEX_FILE="$index" git -C "$MG_ROOT" checkout-index -z --stdin --prefix="$dir/"
  local rc=$?
  rm -f "$index"
  if [ "$rc" -ne 0 ]; then
    rm -rf "$dir"
    mg_failure materialize "checking out the files of $rev under the run directory failed; see log.txt"
    mg_status error
    return 1
  fi
}

# mg__similarity_relpath <path>: sets MG__RELPATH to the path relative to the
# checkout root with its `.`, `..` and empty segments resolved, without
# touching the filesystem (the scanner does the same), so that `./oracle/x`
# meets the excluded-directory checks as `oracle/x`. Empty for the root itself.
# Returns 1 for a path outside the checkout.
mg__similarity_relpath() {
  local p="$1" rest seg n out=()
  MG__RELPATH=""
  case "$p" in
    "$MG_ROOT") p="" ;;
    "$MG_ROOT"/*) p="${p#"$MG_ROOT"/}" ;;
    /*) return 1 ;;
  esac
  rest="$p"
  while [ -n "$rest" ]; do
    case "$rest" in
      */*) seg="${rest%%/*}" rest="${rest#*/}" ;;
      *) seg="$rest" rest="" ;;
    esac
    case "$seg" in
      "" | .) ;;
      ..)
        n=${#out[@]}
        [ "$n" -gt 0 ] || return 1
        unset "out[$((n - 1))]"
        ;;
      *) out[${#out[@]}]="$seg" ;;
    esac
  done
  for seg in ${out[0]+"${out[@]}"}; do
    MG__RELPATH="${MG__RELPATH:+$MG__RELPATH/}$seg"
  done
}

# mg__similarity_given <path>...: the given (normalized) paths as a NUL list,
# each once: checkout-index refuses a path listed twice.
mg__similarity_given() {
  [ "$#" -eq 0 ] || printf '%s\0' "$@" | sort -zu
}

mg_similarity() {
  local base="" rev="" rebuild="" given="" paths=() a
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --base) base="${2:-}"; shift 2 || shift ;;
      --base=*) base="${1#--base=}"; shift ;;
      --rev) rev="${2:-}"; shift 2 || shift ;;
      --rev=*) rev="${1#--rev=}"; shift ;;
      --rebuild-index) rebuild=--rebuild; shift ;;
      --)
        shift
        for a in "$@"; do
          given=yes
          if ! mg__similarity_relpath "$a"; then
            mg_failure bad-path "'$a' is outside the checkout $MG_ROOT; give paths relative to it"
            mg_status error
            return 0
          fi
          # The root is a directory, which the scanner never scans.
          [ -z "$MG__RELPATH" ] || paths[${#paths[@]}]="$MG__RELPATH"
        done
        break
        ;;
      *)
        mg_failure usage "unknown argument '$1'; usage: tools/mg similarity [--base <ref>] [--rev <ref>] [--rebuild-index] [-- paths...]"
        mg_status error
        return 0
        ;;
    esac
  done
  mg__similarity_guard || return 0
  local refs
  refs="$(mg__similarity_corpus)"
  [ -n "$refs" ] || { mg__similarity_no_corpus; return 0; }

  local list="$MG_RUN_DIR/files.lst" root_rel="." rel="${MG_RUN_DIR#"$MG_ROOT"/}" tree="$MG_RUN_DIR/tree"
  : >"$list"
  if [ -n "$base" ] && ! git -C "$MG_ROOT" rev-parse --verify -q "$base^{commit}" >/dev/null; then
    mg_failure bad-base "'$base' is not a commit"
    mg_status error
    return 0
  fi
  if [ -n "$rev" ]; then
    # Scan a commit's tree: materialize its files (and its interop vocabulary)
    # under the run directory.
    if ! git -C "$MG_ROOT" rev-parse --verify -q "$rev^{commit}" >/dev/null; then
      mg_failure bad-rev "'$rev' is not a commit"
      mg_status error
      return 0
    fi
    if [ -n "$given" ]; then
      mg__similarity_given ${paths[0]+"${paths[@]}"} >"$list"
    elif [ -n "$base" ]; then
      git -C "$MG_ROOT" diff -z --name-only --no-renames --diff-filter=d "$base" "$rev" >"$list" || {
        mg_failure bad-base "git diff $base $rev failed"
        mg_status error
        return 0
      }
    else
      git -C "$MG_ROOT" ls-tree -r -z --name-only "$rev" >"$list"
    fi
    mg__similarity_materialize "$rev" "$list" "$tree" ${given:+check} || return 0
    root_rel="$rel/tree"
  elif [ -n "$given" ]; then
    mg__similarity_given ${paths[0]+"${paths[@]}"} >"$list"
  elif [ -n "$base" ]; then
    { git -C "$MG_ROOT" diff -z --name-only --no-renames --diff-filter=d "$base"; git -C "$MG_ROOT" ls-files -z --others --exclude-standard; } |
      sort -zu >"$list"
  else
    mg__similarity_all_files "$MG_ROOT" >"$list"
  fi

  local out="$MG_RUN_DIR/scan" rc
  mg__similarity_run "$refs" python3 -m meguri_similarity scan \
    --refs /refs --cache /work/build/similarity-index \
    --root "/work/$root_rel" --vocab-root "/work/$root_rel" \
    --files-from "/work/$rel/files.lst" --null --out "/work/$rel/scan" $rebuild
  rc=$?
  rm -rf "$tree"
  [ "$rc" -eq 125 ] && return 0
  mg__similarity_record "$out/summary.json" "$rc"
}

# mg__similarity_record <summary.json> <scanner exit code>
mg__similarity_record() {
  local summary="$1" rc="$2"
  if [ ! -f "$summary" ]; then
    mg_failure scanner-exit "the scanner exited with code $rc without a summary; see log.txt"
    mg_status error
    return 0
  fi
  local dir error
  dir="$(dirname "$summary")"
  error="$(jq -r '.error // empty' "$summary")"
  if [ -n "$error" ]; then
    mg_failure similarity:error "$error"
    mg_message "similarity: $error"
    mg_status error
    return 0
  fi
  mg_artifact similarity-implementer "$dir/implementer.md" "implementer view"
  mg_artifact similarity-implementer-json "$dir/implementer.json"
  mg_artifact similarity-reviewer "$dir/reviewer.md" "reviewer view (names Anki paths; human review only)"
  mg_artifact similarity-reviewer-json "$dir/reviewer.json"
  mg_count total "$(jq '.counts.scanned' "$summary")"
  mg_count passed "$(jq '.counts.clean + .counts.allowlisted' "$summary")"
  mg_count failed "$(jq '.counts.flagged + .counts.errors' "$summary")"
  mg_count skipped "$(jq '.counts.skipped' "$summary")"
  mg_count allowlisted "$(jq '.counts.allowlisted' "$summary")"
  mg_detail corpus "$(jq -c '.corpus' "$summary")"
  mg_detail timing "$(jq -c '.timing' "$summary")"
  mg_detail counts "$(jq -c '.counts' "$summary")"
  jq -c '.flags[] | {
      id: ("similarity:" + .path + ":" + .channel),
      message: (.path + " lines " + ([.regions[] | "\(.[0])-\(.[1])"] | join(", ")) + ": " + (.reasons | join("; ")) + "; rewrite this region from the spec"),
      repro: ("tools/mg similarity -- " + (.path | @sh))}' "$summary" >>"$MG_STATE/failures.jsonl"
  jq -c '.errors[] | {id: "similarity:error", message: ., repro: "tools/mg similarity"}' "$summary" >>"$MG_STATE/failures.jsonl"
  jq -c '(.missing // [])[] | {id: "similarity:missing",
      message: (. + ": listed for the scan but not in the scanned tree; it was not checked"),
      repro: ("tools/mg similarity -- " + (. | @sh))}' "$summary" >>"$MG_STATE/failures.jsonl"
  local status
  status="$(jq -r '.status' "$summary")"
  mg_message "$(jq -r '"similarity: \(.counts.scanned) files scanned, \(.counts.flagged) flagged, \(.counts.allowlisted) allowlisted" + (if .counts.missing > 0 then ", \(.counts.missing) listed but missing" else "" end) + "; corpus \(.corpus.tags) tags \(.corpus.first_tag)..\(.corpus.last_tag)"' "$summary")"
  case "$status" in
    pass | warn | fail | error) mg_status "$status" ;;
    *) mg_status error ;;
  esac
}

mg_similarity_nightly() {
  local refs rel="${MG_RUN_DIR#"$MG_ROOT"/}" rc
  mg__similarity_guard || return 0
  refs="$(mg__similarity_corpus)"
  [ -n "$refs" ] || { mg__similarity_no_corpus; return 0; }
  mg__similarity_all_files "$MG_ROOT" >"$MG_RUN_DIR/files.lst"
  mg__similarity_run "$refs" python3 -m meguri_similarity jplag \
    --refs /refs --root /work --files-from "/work/$rel/files.lst" --null --out "/work/$rel/jplag"
  rc=$?
  [ "$rc" -eq 125 ] && return 0
  local summary="$MG_RUN_DIR/jplag/jplag-summary.json"
  if [ ! -f "$summary" ]; then
    mg_failure jplag-exit "the JPlag run exited with code $rc without a summary; see log.txt"
    mg_status error
    return 0
  fi
  local report error
  for report in "$MG_RUN_DIR"/jplag/*.jplag; do
    [ -f "$report" ] && mg_artifact jplag-report "$report" "open with: java -jar jplag.jar -M VIEW <file> (embeds Anki sources; main checkout only)"
  done
  mg_detail jplag "$(jq -c . "$summary")"
  error="$(jq -r '.error // empty' "$summary")"
  if [ -n "$error" ]; then
    mg_failure jplag:error "$error" "tools/mg similarity-nightly"
    mg_message "similarity-nightly: $error"
    mg_status error
    return 0
  fi
  jq -c '.languages | to_entries[] | select(.value.error) |
    {id: ("jplag:" + .key), message: .value.error, repro: "tools/mg similarity-nightly"}' "$summary" >>"$MG_STATE/failures.jsonl"
  mg_message "$(jq -r '"similarity-nightly: " + ([.languages | to_entries[] | "\(.key) " + (if .value.skipped then "skipped (\(.value.skipped))" elif .value.error then "error" else "max \(.value.max_similarity * 100 | floor)%" end)] | join(", ")) + "; tags " + (.tags | join(" "))' "$summary")"
  mg_status "$(jq -r '.status' "$summary")"
}

mg_similarity_selftest() {
  local refs tap="$MG_RUN_DIR/similarity-selftest.tap" rel="${MG_RUN_DIR#"$MG_ROOT"/}" rc
  mg__similarity_guard || return 0
  refs="$(mg__similarity_corpus)"
  if [ -z "$refs" ]; then
    # Without a corpus the unit tests still run; calibration tests skip.
    refs="$MG_RUN_DIR/no-corpus"
    mkdir -p "$refs"
  fi
  mg__similarity_run "$refs" python3 tools/similarity/tests/run_tests.py --out "/work/$rel" ${1+"$@"} >"$tap"
  rc=$?
  [ "$rc" -eq 125 ] && return 0
  cat "$tap" >&2
  mg_artifact tap "$tap"
  if [ -f "$MG_RUN_DIR/calibration.json" ]; then
    mg_artifact calibration "$MG_RUN_DIR/calibration.json" "calibration scores"
    mg_detail calibration "$(jq -c . "$MG_RUN_DIR/calibration.json")"
  fi
  mg_tap_report "$tap" "$rc" "tools/mg similarity-selftest -k"
  if [ ! -d "$refs/anki" ] && [ "$(head -n 1 "$MG_STATE/status")" = pass ]; then
    mg_message "$(cat "$MG_STATE/message") (no Anki corpus: calibration skipped; run from the main checkout)"
    mg_status warn
  fi
}
