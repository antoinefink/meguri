# The private collection vault (U15, R1938–R1943). Sourced by tools/mg.
#
#   tools/mg private status                 what the vault has: source, copies, token set
#   tools/mg private copy                   a copy in build/private/<run>/copy, and the token set
#   tools/mg private suite [--record <name>] [--keep]
#                                           the private suite on a fresh copy; summary of pass counts
#   tools/mg private scan-outputs [path...] files (default: the staged lines) holding a private token
#   tools/mg private pins                   reviewed public files (PUBLIC_ARTIFACTS) edited since their review
#   tools/mg private refresh-public --head <full-oid> [--shared-main]
#                                           drops the tokens that commit already publishes
#
# A host job. The only source is local-data/anki/ of this checkout (the main
# checkout: no worktree has local-data/), and of it only collection.anki2,
# its -wal and collection.media/ are opened (meguri_testkit::private::vault).
# status, copy and suite refuse when a CI marker is set (R1939) and in
# implementer and fixer worktrees (R1955). meguri-cli is built in the dev
# container and runs in the `private` service: no network, the source
# mounted read-only at /vault for that run, local-data/ hidden. Copies, logs
# and the token set stay in build/private/ (gitignored); job results and logs
# carry counts, sizes, hashes and run paths only, never collection text.
#
# The token set (build/private/tokens/tokens.txt) is rebuilt from each copy:
# meguri-cli private tokens lists the candidate strings, and
# tools/scripts/private-scan/private_scan.py keeps those not already
# committed (HEAD, private results excepted). scan-outputs and the
# pre-commit and pre-merge-commit hooks (tools/hooks/) check files against
# it and name files and lines, never a token. Every job that copies the
# collection installs those hooks first (tools/mg hooks install) and copies
# nothing when it cannot.
#
# One answer in every checkout (docs/process/provenance.md): every worktree
# reads the main checkout's token set, so a rebuild or refresh there reaches
# all of them at their next commit. A copy sets meguri.privateTokenSet=required
# in the repository's config; from then on the hooks refuse a commit when the
# set is missing, empty or unreadable instead of passing unscanned.
# baseline.json beside the set names the commit whose text the set last
# dropped (counts, hashes and that id only); tools/mg doctor reports it.
#
# suite runs the checks meguri-cli registers (U249 adds the R1942 checks),
# scans the summary and only then, with --record, writes it to
# conformance/reports/private/<name>.json. Its copy is deleted at the end
# unless --keep.
#
#   tools/mg anonymize [--source <dir>] [--out <dir>] [--salt-file <file>] [--now <instant>] [--tz <zone>]
#
# The anonymized clone (specs/fixtures/anonymizer.md A2.3): meguri-cli
# anonymize in the private service (the source read-only at /vault; V1, V2,
# V4, V5), then V3: tools/mg oracle check --schedule on the source and on
# the clone at one instant, compared. The default source is a fresh vault
# copy (the vault's rules and refusals apply; the copy is deleted at the
# end); another directory, such as build/fixtures/<...>-10k, is used as it
# is. The clone and the salt stay under build/: a clone of the private
# collection is still private-adjacent.

mg_job anonymize host mg_anonymize \
  "Anonymized clone (specs/fixtures/anonymizer.md): [--source <dir>] [--out <dir>] [--salt-file <file>] [--now <instant>] [--tz <zone>]; default source a fresh vault copy, clone in build/private/<run>/clone, salt build/private/anonymize.salt; V1, V2, V4, V5 by meguri-cli, V3 by tools/mg oracle check --schedule on the source and the clone"

mg_job private host mg_private \
  "Private collection vault (local-data/anki/, read-only, never off this Mac): status | copy | suite [--record <name>] [--keep] | refresh-public --head <full-oid> [--shared-main] | scan-outputs [paths...] (default: the staged lines) | pins (reviewed public files edited since their review; reads no token set); copies and the token set stay in build/private/"

mg_job private-selftest host mg_private_selftest \
  "The vault's copy through the real read-only mount, on a made-up profile folder (no private data; any checkout): meguri-cli private copy in the private service gives the collection, its log and media unchanged, the source keeps every hash, size and mtime, writes through /vault are refused, a read-write source is refused"

mg_job anonymize-check host mg_anonymize_check \
  "The anonymizer end to end on a generated collection (U15): [<size>] [--seed N] (default 10k, seed 1); tools/mg fixtures, then tools/mg anonymize --source on it: the real meguri-cli anonymize (V1, V2, V4, V5) and the oracle's next-state and queue digests of the source and the clone (V3)"

# Environment variables that mark a CI or other remote machine. Keep in step
# with CI_MARKERS in core/crates/meguri-testkit/src/private/vault.rs
# (tools/tests/private.bats compares them).
MG_PRIVATE_CI_MARKERS="CI CONTINUOUS_INTEGRATION BUILDKITE BITBUCKET_BUILD_NUMBER CIRCLECI CI_XCODE_PROJECT CODEBUILD_BUILD_ID DRONE GITHUB_ACTIONS GITLAB_CI JENKINS_URL TEAMCITY_VERSION TF_BUILD TRAVIS"
MG_PRIVATE_SOURCE=local-data/anki
MG_PRIVATE_DIR=build/private
MG_PRIVATE_TOKENS=build/private/tokens/tokens.txt
# What the set last dropped as public: {head, tokens, sha256, at, by}.
MG_PRIVATE_BASELINE=build/private/tokens/baseline.json
# The repository config key that makes a missing token set block commits.
MG_PRIVATE_REQUIRED_KEY=meguri.privateTokenSet
MG_PRIVATE_REPORTS=conformance/reports/private
MG_PRIVATE_SCAN=tools/scripts/private-scan/private_scan.py
# meguri-cli as the dev service builds it (profile `fixtures`), seen from the
# private service, which mounts the same target volume.
MG_PRIVATE_CLI=/cargo-target/fixtures/meguri-cli
# The vault copy, as mg__private_run_on runs it: $1 meguri-cli, $2 the source
# (/vault), $3 the run directory under /work. private copy and
# private-selftest run this one command.
MG_PRIVATE_COPY_SCRIPT='"$1" private copy --source "$2" --out "$3/copy" --access-log "$3/vault-access.log" >"$3/copy.json"'

mg__private_usage() {
  mg_failure usage "$1; usage: tools/mg private status | copy | suite [--record <name>] [--keep] | refresh-public --head <full-oid> [--shared-main] | scan-outputs [paths...] | pins"
  mg_message "private: usage error"
  mg_status error
}

# mg__private_ci_marker: prints the first CI marker set to a non-empty value.
mg__private_ci_marker() {
  local name value
  for name in $MG_PRIVATE_CI_MARKERS; do
    eval "value=\${$name:-}"
    if [ -n "$value" ]; then
      printf '%s\n' "$name"
      return 0
    fi
  done
  return 0
}

# mg__private_guard: records an error and returns 1 when the vault may not
# be used here.
mg__private_guard() {
  local marker role
  marker="$(mg__private_ci_marker)"
  if [ -n "$marker" ]; then
    mg_failure ci-marker "refused: the CI marker $marker is set; the private collection is used only on this Mac, never on a remote machine (R1939)"
    mg_message "private: refused on a CI machine ($marker)"
    mg_status error
    return 1
  fi
  role="$(mg_session_role)"
  case "$role" in
    implementer | fixer)
      mg_failure implementer-worktree "this worktree's role is $role: implementers and fixers never read the private collection (R1955); the vault runs from the main checkout" "tools/mg private (from the main checkout)"
      mg_message "private: refused in a worktree of role $role"
      mg_status error
      return 1
      ;;
  esac
}

# mg__private_source_ok: whether the source holds a regular collection file.
# Only stats it; the vault opens it, in the container.
mg__private_source_ok() {
  [ -f "$MG_ROOT/$MG_PRIVATE_SOURCE/collection.anki2" ] && [ ! -L "$MG_ROOT/$MG_PRIVATE_SOURCE/collection.anki2" ] &&
    [ ! -L "$MG_ROOT/$MG_PRIVATE_SOURCE" ]
}

mg__private_no_source() {
  mg_failure no-source "no $MG_PRIVATE_SOURCE/collection.anki2 in this checkout; the private collection is in the main checkout's local-data/ (never committed)" "tools/mg private status"
  mg_message "private: no private collection here"
  mg_status error
}

mg__private_docker() {
  if ! command -v docker >/dev/null 2>&1 || ! docker info >/dev/null 2>&1; then
    mg_failure docker-unreachable "Docker is not reachable; start Docker Desktop" "tools/mg doctor"
    mg_status error
    return 1
  fi
  local service
  for service in dev private; do
    echo "tools/mg: building image for service '$service'" >&2
    if ! mg_compose --progress plain build "$service" >&2; then
      mg_failure image-build "building the '$service' image failed; see log.txt" \
        "docker compose -p $(mg_compose_project) -f docker/compose.yml build $service"
      mg_status error
      return 1
    fi
  done
  # meguri-cli, built without the collection in reach.
  if ! mg_compose run --rm -T --no-deps --label "meguri.mg.run-id=$MG_RUN_ID" dev \
    bash -c 'cd core && cargo build --profile fixtures -p meguri-cli' </dev/null >&2; then
    mg_failure cli-build "building meguri-cli (profile fixtures) failed; see log.txt"
    mg_status error
    return 1
  fi
}

# mg__private_hooks: installs (or refreshes) the hooks that check commits and
# merges against the token set (tools/mg hooks install). Every job that
# copies the collection rebuilds the token set, and a token set no hook reads
# guards nothing (R1940), so they call this first. Records an error and
# returns 1 when the hooks cannot be installed.
mg__private_hooks() {
  if ! mg_hooks_install; then
    mg_failure "$MG_HOOKS_FAILURE" "the private collection is copied only where the hooks that check commits and merges against its token set are installed (R1940): $MG_HOOKS_MESSAGE" "tools/mg hooks install"
    mg_message "private: the commit hooks cannot be installed, so nothing was copied"
    mg_status error
    return 1
  fi
  # shellcheck disable=SC2086 # one hook name per word
  mg_detail hooks "$(mg_json_array $MG_HOOK_NAMES | jq -c --arg d "$MG_HOOKS_DIR" '[.[] | {name: ., path: ($d + "/" + .)}]')"
}

# mg__private_run <script> [args...]: runs `sh -c <script> sh <meguri-cli>
# /vault [args...]` in the private service, the source mounted read-only at
# /vault. Paths the script writes are under /work (the checkout).
mg__private_run() {
  mg__private_run_on "$MG_ROOT/$MG_PRIVATE_SOURCE" "$@"
}

# mg__private_run_on <dir> <script> [args...]: mg__private_run with <dir>
# (absolute) mounted read-only at /vault.
mg__private_run_on() {
  local source="$1" script="$2"
  shift 2
  mg_compose run --rm -T --no-deps --label "meguri.mg.run-id=$MG_RUN_ID" \
    -v "$source:/vault:ro" \
    private sh -c "$script" sh "$MG_PRIVATE_CLI" /vault ${1+"$@"} </dev/null
}

# mg__private_copy <run dir, relative>: copies the source into <run>/copy and
# rebuilds the token set from it. Sets MG__PRIVATE_COPY (copy.json) and
# MG__PRIVATE_TOKENSET (the token set's counts); records an error and
# returns 1 on failure.
mg__private_copy() {
  local run="$1" rc
  mkdir -p "$MG_ROOT/$run" || {
    mg_failure run-dir "could not create $run"
    mg_status error
    return 1
  }
  chmod 700 "$MG_ROOT/$MG_PRIVATE_DIR" "$MG_ROOT/$run" 2>/dev/null
  mg__private_run "$MG_PRIVATE_COPY_SCRIPT" "/work/$run"
  rc=$?
  if [ "$rc" -ne 0 ] || ! jq -e .collection.sha256 "$MG_ROOT/$run/copy.json" >/dev/null 2>&1; then
    mg_failure copy "the vault copy failed (exit $rc); see log.txt"
    mg_status error
    return 1
  fi
  MG__PRIVATE_COPY="$(jq -c . "$MG_ROOT/$run/copy.json")"
  mg__private_run '"$1" private tokens --copy "$3/copy" --out "$3/candidates.jsonl" >"$3/candidates.json"' "/work/$run"
  rc=$?
  if [ "$rc" -ne 0 ]; then
    rm -f "$MG_ROOT/$run/candidates.jsonl"
    mg_failure tokens "listing the token candidates failed (exit $rc); see log.txt"
    mg_status error
    return 1
  fi
  mkdir -p "$MG_ROOT/$(dirname "$MG_PRIVATE_TOKENS")"
  python3 -I -B "$MG_ROOT/$MG_PRIVATE_SCAN" build --candidates "$MG_ROOT/$run/candidates.jsonl" \
    --repo "$MG_ROOT" --out "$MG_ROOT/$MG_PRIVATE_TOKENS" >"$MG_ROOT/$run/token-set.json"
  rc=$?
  rm -f "$MG_ROOT/$run/candidates.jsonl"
  if [ "$rc" -ne 0 ]; then
    mg_failure token-set "building the token set failed (exit $rc); see log.txt"
    mg_status error
    return 1
  fi
  MG__PRIVATE_TOKENSET="$(jq -c . "$MG_ROOT/$run/token-set.json")"
  # From here on a commit this set cannot check is refused, in every
  # worktree (the key is in the repository's shared config).
  if ! git -C "$MG_ROOT" config "$MG_PRIVATE_REQUIRED_KEY" required ||
     ! mg__private_baseline_write "$MG_ROOT" "$(git -C "$MG_ROOT" rev-parse -q --verify 'HEAD^{commit}' 2>/dev/null || true)" build; then
    mg_failure token-set-marker "the token set was built, but $MG_PRIVATE_REQUIRED_KEY or $MG_PRIVATE_BASELINE could not be written, so a later missing set would not block commits"
    mg_status error
    return 1
  fi
}

# mg__private_main: the main checkout (the one whose token set every
# worktree reads); this checkout when git cannot say.
mg__private_main() {
  local common
  if common="$(git -C "$MG_ROOT" rev-parse --path-format=absolute --git-common-dir 2>/dev/null)" && [ -n "$common" ]; then
    dirname "$common"
  else
    printf '%s\n' "$MG_ROOT"
  fi
}

# mg__private_sha256 <file>: its SHA-256 (macOS has shasum, the containers
# sha256sum).
mg__private_sha256() {
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | cut -c1-64
  else
    sha256sum "$1" | cut -c1-64
  fi
}

# mg__private_baseline_write <checkout> <head> <by>: records beside that
# checkout's token set which commit's text it last dropped. Counts, a hash
# and the commit id only. Returns 1 when it cannot be written.
mg__private_baseline_write() {
  local dir="$1/$(dirname "$MG_PRIVATE_TOKENS")" set="$1/$MG_PRIVATE_TOKENS" tmp
  [ -f "$set" ] || return 1
  tmp="$(mktemp "$dir/baseline.tmp.XXXXXX")" || return 1
  if jq -n --arg head "$2" --arg by "$3" --arg at "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
    --arg sha "$(mg__private_sha256 "$set")" --argjson tokens "$(grep -c '' "$set")" \
    '{head: $head, by: $by, at: $at, sha256: $sha, tokens: $tokens}' >"$tmp" && mv -f "$tmp" "$1/$MG_PRIVATE_BASELINE"; then
    return 0
  fi
  rm -f "$tmp"
  return 1
}

# mg__private_set_facts <checkout>: one JSON object about that checkout's
# token set, from stat, a line count and a hash (never its text): path,
# present, readable, tokens, sha256, modified, baseline (baseline.json, or
# null) and baseline_current (whether it describes these bytes).
mg__private_set_facts() {
  local set="$1/$MG_PRIVATE_TOKENS" base="$1/$MG_PRIVATE_BASELINE" present=false readable=false tokens=0 sha="" modified="" baseline=null
  if [ -e "$set" ] || [ -L "$set" ]; then
    present=true
    if [ -f "$set" ] && [ ! -L "$set" ] && [ -r "$set" ]; then
      readable=true
      tokens="$(grep -c '' "$set")"
      sha="$(mg__private_sha256 "$set")"
      modified="$(date -u -r "$set" +%Y-%m-%dT%H:%M:%SZ 2>/dev/null || true)"
    fi
  fi
  # Anything but one object with a commit id and a hash is no baseline.
  [ ! -f "$base" ] || baseline="$(jq -cs 'if length == 1 and (.[0] | type == "object" and (.head | type == "string" and length > 0) and (.sha256 | type == "string")) then .[0] | {head, by, at, sha256, tokens} else null end' "$base" 2>/dev/null)"
  [ -n "$baseline" ] || baseline=null
  jq -nc --arg path "$set" --argjson present "$present" --argjson readable "$readable" --argjson tokens "${tokens:-0}" \
    --arg sha "$sha" --arg modified "$modified" --argjson baseline "$baseline" \
    '{path: $path, present: $present, readable: $readable, tokens: $tokens, sha256: $sha, modified: $modified,
      baseline: $baseline, baseline_current: ($baseline != null and $sha != "" and $baseline.sha256 == $sha)}'
}

# mg__private_required: whether this repository is expected to have a token
# set (the hooks' rule: tools/hooks/pre-commit). -e stats the folder only.
mg__private_required() {
  local main
  main="$(mg__private_main)"
  [ "$(git -C "$MG_ROOT" config --get "$MG_PRIVATE_REQUIRED_KEY" 2>/dev/null)" = required ] ||
    [ -e "$main/$MG_PRIVATE_SOURCE" ] || [ -L "$main/$MG_PRIVATE_SOURCE" ] ||
    [ -e "$MG_ROOT/$MG_PRIVATE_SOURCE" ] || [ -L "$MG_ROOT/$MG_PRIVATE_SOURCE" ]
}

mg__private_status() {
  local src="$MG_ROOT/$MG_PRIVATE_SOURCE" present=false bytes=0 wal=false media=false copies tokens
  if mg__private_source_ok; then
    present=true
    # ls stats the files without opening them.
    bytes="$(ls -ln "$src/collection.anki2" | awk '{print $5}')"
    [ -f "$src/collection.anki2-wal" ] && wal=true
    [ -d "$src/collection.media" ] && [ ! -L "$src/collection.media" ] && media=true
  fi
  copies="$(
    for d in "$MG_ROOT/$MG_PRIVATE_DIR"/*/copy; do
      [ -d "$d" ] || continue
      d="${d%/copy}"
      printf '%s\n' "${d#"$MG_ROOT"/}"
    done | jq -R . | jq -sc .
  )"
  tokens=null
  if [ -f "$MG_ROOT/$MG_PRIVATE_TOKENS" ]; then
    tokens="$(jq -c --arg p "$MG_PRIVATE_TOKENS" '{path: $p} + .' "$MG_ROOT/$(dirname "$MG_PRIVATE_TOKENS")/meta.json" 2>/dev/null ||
      jq -nc --arg p "$MG_PRIVATE_TOKENS" '{path: $p}')"
  fi
  # The set the hooks read here: the main checkout's, in a worktree too.
  mg_detail token_set_now "$(mg__private_set_facts "$(mg__private_main)" | jq -c --argjson required "$(mg__private_required && echo true || echo false)" '. + {required: $required}')"
  mg_detail source "$(jq -nc --arg p "$MG_PRIVATE_SOURCE" --argjson present "$present" --argjson bytes "${bytes:-0}" \
    --argjson wal "$wal" --argjson media "$media" \
    '{path: $p, present: $present, collection_bytes: $bytes, wal: $wal, media_folder: $media}')"
  mg_detail copies "$copies"
  mg_detail token_set "$tokens"
  if [ "$present" = true ]; then
    mg_message "private: source present, $(jq length <<<"$copies") copies, token set $([ "$tokens" = null ] && echo missing || echo present)"
  else
    mg_message "private: no source at $MG_PRIVATE_SOURCE"
  fi
  mg_status pass
}

mg__private_copy_job() {
  [ "$#" -eq 0 ] || {
    mg__private_usage "copy takes no arguments"
    return 0
  }
  mg__private_source_ok || {
    mg__private_no_source
    return 0
  }
  mg__private_hooks || return 0
  mg__private_docker || return 0
  local run="$MG_PRIVATE_DIR/$MG_RUN_ID"
  if ! mg__private_copy "$run"; then
    rm -rf "$MG_ROOT/$run/copy"
    return 0
  fi
  mg_detail copy "$(jq -nc --arg p "$run/copy" --argjson c "$MG__PRIVATE_COPY" '{path: $p} + $c')"
  mg_detail token_set "$(jq -nc --arg p "$MG_PRIVATE_TOKENS" --argjson t "$MG__PRIVATE_TOKENSET" '{path: $p} + $t')"
  mg_artifact copy "$MG_ROOT/$run/copy" "copy of the private collection (never commit or send it)"
  mg_message "private: copy at $run/copy, token set rebuilt"
  mg_status pass
}

# mg__private_scan <json out> <scan args...>: runs the scanner with every
# token set this checkout sees (the main checkout's and its own). Returns
# its exit code, 3 when there is no token set, 4 when one is there but is
# not a readable, non-empty regular file.
mg__private_scan() {
  local out="$1" common main sets=() f
  shift
  common="$(git -C "$MG_ROOT" rev-parse --path-format=absolute --git-common-dir 2>/dev/null)" || common=""
  main="${common:+$(dirname "$common")}"
  for f in "${main:+$main/$MG_PRIVATE_TOKENS}" "$MG_ROOT/$MG_PRIVATE_TOKENS"; do
    [ -n "$f" ] || continue
    { [ -e "$f" ] || [ -L "$f" ]; } || continue
    # A set that is there but cannot be read is an error, never "no set".
    { [ -f "$f" ] && [ ! -L "$f" ] && [ -r "$f" ] && [ -s "$f" ]; } || return 4
    [ "${#sets[@]}" -gt 0 ] && [ "${sets[1]}" = "$f" ] && continue
    sets[${#sets[@]}]=--tokens
    sets[${#sets[@]}]="$f"
  done
  [ "${#sets[@]}" -gt 0 ] || return 3
  python3 -I -B "$MG_ROOT/$MG_PRIVATE_SCAN" scan "${sets[@]}" ${1+"$@"} >"$out"
}

# mg__private_report_scan <scan json>: one failure per file holding a token.
mg__private_report_scan() {
  jq -c '.files[]' "$1" | while IFS= read -r file; do
    local path where
    path="$(jq -r .path <<<"$file")"
    where="$(jq -r 'if .binary then "binary file" else "line " + (.lines | map(tostring) | join(", ")) end' <<<"$file")"
    # mg__quote_args starts each word with a space.
    mg_failure "private-token:$path" "$path holds a private token ($where); the token is never named" \
      "tools/mg private scan-outputs$(mg__quote_args "$path")"
  done
}

mg__private_scan_outputs() {
  local out="$MG_RUN_DIR/scan.json" rc
  if [ "$#" -eq 0 ]; then
    mg__private_scan "$out" --staged --repo "$MG_ROOT"
  else
    mg__private_scan "$out" "$@"
  fi
  rc=$?
  case "$rc" in
    0 | 1)
      mg_artifact report "$out" "files holding a private token (no tokens)"
      mg_detail scan "$(jq -c '{files, files_scanned, tokens}' "$out")"
      mg_count total "$(jq .files_scanned "$out")"
      mg_count failed "$(jq '.files | length' "$out")"
      ;;
  esac
  case "$rc" in
    0)
      mg_message "private: no private token in $(jq .files_scanned "$out") files"
      mg_status pass
      ;;
    1)
      mg__private_report_scan "$out"
      mg_message "private: $(jq '.files | length' "$out") files hold a private token (R1940)"
      mg_status fail
      ;;
    4)
      mg_failure token-set-unreadable "a private token set ($MG_PRIVATE_TOKENS of the main checkout or this one) is not a readable, non-empty regular file, so nothing was scanned" "tools/mg doctor"
      mg_message "private: the token set cannot be read"
      mg_status error
      ;;
    3)
      mg_failure no-token-set "no private token set ($MG_PRIVATE_TOKENS of the main checkout or this one); tools/mg private copy builds it" "tools/mg private copy"
      mg_message "private: no token set"
      mg_status error
      ;;
    *)
      mg_failure scan "the scanner failed (exit $rc); see log.txt"
      mg_status error
      ;;
  esac
}

mg__private_suite() {
  local record="" keep=false
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --record)
        record="${2:-}"
        shift 2 || shift
        ;;
      --keep)
        keep=true
        shift
        ;;
      *)
        mg__private_usage "unknown suite argument '$1'"
        return 0
        ;;
    esac
  done
  if [ -n "$record" ] && ! printf '%s' "$record" | grep -Eq '^[A-Za-z0-9][A-Za-z0-9._-]*$'; then
    mg__private_usage "--record takes a name of letters, digits, '.', '_' and '-' (such as M1)"
    return 0
  fi
  mg__private_source_ok || {
    mg__private_no_source
    return 0
  }
  mg__private_hooks || return 0
  mg__private_docker || return 0
  local run="$MG_PRIVATE_DIR/$MG_RUN_ID" rc
  [ "$keep" = true ] || mg_on_exit "rm -rf '$MG_ROOT/$run/copy'"
  mg__private_copy "$run" || return 0
  mg__private_run '"$1" private suite --copy "$3/copy" --out "$3/summary.json" --log "$3/suite.log" >/dev/null' "/work/$run"
  rc=$?
  if [ "$rc" -gt 1 ] || ! jq -e '.report_version' "$MG_ROOT/$run/summary.json" >/dev/null 2>&1; then
    mg_failure suite "the private suite did not run (exit $rc); see log.txt"
    mg_status error
    return 0
  fi
  # The summary leaves build/private/ only after the scan.
  local scan="$MG_RUN_DIR/summary-scan.json" scan_rc
  mg__private_scan "$scan" "$MG_ROOT/$run/summary.json"
  scan_rc=$?
  if [ "$scan_rc" -ne 0 ]; then
    if [ "$scan_rc" -eq 1 ]; then
      mg_failure summary-private-token "the suite's summary holds a private token; nothing was recorded (R1940)"
      mg_status fail
    else
      mg_failure scan "scanning the summary failed (exit $scan_rc); nothing was recorded"
      mg_status error
    fi
    return 0
  fi
  local summary
  summary="$(jq -c . "$MG_ROOT/$run/summary.json")"
  mg_detail summary "$summary"
  mg_detail copy "$(jq -nc --argjson c "$MG__PRIVATE_COPY" --argjson kept "$keep" --arg p "$run/copy" '{path: $p, kept: $kept} + $c')"
  if [ -n "$record" ]; then
    mkdir -p "$MG_ROOT/$MG_PRIVATE_REPORTS"
    cp "$MG_ROOT/$run/summary.json" "$MG_ROOT/$MG_PRIVATE_REPORTS/$record.json"
    mg_artifact report "$MG_ROOT/$MG_PRIVATE_REPORTS/$record.json" "private suite summary (pass counts only)"
  fi
  jq -r '.checks | to_entries[] | select(.value.error == true or (.value.failed // 0) > 0) | .key' <<<"$summary" |
    while IFS= read -r name; do
      mg_failure "check:$name" "private check $name: $(jq -c --arg n "$name" '.checks[$n]' <<<"$summary")"
    done
  mg_count total "$(jq '.checks | length' <<<"$summary")"
  mg_count failed "$(jq '[.checks[] | select(.error == true or (.failed // 0) > 0)] | length' <<<"$summary")"
  if [ "$(jq '.checks | length' <<<"$summary")" -eq 0 ]; then
    mg_message "private: suite ran no check (none registered yet; U249 adds them)"
    mg_status warn
  elif [ "$rc" -eq 0 ]; then
    mg_message "private: suite passed ($(jq '.checks | length' <<<"$summary") checks)"
    mg_status pass
  else
    mg_message "private: suite failed"
    mg_status fail
  fi
}

# Owner-controlled idle token/Git writer window required; legacy writers do
# not coordinate. This is never invoked by a hook, gate or catch-up.
mg__private_refresh_public() {
  local out="$MG_RUN_DIR/refresh-public.json" rc
  if [ "$#" -lt 2 ] || [ "$#" -gt 3 ] || [ "$1" != --head ] ||
     { [ "$#" -eq 3 ] && [ "$3" != --shared-main ]; }; then
    mg__private_usage "refresh-public requires --head <full-oid> [--shared-main]"
    return 0
  fi
  mg__private_guard || return 0
  python3 -I -B "$MG_ROOT/$MG_PRIVATE_SCAN" refresh-public --repo "$MG_ROOT" "$@" >"$out"
  rc=$?
  if [ "$rc" -ne 0 ]; then
    mg_failure refresh-public "refresh refused or interrupted; owner must inspect private backup before retrying"
    mg_status error
    return 0
  fi
  local target="$MG_ROOT"
  [ "$#" -ne 3 ] || target="$(mg__private_main)"
  # A set built before the key existed: from now on its absence is refused.
  if ! git -C "$MG_ROOT" config "$MG_PRIVATE_REQUIRED_KEY" required ||
     ! mg__private_baseline_write "$target" "$2" refresh-public; then
    mg_failure baseline "the token set was refreshed, but $MG_PRIVATE_BASELINE could not be written; tools/mg doctor will report the baseline as unknown"
    mg_status error
    return 0
  fi
  mg_artifact report "$out" "public-baseline refresh (counts and hashes only)"
  mg_detail refresh_public "$(jq -c . "$out")"
  mg_message "private: removed $(jq .removed "$out") public entries; $(jq .after "$out") tokens remain"
  mg_status pass
}

# tools/mg private pins: reads no token set and no private data, so it runs
# anywhere (CI too). A stale pin is not a leak: the file's later lines were
# scanned when they were committed. It says where a whole-file scan, or a
# merge from a base older than the file's reviewed lines, will flag them.
mg__private_pins() {
  local out="$MG_RUN_DIR/pins.json" rc
  if [ "$#" -ne 0 ]; then
    mg__private_usage "pins takes no argument"
    return 0
  fi
  python3 -I -B "$MG_ROOT/$MG_PRIVATE_SCAN" pins --repo "$MG_ROOT" >"$out"
  rc=$?
  if [ "$rc" -ne 0 ]; then
    mg_failure pins "listing the pinned public files failed (exit $rc); see log.txt"
    mg_status error
    return 0
  fi
  mg_artifact report "$out" "pinned public files whose bytes changed since their review (paths only)"
  mg_detail pins "$(jq -c '{commit, pinned, stale: (.stale | length), absent: ((.absent | length) + .absent_vendor)}' "$out")"
  mg_count total "$(jq .pinned "$out")"
  mg_count warned "$(jq '(.stale | length) + (.absent | length) + .absent_vendor' "$out")"
  if [ "$(jq '(.stale | length) + (.absent | length) + .absent_vendor' "$out")" -eq 0 ]; then
    mg_message "private: all $(jq .pinned "$out") reviewed public files still have their reviewed bytes"
    mg_status pass
  else
    mg_message "private: $(jq '.stale | length' "$out") of $(jq .pinned "$out") reviewed public files changed since their review, $(jq '(.absent | length) + .absent_vendor' "$out") are gone; their reviewed lines are scanned like new ones (docs/process/provenance.md#one-answer-in-every-checkout)"
    mg_status warn
  fi
}

mg_private() {
  local command="${1:-}"
  [ "$#" -eq 0 ] || shift
  case "$command" in
    pins)
      mg__private_pins "$@"
      return 0
      ;;
    refresh-public)
      mg__private_refresh_public "$@"
      return 0
      ;;
    scan-outputs)
      mg__private_scan_outputs "$@"
      return 0
      ;;
    status | copy | suite) ;;
    "")
      mg__private_usage "a command is required"
      return 0
      ;;
    *)
      mg__private_usage "unknown command '$command'"
      return 0
      ;;
  esac
  mg__private_guard || return 0
  case "$command" in
    status) mg__private_status ;;
    copy) mg__private_copy_job "$@" ;;
    suite) mg__private_suite "$@" ;;
  esac
}

# ------------------------------------------------------------ self-test
#
# tools/mg private-selftest: the vault's copy as tools/mg private copy runs
# it (images, meguri-cli, the private service, the read-only mount at
# /vault, MG_PRIVATE_COPY_SCRIPT), on a made-up profile folder in its run
# directory. Docker Desktop's file sharing is what makes the mount read-only,
# so only a real run shows that the vault accepts it and that writes through
# it fail. It never reads local-data/, so it runs in any checkout.

# mg__private_selftest_files <dir>: every file under <dir> as {relative
# path: {sha256, bytes, mtime_ns}}, one JSON object.
mg__private_selftest_files() {
  python3 -I -B -c 'import hashlib, json, os, sys
root = sys.argv[1]
out = {}
for top, dirs, names in os.walk(root):
    dirs.sort()
    for name in sorted(names):
        path = os.path.join(top, name)
        st = os.lstat(path)
        with open(path, "rb") as f:
            digest = hashlib.sha256(f.read()).hexdigest()
        out[os.path.relpath(path, root)] = {"sha256": digest, "bytes": st.st_size, "mtime_ns": st.st_mtime_ns}
print(json.dumps(out, sort_keys=True))' "$1"
}

# mg__private_selftest_check <id> <message> <command...>: one check, passed
# when the command succeeds.
mg__private_selftest_check() {
  local id="$1" message="$2"
  shift 2
  if "$@"; then
    MG__SELFTEST_PASSED=$((MG__SELFTEST_PASSED + 1))
  else
    MG__SELFTEST_FAILED=$((MG__SELFTEST_FAILED + 1))
    mg_failure "$id" "$message"
  fi
}

# Writes through /vault ($2), each in a subshell (a failed redirection of a
# builtin may end sh); writes one "<probe> written" or "<probe> refused:
# <error>" line each to $3/write-probes.txt. Only EROFS counts as refused by
# the mount.
MG_PRIVATE_WRITE_PROBES='v="$2"
exec >"$3/write-probes.txt"
[ -f "$v/collection.anki2" ] && [ -f "$v/prefs21.db" ] || { echo "/vault does not show the profile folder" >&2; exit 3; }
probe() { name="$1"; shift; if err="$( ( "$@" ) 2>&1)"; then echo "$name written"; else echo "$name refused: $err" | tr "\n" " "; echo; fi; }
append() { printf x >>"$1"; }
create() { printf x >"$1"; }
probe append-collection append "$v/collection.anki2"
probe create-file create "$v/made-up-new-file"
probe touch-log touch "$v/collection.anki2-wal"
probe remove-preferences rm "$v/prefs21.db"
probe rename-collection mv "$v/collection.anki2" "$v/moved"
probe create-media create "$v/collection.media/new.jpg"'

mg_private_selftest() {
  if [ "$#" -ne 0 ]; then
    mg_failure usage "private-selftest takes no arguments"
    mg_status error
    return 0
  fi
  local rel="${MG_RUN_DIR#"$MG_ROOT"/}" profile="$MG_RUN_DIR/profile" before after rc ok
  # A made-up profile folder: the collection, its log and media, and two
  # files the vault never opens.
  if ! mkdir -p "$profile/collection.media" "$profile/addons21/1" ||
    ! { printf 'SQLite format 3\0' && head -c 20000 /dev/urandom; } >"$profile/collection.anki2" ||
    ! head -c 3000 /dev/urandom >"$profile/collection.anki2-wal" ||
    ! head -c 500 /dev/urandom >"$profile/collection.media/a b.jpg" ||
    ! printf 'made-up preferences\n' >"$profile/prefs21.db" ||
    ! printf '{"config": {"api_key": "made-up"}}\n' >"$profile/addons21/1/meta.json" ||
    ! before="$(mg__private_selftest_files "$profile")"; then
    mg_failure profile "could not write the made-up profile folder under $rel"
    mg_status error
    return 0
  fi
  printf '%s\n' "$before" >"$MG_RUN_DIR/source-before.json"
  mg__private_docker || return 0
  MG__SELFTEST_PASSED=0 MG__SELFTEST_FAILED=0

  # The copy, exactly as tools/mg private copy runs it.
  mg__private_run_on "$profile" "$MG_PRIVATE_COPY_SCRIPT" "/work/$rel"
  rc=$?
  mg__private_selftest_check copy "meguri-cli private copy through the read-only mount exited $rc; see log.txt" [ "$rc" -eq 0 ]
  if [ "$rc" -eq 0 ]; then
    mg_artifact copy "$MG_RUN_DIR/copy" "copy of the made-up profile folder"
    mg_detail copy "$(jq -c . "$MG_RUN_DIR/copy.json")"
    mg__private_selftest_check copy-report "copy.json does not give the source's hashes and sizes" \
      jq -e --argjson s "$before" '.collection.sha256 == $s["collection.anki2"].sha256
        and .collection.bytes == $s["collection.anki2"].bytes
        and .wal.sha256 == $s["collection.anki2-wal"].sha256
        and .media.files == 1 and .media.bytes == $s["collection.media/a b.jpg"].bytes and .media.skipped == 0' \
      "$MG_RUN_DIR/copy.json"
    mg__private_selftest_check copy-files "the copy is not the collection, its log and the media file, byte for byte" \
      jq -en --argjson s "$before" --argjson c "$(mg__private_selftest_files "$MG_RUN_DIR/copy")" \
      '($c | map_values(.sha256)) == ($s | with_entries(select(.key == "collection.anki2" or .key == "collection.anki2-wal" or .key == "collection.media/a b.jpg")) | map_values(.sha256))'
    mg__private_selftest_check access-log "the vault's access log names more than the collection, its log and the media" \
      [ "$(cat "$MG_RUN_DIR/vault-access.log" 2>/dev/null)" = "$(printf 'collection.anki2\ncollection.anki2-wal\ncollection.media/\ncollection.media/a b.jpg')" ]
  fi

  # Writes through the same mount.
  mg__private_run_on "$profile" "$MG_PRIVATE_WRITE_PROBES" "/work/$rel"
  rc=$?
  mg_artifact report "$MG_RUN_DIR/write-probes.txt" "writes tried through /vault"
  ok=1
  [ "$rc" -eq 0 ] && [ "$(grep -c ' refused: .*Read-only file system' "$MG_RUN_DIR/write-probes.txt" 2>/dev/null)" -eq 6 ] &&
    [ "$(grep -c . "$MG_RUN_DIR/write-probes.txt")" -eq 6 ] && ok=0
  mg__private_selftest_check write-refused "a write through the read-only /vault mount was not refused as a read-only file system (exit $rc): $(grep -v ' refused: .*Read-only file system' "$MG_RUN_DIR/write-probes.txt" 2>/dev/null | tr '\n' ';')" \
    [ "$ok" -eq 0 ]

  # The vault refuses the same folder when it is not mounted read-only (the
  # checkout's own read-write mount at /work).
  mg__private_run_on "$profile" \
    '"$1" private copy --source "$3/profile" --out "$3/copy-rw" 2>"$3/read-write.stderr"; echo "$?" >"$3/read-write.rc"' "/work/$rel"
  ok=1
  [ "$(cat "$MG_RUN_DIR/read-write.rc" 2>/dev/null)" = 1 ] && grep -q 'not on a read-only mount' "$MG_RUN_DIR/read-write.stderr" &&
    [ ! -e "$MG_RUN_DIR/copy-rw" ] && ok=0
  mg__private_selftest_check read-write-refused "meguri-cli private copy did not refuse a source on a read-write mount; see read-write.stderr" \
    [ "$ok" -eq 0 ]

  # The source as it was: same files, hashes, sizes and modification times.
  after="$(mg__private_selftest_files "$profile")"
  printf '%s\n' "$after" >"$MG_RUN_DIR/source-after.json"
  mg__private_selftest_check source-unchanged "the source folder changed: $(jq -rn --argjson b "$before" --argjson a "$after" \
    '[($b + $a) | keys[] as $k | select($b[$k] != $a[$k]) | $k] | join(", ")')" \
    [ "$before" = "$after" ]

  mg_count total $((MG__SELFTEST_PASSED + MG__SELFTEST_FAILED))
  mg_count passed "$MG__SELFTEST_PASSED"
  mg_count failed "$MG__SELFTEST_FAILED"
  if [ "$MG__SELFTEST_FAILED" -eq 0 ]; then
    mg_message "private-selftest: $MG__SELFTEST_PASSED checks pass (copy through the read-only mount, writes refused, source unchanged)"
    mg_status pass
  else
    mg_message "private-selftest: $MG__SELFTEST_FAILED of $((MG__SELFTEST_PASSED + MG__SELFTEST_FAILED)) checks fail"
    mg_status fail
  fi
}

# ------------------------------------------------------------- anonymize

mg__anonymize_usage() {
  mg_failure usage "$1; usage: tools/mg anonymize [--source <dir>] [--out <dir>] [--salt-file <file>] [--now <instant>] [--tz <zone>]"
  mg_message "anonymize: usage error"
  mg_status error
}

# mg__anonymize_abs <path>: the path made absolute against the checkout.
mg__anonymize_abs() {
  case "$1" in
    /*) printf '%s\n' "$1" ;;
    *) printf '%s\n' "$MG_ROOT/$1" ;;
  esac
}

# mg__anonymize_under_build <absolute path>: whether it lies under the
# checkout's build/ (written as such: no '.' or '..' component). The
# private service sees the checkout only, at /work.
mg__anonymize_under_build() {
  case "$1" in
    */../* | */.. | */./* | */.) return 1 ;;
    "$MG_ROOT"/build/?*) return 0 ;;
    *) return 1 ;;
  esac
}

# mg__anonymize_host_zone: this Mac's IANA zone ($TZ when it names one, or
# the target of /etc/localtime).
mg__anonymize_host_zone() {
  local link
  case "${TZ:-}" in
    */* | UTC) printf '%s\n' "$TZ"; return 0 ;;
  esac
  link="$(readlink /etc/localtime 2>/dev/null)" || return 0
  case "$link" in
    */zoneinfo/*) printf '%s\n' "${link#*/zoneinfo/}" ;;
  esac
}

# mg__anonymize_clock <source> <--now or ""> <--tz or "">: sets
# MG__ANON_NOW and MG__ANON_TZ (A2.3): the values given, else the source
# manifest's by collection-check.md K1, else 12:00 today in this Mac's
# zone. Records a usage error and returns 1 when there is none.
mg__anonymize_clock() {
  local source="$1" now="$2" tz="$3" manifest="$1/manifest.json"
  if [ -f "$manifest" ]; then
    [ -n "$now" ] || now="$(jq -r 'if (.now_ms | type) == "number" then (.now_ms / 1000 | floor | todate) else empty end' "$manifest")"
    [ -n "$tz" ] || tz="$(jq -r '.utc_offset_secs as $o
      | if ($o | type) != "number" then empty
        elif $o == 0 then "UTC"
        elif ($o % 3600) == 0 and $o >= -43200 and $o <= 50400 then
          ($o / 3600 | floor) as $h | if $h > 0 then "Etc/GMT-\($h)" else "Etc/GMT+\(0 - $h)" end
        else empty end' "$manifest")"
  else
    [ -n "$tz" ] || tz="$(mg__anonymize_host_zone)"
    if [ -z "$now" ] && [ -n "$tz" ]; then
      now="$(python3 -I -c 'import datetime, sys, zoneinfo
z = zoneinfo.ZoneInfo(sys.argv[1])
print(datetime.datetime.now(z).replace(hour=12, minute=0, second=0, microsecond=0).isoformat())' "$tz" 2>/dev/null)"
    fi
  fi
  if [ -z "$now" ] || [ -z "$tz" ]; then
    mg__anonymize_usage "no instant and zone for the oracle checks: pass --now and --tz (collection-check.md K1)"
    return 1
  fi
  MG__ANON_NOW="$now"
  MG__ANON_TZ="$tz"
}

# mg__anonymize_v3 <source> <clone>: A17 V3. Runs tools/mg oracle check
# --schedule on both at MG__ANON_NOW in MG__ANON_TZ and compares counts,
# timing, check_db, schedule and changed_tables. On its own, the clone's
# Check Database must find nothing (U15 Validation): equal reports do not
# pass when both find the same problems. Sets MG__ANON_V3 to pass, fail or
# error and writes v3.json (the members and deck ids that differ, and the
# clone's problems as kind: count, or its check_db rejection).
mg__anonymize_v3() {
  local source="$1" clone="$2" side dir result repro
  for side in source clone; do
    dir="$source"
    [ "$side" = source ] || dir="$clone"
    repro="tools/mg oracle check $dir --schedule --now $MG__ANON_NOW --tz $MG__ANON_TZ"
    echo "anonymize: V3 on the $side -> $repro" >&2
    result="$("$MG_TOOLS/mg" oracle check "$dir" --schedule --now "$MG__ANON_NOW" --tz "$MG__ANON_TZ" </dev/null)"
    result="${result##*$'\n'}"
    if [ -z "$result" ] || [ ! -f "$result" ]; then
      mg_failure "v3/$side" "tools/mg oracle check wrote no result for the $side; see log.txt" "$repro"
      MG__ANON_V3=error
      return 0
    fi
    mg_artifact oracle-check "$result" "$side"
    if ! jq -e '.details.check.schedule.sha256' "$result" >/dev/null 2>&1; then
      mg_failure "v3/$side" "oracle check on the $side gave no schedule: $(jq -r '.status + ": " + ([.failures[]? | .id + " " + .message] | join("; "))' "$result")" "$repro"
      MG__ANON_V3=error
      return 0
    fi
    jq -c '.details.check' "$result" >"$MG_RUN_DIR/v3-$side.json"
  done
  jq -n --slurpfile s "$MG_RUN_DIR/v3-source.json" --slurpfile c "$MG_RUN_DIR/v3-clone.json" '
    $s[0] as $s | $c[0] as $c
    | {members: [("counts", "timing", "check_db", "schedule") as $k | select($s[$k] != $c[$k]) | $k],
       decks: ([$s.schedule.decks, $c.schedule.decks] | map(keys) | add | unique
         | map(select($s.schedule.decks[.] != $c.schedule.decks[.]))),
       changed_tables: [$c.changed_tables[] | select(. != "col" and . != "config")
         | select(. as $t | ($s.changed_tables | index($t)) == null)],
       sha256: {source: $s.schedule.sha256, clone: $c.schedule.sha256},
       clone_check_db: (if ($c.check_db | type) != "object" then {error: ($c.check_db | tostring)}
         elif ($c.check_db | has("error")) then {error: $c.check_db.error}
         else $c.check_db | to_entries | map(select(.value != 0)) | sort_by(.key) | from_entries end),
       now: $s.now, tz: $s.tz}
    | . + {equal: ((.members | length) == 0 and (.changed_tables | length) == 0)}' >"$MG_RUN_DIR/v3.json"
  if [ ! -s "$MG_RUN_DIR/v3.json" ]; then
    mg_failure v3/compare "could not compare the oracle reports; see log.txt" "tools/mg anonymize"
    MG__ANON_V3=error
    return 0
  fi
  mg_detail v3 "$(jq -c . "$MG_RUN_DIR/v3.json")"
  if jq -e '.equal and .clone_check_db == {}' "$MG_RUN_DIR/v3.json" >/dev/null; then
    MG__ANON_V3=pass
    return 0
  fi
  MG__ANON_V3=fail
  if jq -e '.clone_check_db != {}' "$MG_RUN_DIR/v3.json" >/dev/null; then
    mg_failure v3/clone_check_db "the oracle's Check Database finds problems in the clone: $(jq -r '.clone_check_db | to_entries | map("\(.key) \(.value)") | join(", ")' "$MG_RUN_DIR/v3.json") (a clone must have none; when the source has the same, repair the source first)" "tools/mg anonymize"
  fi
  jq -r '.members[] | select(. != "schedule")' "$MG_RUN_DIR/v3.json" | while IFS= read -r member; do
    mg_failure "v3/$member" "the oracle reports a different $member for the clone than for the source" "tools/mg anonymize"
  done
  if jq -e '.members | index("schedule")' "$MG_RUN_DIR/v3.json" >/dev/null; then
    mg_failure v3/schedule "the clone's schedule differs from the source's (schedule.sha256); decks whose digests differ: $(jq -r '.decks | join(", ")' "$MG_RUN_DIR/v3.json")" "tools/mg anonymize"
  fi
  jq -r '.changed_tables[]' "$MG_RUN_DIR/v3.json" | while IFS= read -r table; do
    mg_failure "v3/changed_$table" "Check Database changed $table in the clone but not in the source" "tools/mg anonymize"
  done
}

# tools/mg anonymize [--source <dir>] [--out <dir>] [--salt-file <file>] [--now <instant>] [--tz <zone>]
mg_anonymize() {
  local source="" out="" salt="" now="" tz="" vault=false
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --source | --out | --salt-file | --now | --tz)
        [ "$#" -ge 2 ] || {
          mg__anonymize_usage "$1 needs a value"
          return 0
        }
        case "$1" in
          --source) source="$2" ;;
          --out) out="$2" ;;
          --salt-file) salt="$2" ;;
          --now) now="$2" ;;
          --tz) tz="$2" ;;
        esac
        shift 2
        ;;
      *)
        mg__anonymize_usage "unknown argument '$1'"
        return 0
        ;;
    esac
  done
  local run="$MG_PRIVATE_DIR/$MG_RUN_ID"
  out="$(mg__anonymize_abs "${out:-$run/clone}")"
  salt="$(mg__anonymize_abs "${salt:-$MG_PRIVATE_DIR/anonymize.salt}")"
  mg__anonymize_under_build "$out" || {
    mg__anonymize_usage "--out must lie under build/ (nothing is written outside it)"
    return 0
  }
  mg__anonymize_under_build "$salt" || {
    mg__anonymize_usage "--salt-file must lie under build/"
    return 0
  }
  if [ -z "$source" ]; then
    vault=true
    mg__private_guard || return 0
    mg__private_source_ok || {
      mg__private_no_source
      return 0
    }
    mg__private_hooks || return 0
  else
    source="$(mg__anonymize_abs "$source")"
    if [ ! -f "$source/collection.anki2" ]; then
      mg_failure no_collection "$source has no collection.anki2" "tools/mg anonymize --source <dir with a collection.anki2>"
      mg_message "anonymize: no collection.anki2 in the source"
      mg_status error
      return 0
    fi
    source="$(cd "$source" && pwd -P)"
    case "$source/" in
      "$(cd "$MG_ROOT" && pwd -P)/$(dirname "$MG_PRIVATE_SOURCE")/"*)
        mg__anonymize_usage "the private collection is read only through the vault: leave --source out"
        return 0
        ;;
      *:*)
        mg__anonymize_usage "$source has a ':', which a bind mount cannot carry"
        return 0
        ;;
    esac
  fi
  mg__private_docker || return 0
  if [ "$vault" = true ]; then
    mg_on_exit "rm -rf '$MG_ROOT/$run/copy'"
    mg__private_copy "$run" || return 0
    source="$MG_ROOT/$run/copy"
  fi
  mg__anonymize_clock "$source" "$now" "$tz" || return 0
  mkdir -p "$(dirname "$out")" "$(dirname "$salt")"
  chmod 700 "$MG_ROOT/$MG_PRIVATE_DIR" 2>/dev/null

  local err="$MG_RUN_DIR/anonymize.stderr" rc kind
  mg__private_run_on "$source" '"$1" anonymize --copy "$2" --out "$3" --salt-file "$4"' \
    "/work/${out#"$MG_ROOT"/}" "/work/${salt#"$MG_ROOT"/}" 2>"$err" >/dev/null
  rc=$?
  cat "$err" >&2
  if [ "$rc" -ne 0 ] || [ ! -f "$out/anonymize.json" ]; then
    kind="$(sed -n 's/^meguri-cli anonymize: \([a-z_]*\):.*$/\1/p' "$err" | tail -n 1)"
    mg_failure "${kind:-anonymize}" "meguri-cli anonymize failed (exit $rc): $(grep '^meguri-cli anonymize:' "$err" | tail -n 1)" "tools/mg anonymize"
    mg_message "anonymize: ${kind:-meguri-cli failed}"
    mg_status error
    return 0
  fi
  local report
  report="$(jq -c . "$out/anonymize.json")"
  mg_detail report "$report"
  mg_detail clone "$(jq -nc --arg p "${out#"$MG_ROOT"/}" --arg now "$MG__ANON_NOW" --arg tz "$MG__ANON_TZ" --argjson vault "$vault" \
    '{path: $p, now: $now, tz: $tz, from_vault: $vault}')"
  mg_artifact clone "$out" "anonymized clone (private-adjacent: stays on this Mac)"
  mg_artifact report "$out/anonymize.json" "anonymize.json (counts only)"

  MG__ANON_V3=error
  mg__anonymize_v3 "$source" "$out"

  local check status passed=0 failed=0 leak
  for check in structure rows leak media; do
    if [ "$check" = leak ]; then
      status="$(jq -r '.checks.leak.status' <<<"$report")"
    else
      status="$(jq -r --arg c "$check" '.checks[$c]' <<<"$report")"
    fi
    case "$status" in
      pass | warn) passed=$((passed + 1)) ;;
      *)
        failed=$((failed + 1))
        mg_failure "$check" "anonymize.json: check $check is $status" "tools/mg anonymize"
        ;;
    esac
  done
  case "$MG__ANON_V3" in
    pass) passed=$((passed + 1)) ;;
    *) failed=$((failed + 1)) ;;
  esac
  mg_count total 5
  mg_count passed "$passed"
  mg_count failed "$failed"
  leak="$(jq -r '.checks.leak.status' <<<"$report")"
  if [ "$failed" -gt 0 ] && { [ "$MG__ANON_V3" != error ] || [ "$failed" -gt 1 ]; }; then
    mg_message "anonymize: $failed of 5 checks fail (clone at ${out#"$MG_ROOT"/})"
    mg_status fail
  elif [ "$MG__ANON_V3" = error ]; then
    mg_message "anonymize: clone at ${out#"$MG_ROOT"/}; V1, V2, V4, V5 ran, V3 (the oracle) could not"
    mg_status error
  elif [ "$leak" = warn ]; then
    mg_message "anonymize: every check passes; V4 lists $(jq '.checks.leak.locations | length' <<<"$report") review items (clone at ${out#"$MG_ROOT"/})"
    mg_status warn
  else
    mg_message "anonymize: every check passes (clone at ${out#"$MG_ROOT"/})"
    mg_status pass
  fi
}

# ------------------------------------------------------- anonymize-check
#
# tools/mg anonymize-check [<size>] [--seed N]: U15's "clone of a generated
# 10k collection", end to end with nothing faked: tools/mg fixtures <size>,
# then tools/mg anonymize --source on that collection, both nested. It passes
# (or warns, for V4 review items) only when anonymize does with V3 run: the
# oracle's next-state and queue digests equal for every deck and card, and
# the clone's Check Database clean. Where the oracle cannot run (a unit
# branch carrying spec-side oracle changes) it is error, as anonymize is.

mg__anonymize_check_usage() {
  mg_failure usage "$1; usage: tools/mg anonymize-check [<size>] [--seed N]" "tools/mg anonymize-check 10k"
  mg_message "anonymize-check: usage error"
  mg_status error
}

mg_anonymize_check() {
  local size="" seed=1 gen dir anon status repro
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --seed)
        [ "$#" -ge 2 ] || {
          mg__anonymize_check_usage "--seed needs a value"
          return 0
        }
        seed="$2"
        shift 2
        ;;
      -*)
        mg__anonymize_check_usage "unknown option '$1'"
        return 0
        ;;
      *)
        [ -z "$size" ] || {
          mg__anonymize_check_usage "one size only"
          return 0
        }
        size="$1"
        shift
        ;;
    esac
  done
  size="${size:-10k}"
  case "$seed" in '' | *[!0-9]*)
    mg__anonymize_check_usage "--seed takes a non-negative integer, not '$seed'"
    return 0
    ;;
  esac
  if ! printf '%s' "$size" | grep -Eqi '^[1-9][0-9]*[km]?$'; then
    mg__anonymize_check_usage "<size> is a card count such as 10k, not '$size'"
    return 0
  fi

  repro="tools/mg fixtures $size --seed $seed"
  echo "anonymize-check: $repro" >&2
  gen="$("$MG_TOOLS/mg" fixtures "$size" --seed "$seed" </dev/null)"
  gen="${gen##*$'\n'}"
  if [ -z "$gen" ] || [ ! -f "$gen" ]; then
    mg_failure fixtures "tools/mg fixtures wrote no result; see log.txt" "$repro"
    mg_status error
    return 0
  fi
  mg_artifact fixtures "$gen" "tools/mg fixtures result"
  dir="$(jq -r '.details.fixtures[0].dir // empty' "$gen")"
  if [ "$(jq -r .status "$gen")" != pass ] || [ -z "$dir" ]; then
    mg_failure fixtures "tools/mg fixtures: $(jq -r .message "$gen")" "$repro"
    mg_message "anonymize-check: generating the $size collection failed"
    mg_status error
    return 0
  fi

  repro="tools/mg anonymize --source $dir"
  echo "anonymize-check: $repro" >&2
  anon="$("$MG_TOOLS/mg" anonymize --source "$dir" </dev/null)"
  anon="${anon##*$'\n'}"
  if [ -z "$anon" ] || [ ! -f "$anon" ]; then
    mg_failure anonymize "tools/mg anonymize wrote no result; see log.txt" "$repro"
    mg_status error
    return 0
  fi
  mg_artifact anonymize "$anon" "tools/mg anonymize result"
  mg_detail anonymize "$(jq -c '{status, message, summary, v3: .details.v3, report: .details.report, clone: .details.clone}' "$anon")"
  jq -c '.failures[]' "$anon" | while IFS= read -r failure; do
    mg_failure "$(jq -r .id <<<"$failure")" "$(jq -r .message <<<"$failure")" "$repro"
  done
  mg_count total "$(jq '.summary.total // 0' "$anon")"
  mg_count passed "$(jq '.summary.passed // 0' "$anon")"
  mg_count failed "$(jq '.summary.failed // 0' "$anon")"
  status="$(jq -r .status "$anon")"
  case "$status" in
    pass | warn)
      if jq -e '.details.v3.equal == true and .details.v3.clone_check_db == {}
        and .details.v3.sha256.source == .details.v3.sha256.clone' "$anon" >/dev/null; then
        mg_message "anonymize-check: the clone of the $size collection schedules as its source in the oracle (schedule $(jq -r '.details.v3.sha256.source[0:12]' "$anon")); $(jq -r .message "$anon")"
        mg_status "$status"
      else
        mg_failure v3 "tools/mg anonymize reported $status without an equal V3 and a clean Check Database" "$repro"
        mg_message "anonymize-check: no V3 evidence for the $size collection"
        mg_status fail
      fi
      ;;
    fail)
      mg_message "anonymize-check: the clone of the $size collection fails: $(jq -r .message "$anon")"
      mg_status fail
      ;;
    *)
      mg_message "anonymize-check: $(jq -r .message "$anon")"
      mg_status error
      ;;
  esac
}
