# Anki oracle jobs (oracle/, AGPL quarantine). Sourced by tools/mg.
#
# A host job: it builds the oracle image with a content-addressed tag and runs
# oracle containers through compose (service `oracle`, internal network).
# Implementers use these commands as black boxes and never read oracle/. Their
# worktrees have none (U2's sparse checkout), so the image is built from the
# main worktree's oracle/ (mg_oracle__tree): tools/mg reads it, they never do.
# test, bump and lock refuse to run there (mg_oracle__own_tree): they copy or
# rewrite oracle/ files, and a copy under that checkout's build/ is outside
# U2's deny rules. Copies they make are deleted when they finish.
#
# --platform apple (hello, record, compare, time; arm64 Mac only) runs the same
# driver natively through oracle/macos/run-native.sh, from a venv under the
# oracle tree's checkout (build/oracle-macos-venv/, covered by U2's deny
# rules), for overlay goldens (specs/conformance/float-determinism.md F6, F7).
# lock --platform apple writes oracle/macos/requirements-macos.lock.

mg_job oracle host mg_oracle \
  "Anki oracle: build | hello [--platform apple] [--tz Z] | record [--platform apple] [--jobs N] [filter...] | compare [--platform apple] [--jobs N] [filter...] | recordings <filter>... | check <dir> [--now T] [--tz Z] [--deck D]... [--render-note N]... [--schedule] | time <dir> [--platform apple] [--op N]... [--now T] [--tz Z] [--repeat N] | bump <version> | lock [--platform apple] | test [pattern]"

MG_ORACLE_UV_VERSION=0.12.24
# uv resolves for this platform tag; the lock still hashes every published wheel.
MG_ORACLE_LOCK_PLATFORM=x86_64-manylinux_2_40
# The native lane's lock: the same versions as requirements.lock (its
# constraint), resolved for an arm64 Mac.
MG_ORACLE_MACOS_LOCK_PLATFORM=aarch64-apple-darwin
MG_ORACLE_MACOS_LOCK=macos/requirements-macos.lock

mg_oracle__usage() {
  mg_failure usage "usage: tools/mg oracle build | hello [--platform apple] [--tz Z] | record [--platform apple] [--jobs N] [filter...] | compare [--platform apple] [--jobs N] [filter...] | recordings <filter>... | check <dir> [--now T] [--tz Z] [--deck D]... [--render-note N]... [--schedule] | time <dir> [--platform apple] [--op N]... [--now T] [--tz Z] [--repeat N] | bump <version> | lock [--platform apple] | test [pattern]" "tools/mg oracle build"
  mg_message "oracle: $1"
  mg_status error
}

mg_oracle__sha256() {
  if command -v sha256sum >/dev/null 2>&1; then sha256sum | cut -d' ' -f1; else shasum -a 256 | cut -d' ' -f1; fi
}

# mg_oracle__inputs_hash <oracle dir>: sha256 over the image inputs (mode,
# content hash and path of each file the Dockerfile copies), so the tag changes
# exactly when the image can.
mg_oracle__inputs_hash() {
  (
    cd "$1" || exit 1
    {
      printf '%s\n' Dockerfile Dockerfile.dockerignore entrypoint.sh requirements.lock
      find driver -type f ! -path '*/__pycache__/*' ! -name '*.pyc'
    } | LC_ALL=C sort | while IFS= read -r f; do
      mode=644
      [ -x "$f" ] && mode=755
      printf '%s %s %s\n' "$mode" "$(mg_oracle__sha256 <"$f")" "$f"
    done
  ) | mg_oracle__sha256
}

# mg_oracle__tag <oracle dir>: <anki version>-<16 hex of the inputs hash>
mg_oracle__tag() {
  local version hash
  version="$(sed -n 's/^anki==\([^ ]*\).*/\1/p' "$1/requirements.lock" | head -n 1)"
  [ -n "$version" ] || return 1
  hash="$(mg_oracle__inputs_hash "$1")" || return 1
  printf '%s-%s\n' "$version" "$(printf '%s' "$hash" | cut -c1-16)"
}

# mg_oracle__tree: sets MG_ORACLE_DIR to the oracle/ tree to build from: this
# checkout's, or the main worktree's (the first `git worktree list` entry) when
# this checkout has none. It refuses the main worktree's when this branch has
# changed oracle/ since it forked and the main worktree's HEAD lacks those
# changes (a unit branch carrying its spec side's driver work): the run would
# replay, or record goldens, on another oracle than the branch's.
mg_oracle__tree() {
  local main head base
  MG_ORACLE_DIR="$MG_ROOT/oracle"
  [ -f "$MG_ORACLE_DIR/Dockerfile" ] && return 0
  main="$(git -C "$MG_ROOT" worktree list --porcelain 2>/dev/null | sed -n '1s/^worktree //p')"
  if [ -n "$main" ] && [ -f "$main/oracle/Dockerfile" ]; then
    head="$(git -C "$main" rev-parse -q --verify HEAD 2>/dev/null)"
    base="$(git -C "$MG_ROOT" merge-base "${head:-HEAD}" HEAD 2>/dev/null)"
    if [ -n "$head" ] && [ -n "$base" ] &&
      ! git -C "$MG_ROOT" diff --quiet "$base" HEAD -- oracle/ &&
      ! git -C "$MG_ROOT" diff --quiet "$head" HEAD -- oracle/; then
      mg_failure oracle-tree-differs "this branch changes oracle/ since $(git -C "$MG_ROOT" rev-parse --short "$base"), and the main worktree ($main, at $(git -C "$main" rev-parse --short HEAD)) does not have those changes, so its oracle/ is not this branch's oracle; run oracle jobs in a worktree that has oracle/ (the unit's spec worktree) or after the branch is integrated"
      mg_status error
      return 1
    fi
    MG_ORACLE_DIR="$main/oracle"
    echo "oracle: this checkout has no oracle/; using the main worktree's" >&2
    return 0
  fi
  mg_failure no-oracle-tree "neither this checkout nor its main worktree (${main:-none found}) has oracle/Dockerfile"
  mg_status error
  return 1
}

# mg_oracle__own_tree <what it does>: refuses (error) when this checkout has no
# oracle/, so the main worktree's tree is never copied or rewritten from here.
mg_oracle__own_tree() {
  [ -f "$MG_ROOT/oracle/Dockerfile" ] && return 0
  mg_failure no-oracle-tree "$1, and this checkout has no oracle/; run it in a worktree that has oracle/"
  mg_status error
  return 1
}

# mg_oracle__compose <tag> <context dir> compose args...
mg_oracle__compose() {
  local tag="$1" ctx="$2"
  shift 2
  MEGURI_ORACLE_TAG="$tag" MEGURI_ORACLE_CONTEXT="$ctx" mg_compose "$@"
}

# mg_oracle__build <oracle dir>: builds and sets MG_ORACLE_TAG, MG_ORACLE_ID.
mg_oracle__build() {
  local dir="$1" log rc
  log="$MG_RUN_DIR/build-$(basename "$1").log"
  if ! MG_ORACLE_TAG="$(mg_oracle__tag "$dir")"; then
    mg_failure tag "cannot compute the image tag: no anki== pin in $dir/requirements.lock"
    mg_status error
    return 1
  fi
  echo "oracle: building meguri-oracle:$MG_ORACLE_TAG from $dir" >&2
  mg_oracle__compose "$MG_ORACLE_TAG" "$dir" --progress plain build oracle 2>&1 | tee "$log" >&2
  rc=${PIPESTATUS[0]}
  mg_artifact log "$log" "oracle image build"
  if [ "$rc" -ne 0 ]; then
    mg_failure image-build "building meguri-oracle:$MG_ORACLE_TAG failed; see $(basename "$log")"
    mg_status error
    return 1
  fi
  MG_ORACLE_ID="$(docker image inspect "meguri-oracle:$MG_ORACLE_TAG" --format '{{.Id}}' 2>/dev/null)"
  if [ -z "$MG_ORACLE_ID" ]; then
    mg_failure image-inspect "meguri-oracle:$MG_ORACLE_TAG is missing after the build"
    mg_status error
    return 1
  fi
}

# mg_oracle__run <tag> <context> [compose run options...] -- <container args...>
# Every container carries the run label, so a cancelled run can remove it.
mg_oracle__run() {
  local tag="$1" ctx="$2"
  shift 2
  mg_oracle__compose "$tag" "$ctx" run --rm -T --no-deps --label "meguri.mg.run-id=$MG_RUN_ID" "$@"
}

mg_oracle__cleanup() {
  local ids
  ids="$(docker ps -aq --filter "label=meguri.mg.run-id=$MG_RUN_ID" 2>/dev/null)"
  # shellcheck disable=SC2086 # one container id per word
  [ -z "$ids" ] || docker rm -f $ids >/dev/null 2>&1
  [ -z "${MG_ORACLE_STAGED:-}" ] || mg_oracle__unstage "$MG_ORACLE_STAGED"
  [ -z "${MG_ORACLE_NATIVE_SCRATCH:-}" ] || rm -rf "$MG_ORACLE_NATIVE_SCRATCH"
  return 0
}

# mg_oracle__unstage <staged oracle dir>: deletes the copied image inputs (the
# driver among them) and keeps requirements.in and requirements.lock.
mg_oracle__unstage() {
  rm -rf "$1/driver" "$1/Dockerfile" "$1/Dockerfile.dockerignore" "$1/entrypoint.sh"
}

# mg_oracle__hello <tag> <context> <tz> <out file>: one worker, one hello.
mg_oracle__hello() {
  printf '{"cmd":"hello"}\n' | mg_oracle__run "$1" "$2" -e "TZ=$3" oracle >"$4"
  jq -e '.ok == true' "$4" >/dev/null 2>&1
}

# mg_oracle__lock <dir> [apple]: regenerate <dir>/requirements.lock from
# requirements.in, or with apple <dir>/macos/requirements-macos.lock pinned to
# requirements.lock's versions (uv in the oracle's base image, default
# network: it only talks to PyPI).
mg_oracle__lock() {
  local dir="$1" base cmd
  base="$(sed -n 's/^FROM \(python:[^ ]*\).*/\1/p' "$dir/Dockerfile" | head -n 1)"
  if [ "${2:-}" = apple ]; then
    mkdir -p "$dir/macos"
    cmd="uv pip compile --quiet --generate-hashes --python-version 3.12 \
      --python-platform $MG_ORACLE_MACOS_LOCK_PLATFORM --constraint requirements.lock \
      --custom-compile-command 'tools/mg oracle lock --platform apple' \
      requirements.in -o $MG_ORACLE_MACOS_LOCK"
  else
    cmd="uv pip compile --quiet --generate-hashes --python-version 3.12 \
      --python-platform $MG_ORACLE_LOCK_PLATFORM \
      --custom-compile-command 'tools/mg oracle lock' \
      requirements.in -o requirements.lock"
  fi
  docker run --rm --init --label "meguri.mg.run-id=$MG_RUN_ID" -v "$dir:/lock" -w /lock "$base" sh -c "
    pip install --quiet --disable-pip-version-check --root-user-action=ignore uv==$MG_ORACLE_UV_VERSION &&
    $cmd" >&2
}

# mg_oracle__harness <tag> <context> <goldens dir> harness args...: record or compare
# with conformance/scenarios and conformance/fixtures read-only (workers find
# fixtures at $ORACLE_FIXTURES, storage-ops.md P1.4), goldens and the run
# directory writable, and this run's fault proxy API and recorder when it has
# them.
mg_oracle__harness() {
  local tag="$1" ctx="$2" goldens="$3"
  shift 3
  mkdir -p "$goldens" "$MG_ROOT/conformance/fixtures"
  mg_oracle__run "$tag" "$ctx" -u "$(id -u):$(id -g)" \
    ${MG_ORACLE_SYNC_FAULTS:+-e "ORACLE_SYNC_FAULTS=$MG_ORACLE_SYNC_FAULTS"} \
    ${MG_ORACLE_SYNC_RECORDER:+-e "ORACLE_SYNC_RECORDER=$MG_ORACLE_SYNC_RECORDER"} \
    -v "$MG_ROOT/conformance/scenarios:/in/scenarios:ro" \
    -v "$MG_ROOT/conformance/fixtures:/in/fixtures:ro" -e ORACLE_FIXTURES=/in/fixtures \
    -v "$goldens:/in/goldens" \
    -v "$MG_RUN_DIR:/out" \
    oracle harness "$@" </dev/null
}

# --- Actor scenarios (specs/conformance/sync-ops.md S8.2, S9) ---------------
# Their workers create one fault proxy per run, named for the run, in this
# checkout's `sync-faults` service (ORACLE_SYNC_FAULTS=sync-faults:8474),
# which runs share and which stays up between them. Recordings route the
# oracle actors through a sync-recorder container of their own
# (ORACLE_SYNC_RECORDER), labeled with the run, so a cancelled run removes it.
# Both are only on oracle-internal.

MG_ORACLE_SYNC_FAULTS=""
MG_ORACLE_SYNC_RECORDER=""
MG_ORACLE_SYNC_CONTAINERS="" # this run's recorders

# mg_oracle__actor_scenarios [filter...]: the ids of the actor scenarios that
# the filters select, by meguri-cli's rules: an id, a directory prefix or a
# pattern (`*` also matches `/`), with or without suffix; every actor scenario
# without filters. An actor scenario is one whose parsed value has a top-level
# `actors` member, as meguri-cli's scenario parser reads it (`conformance
# to-json`), so a quoted key or a flow mapping counts as `actors:` does. When
# the parser cannot run, it records an error and returns 1: a run that missed
# an actor scenario would start its workers without the fault proxy.
mg_oracle__actor_scenarios() {
  local dir="$MG_ROOT/conformance/scenarios" file rel id f out rc
  local files=()
  [ -d "$dir" ] || return 0
  while IFS= read -r file; do
    [ -n "$file" ] || continue
    rel="${file#"$dir"/}"
    id="${rel%.*}"
    if [ "$#" -eq 0 ]; then
      files[${#files[@]}]="$file"
      continue
    fi
    for f in "$@"; do
      f="${f#/}"
      f="${f%/}"
      f="${f%.yaml}"
      f="${f%.yml}"
      f="${f%.json}"
      # shellcheck disable=SC2254 # the filter is a pattern
      case "$id" in
        "$f" | "$f"/* | $f)
          files[${#files[@]}]="$file"
          break
          ;;
      esac
    done
  done <<EOF
$(find "$dir" -type f \( -name '*.yaml' -o -name '*.yml' -o -name '*.json' \) ! -name 'area.yaml' ! -name 'area.yml' | LC_ALL=C sort)
EOF
  [ "${#files[@]}" -gt 0 ] || return 0
  out="$(mg_conf__cli_native conformance to-json "${files[@]}")"
  rc=$?
  # to-json exits 1 when a file does not parse, and still answers for it: that
  # file is no actor scenario here, and the harness reports it invalid.
  if [ "$rc" -ge 2 ] || [ "$(printf '%s\n' "$out" | grep -c '^{')" -ne "${#files[@]}" ]; then
    mg_failure scenario-parse "meguri-cli could not read the selected scenarios (exit $rc), so which of them are actor scenarios is unknown; see log.txt" \
      "cd core && cargo run -p meguri-cli -- conformance to-json <scenario file>"
    mg_status error
    return 1
  fi
  printf '%s\n' "$out" | jq -r 'select(.value | type == "object" and has("actors")) | .path' |
    while IFS= read -r file; do
      rel="${file#"$dir"/}"
      printf '%s\n' "${rel%.*}"
    done
}

# mg_oracle__recorder <name>: starts a sync-recorder container with the run's
# label (its recording directory: $MEGURI_RECORD_DIR) and waits (up to 60 s)
# until the addon reports that it records.
mg_oracle__recorder() {
  local name="$1" state _
  MG_ORACLE_SYNC_CONTAINERS="$MG_ORACLE_SYNC_CONTAINERS $name"
  mg_compose --profile oracle-record run -d --no-deps --name "$name" --label "meguri.mg.run-id=$MG_RUN_ID" \
    sync-recorder >/dev/null </dev/null || return 1
  for _ in $(seq 120); do
    state="$(docker inspect --format '{{.State.Running}} {{.State.Health.Status}}' "$name" 2>/dev/null)"
    case "$state" in
      "true healthy") return 0 ;;
      "true "*) sleep 0.5 ;;
      *)
        echo "oracle: the recorder $name stopped (${state:-gone})" >&2
        return 1
        ;;
    esac
  done
  echo "oracle: the recorder $name did not become healthy" >&2
  return 1
}

# mg_oracle__sync_faults: starts the checkout's sync-faults service unless it
# runs, and sets MG_ORACLE_SYNC_FAULTS, which every oracle worker of the run
# gets.
mg_oracle__sync_faults() {
  [ -z "$MG_ORACLE_SYNC_FAULTS" ] || return 0
  echo "oracle: starting sync-faults (toxiproxy) for actor scenarios" >&2
  if ! mg_compose --profile oracle-sync up --detach --wait --wait-timeout 60 sync-faults >&2 </dev/null; then
    mg_failure sync-faults "the sync-faults service (toxiproxy) did not start or become healthy; see log.txt"
    mg_status error
    return 1
  fi
  MG_ORACLE_SYNC_FAULTS="sync-faults:8474"
}

# mg_oracle__sync_stop: removes this run's recorders; sync-faults stays up
# for other runs.
mg_oracle__sync_stop() {
  # shellcheck disable=SC2086 # one container name per word
  [ -z "${MG_ORACLE_SYNC_CONTAINERS// /}" ] || docker rm -f $MG_ORACLE_SYNC_CONTAINERS >/dev/null 2>&1
  MG_ORACLE_SYNC_CONTAINERS=""
  MG_ORACLE_SYNC_FAULTS=""
  MG_ORACLE_SYNC_RECORDER=""
  return 0
}

mg_oracle() {
  local sub="${1:-}"
  [ "$#" -gt 0 ] && shift
  trap 'mg_oracle__cleanup; exit 143' TERM
  case "$sub" in
    build) mg_oracle_build "$@" ;;
    hello) mg_oracle_hello "$@" ;;
    record | compare) mg_oracle_record_compare "$sub" "$@" ;;
    recordings) mg_oracle_recordings "$@" ;;
    check) mg_oracle_check "$@" ;;
    time) mg_oracle_time "$@" ;;
    bump) mg_oracle_bump "$@" ;;
    lock) mg_oracle_lock "$@" ;;
    test) mg_oracle_test "$@" ;;
    "") mg_oracle__usage "missing subcommand" ;;
    *) mg_oracle__usage "unknown subcommand '$sub'" ;;
  esac
}

mg_oracle__docker_ok() {
  if ! docker info >/dev/null 2>&1; then
    mg_failure docker-unreachable "Docker is not reachable; start Docker Desktop" "tools/mg doctor"
    mg_status error
    return 1
  fi
}

mg_oracle_build() {
  [ "$#" -eq 0 ] || { mg_oracle__usage "build takes no arguments"; return 0; }
  mg_oracle__docker_ok || return 0
  mg_oracle__tree && mg_oracle__build "$MG_ORACLE_DIR" || return 0
  mg_detail image "$(jq -nc --arg t "meguri-oracle:$MG_ORACLE_TAG" --arg i "$MG_ORACLE_ID" '{tag: $t, id: $i}')"
  mg_message "oracle: built meguri-oracle:$MG_ORACLE_TAG"
  mg_status pass
}

mg_oracle_hello() {
  local tz=UTC out="$MG_RUN_DIR/hello.json" platform=""
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --tz) tz="${2:-}"; shift 2 || { mg_oracle__usage "--tz needs a zone"; return 0; } ;;
      --platform) platform="${2:-}"; shift 2 || { mg_oracle__usage "--platform needs apple"; return 0; } ;;
      *) mg_oracle__usage "hello: unexpected argument '$1'"; return 0 ;;
    esac
  done
  case "$platform" in
    "") ;;
    apple) mg_oracle__native_hello "$tz"; return 0 ;;
    *) mg_oracle__usage "--platform takes apple"; return 0 ;;
  esac
  mg_oracle__docker_ok || return 0
  mg_oracle__tree && mg_oracle__build "$MG_ORACLE_DIR" || return 0
  if ! mg_oracle__hello "$MG_ORACLE_TAG" "$MG_ORACLE_DIR" "$tz" "$out"; then
    mg_failure hello "the worker did not answer hello: $(head -c 2000 "$out" 2>/dev/null)"
    mg_status fail
    return 0
  fi
  mg_artifact json "$out" "hello response"
  mg_detail image "$(jq -nc --arg t "meguri-oracle:$MG_ORACLE_TAG" --arg i "$MG_ORACLE_ID" '{tag: $t, id: $i}')"
  mg_detail hello "$(jq -c 'del(.ok)' "$out")"
  mg_message "oracle: $(jq -r '"anki \(.anki_version), tz \(.tz), lock \(.lock_sha256[0:12]), driver \(.driver_sha256[0:12])"' "$out")"
  mg_status pass
}

mg_oracle_record_compare() {
  local cmd="$1" jobs=12 report="$MG_RUN_DIR/report.json" rc platform=""
  shift
  local filters=() selected=()
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --jobs) jobs="${2:-}"; shift 2 || { mg_oracle__usage "--jobs needs a number"; return 0; } ;;
      --platform) platform="${2:-}"; shift 2 || { mg_oracle__usage "--platform needs apple"; return 0; } ;;
      --*) mg_oracle__usage "$cmd: unknown option '$1'"; return 0 ;;
      *) filters[${#filters[@]}]="--filter"; filters[${#filters[@]}]="$1"; selected[${#selected[@]}]="$1"; shift ;;
    esac
  done
  case "$jobs" in '' | *[!0-9]* | 0) mg_oracle__usage "--jobs needs a positive number"; return 0 ;; esac
  case "$platform" in "" | apple) ;; *) mg_oracle__usage "--platform takes apple"; return 0 ;; esac
  if [ ! -d "$MG_ROOT/conformance/scenarios" ]; then
    mg_failure no-scenarios "conformance/scenarios does not exist"
    mg_status fail
    return 0
  fi
  if [ "$platform" = apple ]; then
    mg_oracle__native_record_compare "$cmd" "$jobs" ${filters[0]+"${filters[@]}"}
    return 0
  fi
  mg_oracle__docker_ok || return 0
  local actors
  actors="$(mg_oracle__actor_scenarios ${selected[0]+"${selected[@]}"})" || return 0
  mg_oracle__tree && mg_oracle__build "$MG_ORACLE_DIR" || return 0
  if [ -n "$actors" ]; then
    mg_oracle__sync_faults || return 0
  fi
  mg_oracle__harness "$MG_ORACLE_TAG" "$MG_ORACLE_DIR" "$MG_ROOT/conformance/goldens" \
    "$cmd" --scenarios /in/scenarios --goldens /in/goldens --jobs "$jobs" --report /out/report.json \
    ${filters[0]+"${filters[@]}"}
  rc=$?
  mg_oracle__sync_stop
  mg_oracle__report "$cmd" "$rc" "$report"
}

# --- the macOS native lane: --platform apple (float-determinism.md F6, F7) ----
# The same driver natively on this Mac through oracle/macos/run-native.sh,
# from the venv it bootstraps under its own checkout's build/oracle-macos-venv/
# (covered by U2's deny rules). It copies nothing out of oracle/. Workers are
# accepted only when they answer macos-arm64, the pinned anki_version and the
# pin's macos_lock_sha256 (conformance/oracle.json; the harness checks every
# worker, hello here).

MG_ORACLE_NATIVE=""
MG_ORACLE_NATIVE_SCRATCH=""

# mg_oracle__native_host: usage error anywhere but an arm64 Mac.
mg_oracle__native_host() {
  if [ "$(uname -s)" != Darwin ] || [ "$(uname -m)" != arm64 ]; then
    mg_oracle__usage "--platform apple runs only on an arm64 Mac (the native oracle lane)"
    return 1
  fi
}

# mg_oracle__native_setup: the oracle tree (mg_oracle__tree), its launcher
# (MG_ORACLE_NATIVE) and a venv bootstrapped from its pins.
mg_oracle__native_setup() {
  mg_oracle__tree || return 1
  MG_ORACLE_NATIVE="$MG_ORACLE_DIR/macos/run-native.sh"
  if [ ! -x "$MG_ORACLE_NATIVE" ] || [ ! -f "$MG_ORACLE_DIR/$MG_ORACLE_MACOS_LOCK" ]; then
    mg_failure no-native-lane "$MG_ORACLE_DIR has no macos/run-native.sh or $MG_ORACLE_MACOS_LOCK"
    mg_status error
    return 1
  fi
  if ! "$MG_ORACLE_NATIVE" bootstrap </dev/null; then
    mg_failure native-bootstrap "oracle/macos/run-native.sh bootstrap failed; see log.txt"
    mg_status error
    return 1
  fi
}

# mg_oracle__native_refusal <hello file>: why the pin refuses that worker, or nothing.
mg_oracle__native_refusal() {
  jq -r --slurpfile pin "$MG_ROOT/conformance/oracle.json" '$pin[0] as $p | [
    (if .platform != "macos-arm64" then "platform \(.platform)" else empty end),
    (if .anki_version != $p.anki_version then "anki \(.anki_version), the pin is \($p.anki_version)" else empty end),
    (if ($p.macos_lock_sha256 // "") == "" then "conformance/oracle.json has no macos_lock_sha256"
     elif .lock_sha256 != $p.macos_lock_sha256 then "lock \(.lock_sha256[0:12]), the pin is \($p.macos_lock_sha256[0:12])"
     else empty end)] | join("; ")' "$1"
}

mg_oracle__native_hello() {
  local tz="$1" out="$MG_RUN_DIR/hello.json" why
  mg_oracle__native_host || return 0
  mg_oracle__native_setup || return 0
  if ! "$MG_ORACLE_NATIVE" hello --tz "$tz" >"$out" </dev/null || ! jq -e '.ok == true' "$out" >/dev/null 2>&1; then
    mg_failure hello "the native worker did not answer hello: $(head -c 2000 "$out" 2>/dev/null)"
    mg_status fail
    return 0
  fi
  mg_artifact json "$out" "hello response"
  mg_detail hello "$(jq -c 'del(.ok)' "$out")"
  why="$(mg_oracle__native_refusal "$out")"
  if [ -n "$why" ]; then
    mg_failure oracle-mismatch "the native worker is not the pinned oracle: $why" "tools/mg oracle hello --platform apple"
    mg_message "oracle (apple): refused, $why"
    mg_status fail
    return 0
  fi
  mg_message "oracle (apple): $(jq -r '"anki \(.anki_version), \(.platform), time control \(.time_control), tz \(.tz), lock \(.lock_sha256[0:12]), driver \(.driver_sha256[0:12])"' "$out")"
  if [ "$(jq -r .time_control "$out")" = faketime ]; then
    mg_status pass
  else
    mg_failure no-faketime "the native worker runs on the real clock (time_control none): record and compare run only clock-free scenarios" \
      "tools/mg oracle hello --platform apple"
    mg_status warn
  fi
}

# mg_oracle__native_record_compare <record|compare> <jobs> [--filter F]...
mg_oracle__native_record_compare() {
  local cmd="$1" jobs="$2" report="$MG_RUN_DIR/report.json" rc
  shift 2
  mg_oracle__native_host || return 0
  if [ ! -f "$MG_ROOT/conformance/oracle.json" ]; then
    mg_failure no-pin "conformance/oracle.json does not exist"
    mg_status error
    return 0
  fi
  mg_oracle__native_setup || return 0
  MG_ORACLE_NATIVE_SCRATCH="$MG_RUN_DIR/scratch"
  mkdir -p "$MG_ORACLE_NATIVE_SCRATCH"
  ORACLE_SCRATCH_ROOT="$MG_ORACLE_NATIVE_SCRATCH" ORACLE_FIXTURES="$MG_ROOT/conformance/fixtures" \
    "$MG_ORACLE_NATIVE" harness "$cmd-apple" \
    --scenarios "$MG_ROOT/conformance/scenarios" --goldens "$MG_ROOT/conformance/goldens" \
    --overlays "$MG_ROOT/conformance/goldens-apple" --pin "$MG_ROOT/conformance/oracle.json" \
    --jobs "$jobs" --report "$report" ${1+"$@"} </dev/null
  rc=$?
  rm -rf "$MG_ORACLE_NATIVE_SCRATCH"
  MG_ORACLE_NATIVE_SCRATCH=""
  mg_oracle__native_report "$cmd" "$rc" "$report"
}

# mg_oracle__native_report <record|compare> <harness rc> <report.json>
mg_oracle__native_report() {
  local cmd="$1" rc="$2" report="$3" ok total passed unsupported failed
  if [ ! -s "$report" ] || [ "$rc" -ge 2 ]; then
    mg_failure harness "the native harness exited with code $rc without a report; see log.txt"
    mg_status error
    return 0
  fi
  mg_artifact report "$report" "$cmd --platform apple report"
  ok='["recorded", "no_difference"]'
  [ "$cmd" = compare ] && ok='["pass"]'
  total="$(jq '.scenario_count' "$report")"
  passed="$(jq --argjson ok "$ok" '[.scenarios[] | select(.status | IN($ok[]))] | length' "$report")"
  unsupported="$(jq '[.scenarios[] | select(.status == "clock_unsupported")] | length' "$report")"
  failed=$((total - passed - unsupported))
  mg_count total "$total"
  mg_count passed "$passed"
  mg_count failed "$failed"
  mg_count skipped "$unsupported"
  mg_detail oracle "$(jq -c '.oracle' "$report")"
  mg_detail totals "$(jq -c '.totals' "$report")"
  mg_detail timing "$(jq -c '{jobs, wall_s, mean_worker_ms}' "$report")"
  [ "$cmd" = compare ] || mg_detail overlays "$(jq -c '.overlay_totals' "$report")"
  jq -c --argjson ok "$ok" --arg cmd "$cmd" '.scenarios[] | select((.status | IN($ok[])) | not)
    | select(.status != "clock_unsupported")
    | {id: .scenario,
       message: (.status
         + (if .error then ": " + (.error | tostring) else "" end)
         + (if .pointer then " at " + .pointer else "" end)
         + (if (.outside_class // []) != [] then ": \(.outside_class | length) entries outside the fp-libm-boundary class, first " + .outside_class[0].pointer + " (linux " + (.outside_class[0].linux | tostring) + ", apple " + (.outside_class[0].apple | tostring) + ")" else "" end)
         + (if .diffs then " at " + .diffs[0].pointer + " (expected " + (.diffs[0].expected | tostring) + ", got " + (.diffs[0].actual | tostring) + ")" else "" end)),
       repro: ("tools/mg oracle " + $cmd + " --platform apple " + (.scenario | @sh))}' "$report" >>"$MG_STATE/failures.jsonl"
  local what
  if [ "$cmd" = record ]; then
    what="$(jq -r '.overlay_totals | "\(.overlays_written) overlays written, \(.overlays_deleted) deleted; \(.entries) entries (\(.day_entries) days, \(.memory_entries) memory, \(.other_entries) other), \(.outside_class) outside the class"' "$report")"
  else
    what="$(jq -r '"\([.scenarios[].overlay_applied // 0] | add // 0) overlay entries applied"' "$report")"
  fi
  if [ "$total" -eq 0 ]; then
    mg_failure no-scenarios "no scenario matched"
    mg_message "oracle $cmd (apple): no scenario matched"
    mg_status fail
  elif [ "$failed" -gt 0 ]; then
    mg_message "oracle $cmd (apple): $passed/$total ok, $failed failed, $unsupported not run (clock_unsupported); $what"
    mg_status fail
  elif [ "$unsupported" -gt 0 ]; then
    mg_failure clock_unsupported "$unsupported scenarios were not run: the native worker has no faketime and they are not clock-free" \
      "tools/mg oracle hello --platform apple"
    mg_message "oracle $cmd (apple): $passed/$total ok, $unsupported not run (clock_unsupported); $what"
    mg_status warn
  else
    mg_message "oracle $cmd (apple): $passed/$total ok; $what"
    mg_status pass
  fi
}

# recordings <filter>... (sync-ops.md S9, R1933): the only writer of
# conformance/recordings/scenarios/<id>/. Each selected actor scenario runs
# again in the oracle harness, one at a time, with `compare` (so its
# observations must equal its fresh golden) and every oracle actor's sync
# traffic through a recorder of its own. Its exchanges.jsonl and
# recording.json replace the scenario's recording only when the run matched
# the golden and the recorder wrote every exchange.
mg_oracle_recordings() {
  local arg
  [ "$#" -gt 0 ] || { mg_oracle__usage "recordings needs at least one scenario filter"; return 0; }
  for arg in "$@"; do
    case "$arg" in -*) mg_oracle__usage "recordings: unknown option '$arg'"; return 0 ;; esac
  done
  local ids plan="$MG_RUN_DIR/plan.json"
  ids="$(mg_oracle__actor_scenarios "$@")" || return 0
  if [ -z "$ids" ]; then
    mg_failure no-scenarios "no actor scenario matches $*; only actor scenarios have oracle-client exchanges"
    mg_message "oracle recordings: no actor scenario matched"
    mg_status fail
    return 0
  fi
  mg_oracle__docker_ok || return 0
  # A recording names the scenario text and the golden it reproduces: fresh
  # goldens only (T0 lists an actor scenario with one as live or pending).
  # shellcheck disable=SC2086 # one id per word
  mg_conf__cli_native conformance run --conformance "$MG_ROOT/conformance" --lane plan --report "$plan" $ids >/dev/null 2>&1
  if [ ! -s "$plan" ]; then
    mg_failure plan "could not check the goldens of $(printf '%s' "$ids" | tr '\n' ' '); see log.txt"
    mg_status error
    return 0
  fi
  mg_oracle__tree && mg_oracle__build "$MG_ORACLE_DIR" || return 0
  mg_oracle__sync_faults || return 0
  local id n=0 recorded=0 status sha
  while IFS= read -r id; do
    n=$((n + 1))
    status="$(jq -r --arg id "$id" '.scenarios[] | select(.scenario == $id) | .status' "$plan")"
    sha="$(jq -r --arg id "$id" '.scenarios[] | select(.scenario == $id) | .scenario_sha256 // empty' "$plan")"
    case "$status" in
      live | pending) ;;
      *)
        mg_oracle__recording_failure "$id" "its golden is ${status:-unknown}: record a fresh golden first" \
          "tools/mg oracle record $id"
        continue
        ;;
    esac
    mg_oracle__record_one "$id" "$sha" "$n" && recorded=$((recorded + 1))
  done <<EOF
$ids
EOF
  mg_oracle__sync_stop
  mg_count total "$n"
  mg_count passed "$recorded"
  mg_count failed $((n - recorded))
  mg_detail image "$(jq -nc --arg t "meguri-oracle:$MG_ORACLE_TAG" --arg i "$MG_ORACLE_ID" '{tag: $t, id: $i}')"
  if [ "$recorded" -eq "$n" ]; then
    mg_message "oracle recordings: $recorded of $n recorded under conformance/recordings/scenarios/; review and commit them"
    mg_status pass
  else
    mg_message "oracle recordings: $recorded of $n recorded; the others kept no recording"
    mg_status fail
  fi
}

# mg_oracle__recording_failure <id> <message> [repro]
mg_oracle__recording_failure() {
  jq -nc --arg id "$1" --arg m "$2" --arg r "${3:-tools/mg oracle recordings $1}" \
    '{id: $id, message: $m, repro: $r}' >>"$MG_STATE/failures.jsonl"
}

# mg_oracle__record_one <id> <scenario sha256> <n>: one recording run.
mg_oracle__record_one() {
  local id="$1" sha="$2" out="$MG_RUN_DIR/recordings/$1" rec report health dest rc how
  rec="mg-sync-recorder-$(printf '%s' "$MG_RUN_ID" | tr 'A-Z' 'a-z')-$3"
  report="$out/compare-report.json"
  mkdir -p "$out/recorder"
  echo "oracle: recording $id through $rec" >&2
  if ! MEGURI_RECORD_DIR="$out/recorder" mg_oracle__recorder "$rec"; then
    docker rm -f "$rec" >/dev/null 2>&1
    mg_oracle__recording_failure "$id" "the recorder did not start or become healthy; see log.txt"
    return 1
  fi
  MG_ORACLE_SYNC_RECORDER="$rec:8080"
  mg_oracle__harness "$MG_ORACLE_TAG" "$MG_ORACLE_DIR" "$MG_ROOT/conformance/goldens" \
    compare --scenarios /in/scenarios --goldens /in/goldens --jobs 1 \
    --report "/out/recordings/$id/compare-report.json" --filter "$id"
  rc=$?
  MG_ORACLE_SYNC_RECORDER=""
  health="$(docker exec "$rec" cat /tmp/record-sync-health.json 2>/dev/null)"
  docker rm -f "$rec" >/dev/null 2>&1
  [ ! -s "$report" ] || mg_artifact report "$report" "recording run of $id"
  if [ ! -s "$report" ] || [ "$rc" -ge 2 ] ||
    [ "$(jq -r --arg id "$id" '[.scenarios[] | select(.scenario == $id) | .status] == ["pass"]' "$report")" != true ]; then
    how="harness exit $rc"
    [ ! -s "$report" ] || how="$how$(jq -r --arg id "$id" '.scenarios[] | select(.scenario == $id)
      | ", " + .status + (if .error then ": " + (.error | tostring) else "" end)' "$report")"
    mg_oracle__recording_failure "$id" "the recording run did not reproduce the golden ($how); nothing recorded"
    return 1
  fi
  if [ "$(printf '%s' "$health" | jq -r '.ok == true and .errors == 0 and .written > 0' 2>/dev/null)" != true ]; then
    mg_oracle__recording_failure "$id" "the recorder did not record every exchange (${health:-no health report}); nothing recorded"
    return 1
  fi
  if [ -z "$sha" ] ||
    [ "$(jq -r '.oracle | [.anki_version, .driver_sha256, .lock_sha256] | all(type == "string")' "$report")" != true ]; then
    mg_oracle__recording_failure "$id" "the run reported no oracle metadata or scenario hash; nothing recorded"
    return 1
  fi
  dest="$MG_ROOT/conformance/recordings/scenarios/$id"
  rm -rf "$dest"
  mkdir -p "$dest"
  cp "$out/recorder/exchanges.jsonl" "$dest/exchanges.jsonl"
  jq -nS --indent 2 --arg id "$id" --arg sha "$sha" --slurpfile r "$report" \
    '{oracle: ($r[0].oracle | {anki_version, driver_sha256, lock_sha256}), scenario_id: $id, scenario_sha256: $sha}' \
    >"$dest/recording.json"
  mg_artifact recording "$dest/exchanges.jsonl" "$id"
  return 0
}


# mg_oracle__report <record|compare> <harness rc> <report.json>
mg_oracle__report() {
  local cmd="$1" rc="$2" report="$3" ok
  if [ ! -s "$report" ] || [ "$rc" -ge 2 ]; then
    mg_failure harness "the harness exited with code $rc without a report; see log.txt"
    mg_status error
    return 0
  fi
  mg_artifact report "$report" "$cmd report"
  ok=recorded
  [ "$cmd" = compare ] && ok=pass
  local total passed
  total="$(jq '.scenario_count' "$report")"
  passed="$(jq --arg ok "$ok" '[.scenarios[] | select(.status == $ok)] | length' "$report")"
  mg_count total "$total"
  mg_count passed "$passed"
  mg_count failed $((total - passed))
  mg_detail image "$(jq -nc --arg t "meguri-oracle:$MG_ORACLE_TAG" --arg i "$MG_ORACLE_ID" '{tag: $t, id: $i}')"
  mg_detail oracle "$(jq -c '.oracle' "$report")"
  mg_detail totals "$(jq -c '.totals' "$report")"
  mg_detail timing "$(jq -c '{jobs, wall_s, mean_worker_ms}' "$report")"
  jq -c --arg ok "$ok" --arg cmd "$cmd" '.scenarios[] | select(.status != $ok)
    | {id: .scenario,
       message: (.status
         + (if .error then ": " + (.error | tostring) else "" end)
         + (if .diffs then " at " + .diffs[0].pointer + " (expected " + (.diffs[0].expected | tostring) + ", got " + (.diffs[0].actual | tostring) + ")" else "" end)),
       repro: ("tools/mg oracle " + $cmd + " " + (.scenario | @sh))}' "$report" >>"$MG_STATE/failures.jsonl"
  if [ "$total" -eq 0 ]; then
    mg_failure no-scenarios "no scenario matched"
    mg_message "oracle $cmd: no scenario matched"
    mg_status fail
  elif [ "$passed" -eq "$total" ]; then
    mg_message "oracle $cmd: $passed/$total $ok ($(jq -r '"mean \(.mean_worker_ms) ms per scenario in the worker"' "$report"))"
    mg_status pass
  else
    mg_message "oracle $cmd: $passed/$total $ok; $(jq -r '.totals | to_entries | map("\(.key) \(.value)") | join(", ")' "$report")"
    mg_status fail
  fi
}

# check <dir> [--now T] [--tz Z] [--deck D]... [--render-note N]... [--schedule]
# (specs/conformance/collection-check.md): one worker checks a copy of <dir>'s
# collection, the directory mounted read-only. The harness derives the clock
# and zone from the manifest when they are not given and checks the values;
# here only the shape of the arguments and the collection file are checked,
# before the image is built. The report is check.json (artifact `report`,
# repeated under details.check); status per K9, and a K7 kind as the failure
# of an `error`.
mg_oracle_check() {
  local dir="" abs rc
  local args=()
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --now | --tz | --deck | --render-note)
        [ "$#" -ge 2 ] || { mg_oracle__usage "check: $1 needs a value"; return 0; }
        # --opt=value, so a value starting with '-' stays a value.
        args[${#args[@]}]="$1=$2"
        shift 2
        ;;
      --now=* | --tz=* | --deck=* | --render-note=*) args[${#args[@]}]="$1"; shift ;;
      --schedule) args[${#args[@]}]="$1"; shift ;;
      -*) mg_oracle__usage "check: unknown option '$1'"; return 0 ;;
      *)
        [ -z "$dir" ] || { mg_oracle__usage "check takes one collection directory"; return 0; }
        dir="$1"
        shift
        ;;
    esac
  done
  [ -n "$dir" ] || { mg_oracle__usage "check needs a collection directory"; return 0; }
  if [ ! -f "$dir/collection.anki2" ]; then
    mg_failure no_collection "$dir/collection.anki2 does not exist" "tools/mg oracle check <dir with a collection.anki2>"
    mg_message "oracle check: no collection.anki2 in $dir"
    mg_status error
    return 0
  fi
  abs="$(cd "$dir" && pwd -P)"
  case "$abs" in *:*) mg_oracle__usage "check: $abs has a ':', which a bind mount cannot carry"; return 0 ;; esac
  mg_oracle__docker_ok || return 0
  mg_oracle__tree && mg_oracle__build "$MG_ORACLE_DIR" || return 0
  rm -f "$MG_RUN_DIR/check.json" "$MG_RUN_DIR/check-error.json"
  mg_oracle__run "$MG_ORACLE_TAG" "$MG_ORACLE_DIR" -u "$(id -u):$(id -g)" \
    -v "$abs:/in/collection:ro" -v "$MG_RUN_DIR:/out" \
    oracle harness check --dir /in/collection "--label=$dir" --out /out ${args[0]+"${args[@]}"} </dev/null
  rc=$?
  mg_detail image "$(jq -nc --arg t "meguri-oracle:$MG_ORACLE_TAG" --arg i "$MG_ORACLE_ID" '{tag: $t, id: $i}')"
  mg_oracle__check_result "$rc" "$dir"
}

# mg_oracle__check_result <harness rc> <dir as given>: K9.
mg_oracle__check_result() {
  local rc="$1" dir="$2" report="$MG_RUN_DIR/check.json" err="$MG_RUN_DIR/check-error.json" kind problems
  if [ "$rc" -eq 0 ] && [ -s "$report" ]; then
    mg_artifact report "$report" "collection check"
    mg_detail check "$(jq -c . "$report")"
    problems="$(jq -c --arg repro "$MG_DEFAULT_REPRO" '
      (if (.check_db | has("error")) then [{id: "corrupt", message: "Check Database found the file corrupt"}]
       else [.check_db | to_entries[] | select(.value > 0)
         | {id: .key, message: "Check Database reported \(.value) of \(.key)"}] end)
      + [.changed_tables[] | select(. != "col" and . != "config")
         | {id: ("changed_" + .), message: "Check Database changed the \(.) table"}]
      | .[] | . + {repro: $repro}' "$report")"
    if [ -n "$problems" ]; then
      printf '%s\n' "$problems" >>"$MG_STATE/failures.jsonl"
      mg_message "oracle check: $(printf '%s\n' "$problems" | grep -c .) problems in $dir; see check.json"
      mg_status fail
    else
      mg_message "oracle check: Check Database found nothing in $dir ($(jq -r '.counts | "\(.notes) notes, \(.cards) cards, \(.revlog) reviews"' "$report"))"
      mg_status pass
    fi
  elif [ -s "$err" ]; then
    kind="$(jq -r '.kind' "$err")"
    case "$kind" in usage | no_collection | open_failed | oracle_error) ;; *) kind=oracle_error ;; esac
    mg_failure "$kind" "$(jq -r '.detail' "$err")"
    mg_message "oracle check: $kind for $dir"
    mg_status error
  else
    mg_failure oracle_error "the check exited with code $rc without a report; see log.txt"
    mg_message "oracle check: no report for $dir"
    mg_status error
  fi
}

# time <dir> [--platform apple] [--op N]... [--now T] [--tz Z] [--repeat N]
# (specs/conformance/timed-operations.md): one timing worker times the T6
# operations on copies of <dir>'s collection. Docker lane: <dir> mounted
# read-only, the worker's scratch on the container's /scratch tmpfs.
# --platform apple: the native lane (bootstrapped first), its scratch under the
# run directory, or under build/private/ when <dir> is there (T2.1). The
# harness derives the clock and zone and checks --op and --repeat (T1); here
# only the shape of the arguments and the collection file are checked. The
# report is timing.json (artifact `report`, repeated under details.timing);
# status per T9, and a T8 kind as the failure of an `error`.
mg_oracle_time() {
  local dir="" abs platform="" rc scratch private
  local args=()
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --platform)
        [ "$#" -ge 2 ] || { mg_oracle__usage "time: --platform needs apple"; return 0; }
        platform="$2"
        shift 2
        ;;
      --now | --tz | --op | --repeat)
        [ "$#" -ge 2 ] || { mg_oracle__usage "time: $1 needs a value"; return 0; }
        # --opt=value, so a value starting with '-' stays a value.
        args[${#args[@]}]="$1=$2"
        shift 2
        ;;
      --now=* | --tz=* | --op=* | --repeat=*) args[${#args[@]}]="$1"; shift ;;
      -*) mg_oracle__usage "time: unknown option '$1'"; return 0 ;;
      *)
        [ -z "$dir" ] || { mg_oracle__usage "time takes one collection directory"; return 0; }
        dir="$1"
        shift
        ;;
    esac
  done
  [ -n "$dir" ] || { mg_oracle__usage "time needs a collection directory"; return 0; }
  case "$platform" in "" | apple) ;; *) mg_oracle__usage "--platform takes apple"; return 0 ;; esac
  if [ ! -f "$dir/collection.anki2" ]; then
    mg_failure no_collection "$dir/collection.anki2 does not exist" "tools/mg oracle time <dir with a collection.anki2>"
    mg_message "oracle time: no collection.anki2 in $dir"
    mg_status error
    return 0
  fi
  abs="$(cd "$dir" && pwd -P)"
  rm -f "$MG_RUN_DIR/timing.json" "$MG_RUN_DIR/timing-error.json"
  if [ "$platform" = apple ]; then
    mg_oracle__native_host || return 0
    mg_oracle__native_setup || return 0
    scratch="$MG_RUN_DIR/scratch"
    private="$(cd "$MG_ROOT/build/private" 2>/dev/null && pwd -P)"
    if [ -n "$private" ]; then
      case "$abs/" in "$private"/*) scratch="$private/oracle-time-$MG_RUN_ID" ;; esac
    fi
    MG_ORACLE_NATIVE_SCRATCH="$scratch"
    mkdir -p "$scratch"
    ORACLE_SCRATCH_ROOT="$scratch" "$MG_ORACLE_NATIVE" harness time --dir "$abs" --out "$MG_RUN_DIR" \
      ${args[0]+"${args[@]}"} </dev/null
    rc=$?
    rm -rf "$scratch"
    MG_ORACLE_NATIVE_SCRATCH=""
  else
    case "$abs" in *:*) mg_oracle__usage "time: $abs has a ':', which a bind mount cannot carry"; return 0 ;; esac
    mg_oracle__docker_ok || return 0
    mg_oracle__tree && mg_oracle__build "$MG_ORACLE_DIR" || return 0
    mg_oracle__run "$MG_ORACLE_TAG" "$MG_ORACLE_DIR" -u "$(id -u):$(id -g)" \
      -v "$abs:/in/collection:ro" -v "$MG_RUN_DIR:/out" \
      oracle harness time --dir /in/collection --out /out ${args[0]+"${args[@]}"} </dev/null
    rc=$?
    mg_detail image "$(jq -nc --arg t "meguri-oracle:$MG_ORACLE_TAG" --arg i "$MG_ORACLE_ID" '{tag: $t, id: $i}')"
  fi
  mg_oracle__time_result "$rc" "$dir"
}

# mg_oracle__time_result <harness rc> <dir as given>: T9.
mg_oracle__time_result() {
  local rc="$1" dir="$2" report="$MG_RUN_DIR/timing.json" err="$MG_RUN_DIR/timing-error.json" kind total bad
  if [ "$rc" -eq 0 ] && [ -s "$report" ]; then
    mg_artifact report "$report" "timed operations"
    mg_detail timing "$(jq -c . "$report")"
    total="$(jq '.operations | length' "$report")"
    bad="$(jq '[.operations[] | select(.status != "ok")] | length' "$report")"
    mg_count total "$total"
    mg_count passed "$((total - bad))"
    mg_count failed "$bad"
    jq -c --arg repro "$MG_DEFAULT_REPRO" '.operations[] | select(.status != "ok")
      | {id: .name,
         message: (.status + (if .error then " (" + .error.kind + " at " + .error.stage
           + (if .error.exception then ", " + .error.exception else "" end) + ")" else "" end)),
         repro: $repro}' "$report" >>"$MG_STATE/failures.jsonl"
    if [ "$bad" -gt 0 ]; then
      mg_message "oracle time: $bad of $total operations not ok on $(jq -r .platform "$report") in $dir; see timing.json"
      mg_status fail
    else
      mg_message "oracle time: $total operations ok on $(jq -r '"\(.platform), \(.protocol.repeat) timed runs each"' "$report") in $dir"
      mg_status pass
    fi
  elif [ -s "$err" ]; then
    kind="$(jq -r '.kind' "$err")"
    case "$kind" in usage | no_collection | open_failed | clock_unsupported | oracle_error) ;; *) kind=oracle_error ;; esac
    mg_failure "$kind" "$(jq -r '.detail' "$err")"
    mg_message "oracle time: $kind for $dir"
    mg_status error
  else
    mg_failure oracle_error "the timing run exited with code $rc without a report; see log.txt"
    mg_message "oracle time: no report for $dir"
    mg_status error
  fi
}

mg_oracle_lock() {
  local platform="" file=requirements.lock
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --platform) platform="${2:-}"; shift 2 || { mg_oracle__usage "--platform needs apple"; return 0; } ;;
      *) mg_oracle__usage "lock: unexpected argument '$1'"; return 0 ;;
    esac
  done
  case "$platform" in "") ;; apple) file="$MG_ORACLE_MACOS_LOCK" ;; *) mg_oracle__usage "--platform takes apple"; return 0 ;; esac
  mg_oracle__own_tree "lock rewrites oracle/$file" || return 0
  mg_oracle__docker_ok || return 0
  if ! mg_oracle__lock "$MG_ROOT/oracle" "$platform"; then
    mg_failure lock "uv could not resolve oracle/requirements.in for oracle/$file; see log.txt"
    mg_status fail
    return 0
  fi
  mg_artifact lock "$MG_ROOT/oracle/$file"
  if [ "$platform" = apple ]; then
    mg_message "oracle: oracle/$file regenerated (lock $(mg_oracle__sha256 <"$MG_ROOT/oracle/$file" | cut -c1-12)); conformance/oracle.json's macos_lock_sha256 moves only with an oracle bump"
  else
    mg_message "oracle: oracle/requirements.lock regenerated (lock $(mg_oracle__sha256 <"$MG_ROOT/oracle/requirements.lock" | cut -c1-12))"
  fi
  mg_status pass
}

# bump <version> (R1874): a staging copy of the image inputs with the new pin,
# a fresh lock, a new image, every golden re-recorded into the run directory and
# a behavior-diff report against the committed goldens. Nothing in the
# repository changes; adopting the bump is one separate change. It passes only
# when every scenario and every committed golden was re-recorded.
mg_oracle_bump() {
  local version="${1:-}" jobs=12
  [ "$#" -gt 0 ] && shift
  case "$version" in '' | *[!0-9.]* | .* | *.) mg_oracle__usage "bump needs a version like 26.10.0"; return 0 ;; esac
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --jobs) jobs="${2:-}"; shift 2 || { mg_oracle__usage "--jobs needs a number"; return 0; } ;;
      *) mg_oracle__usage "bump: unexpected argument '$1'"; return 0 ;;
    esac
  done
  mg_oracle__own_tree "bump copies oracle/ into a staging tree" || return 0
  mg_oracle__docker_ok || return 0
  MG_ORACLE_DIR="$MG_ROOT/oracle"
  MG_ORACLE_STAGED="$MG_RUN_DIR/staging/oracle"
  mg_oracle__bump "$version" "$jobs"
  mg_oracle__unstage "$MG_ORACLE_STAGED"
  MG_ORACLE_STAGED=""
}

mg_oracle__bump() {
  local version="$1" jobs="$2" staging="$MG_RUN_DIR/staging" old_tag
  mkdir -p "$staging/oracle"
  (cd "$MG_ORACLE_DIR" && cp Dockerfile Dockerfile.dockerignore entrypoint.sh requirements.in "$staging/oracle/" &&
    tar -cf - --exclude __pycache__ --exclude '*.pyc' driver | tar -xf - -C "$staging/oracle")
  sed "s/^anki==.*/anki==$version/" "$MG_ORACLE_DIR/requirements.in" >"$staging/oracle/requirements.in"
  echo "oracle: locking anki==$version" >&2
  if ! mg_oracle__lock "$staging/oracle"; then
    mg_failure lock "uv could not resolve anki==$version; see log.txt"
    mg_status fail
    return 0
  fi
  mg_artifact lock "$staging/oracle/requirements.lock" "staged lock for anki $version"
  # The native lane's lock moves with it (float-determinism.md F6.1).
  if ! mg_oracle__lock "$staging/oracle" apple || [ ! -s "$staging/oracle/$MG_ORACLE_MACOS_LOCK" ]; then
    mg_failure lock "uv could not resolve anki==$version for the macOS native lane; see log.txt"
    mg_status fail
    return 0
  fi
  mg_artifact lock "$staging/oracle/$MG_ORACLE_MACOS_LOCK" "staged macOS lock for anki $version"
  old_tag="$(mg_oracle__tag "$MG_ORACLE_DIR")"
  mg_oracle__build "$staging/oracle" || return 0
  local hello="$MG_RUN_DIR/hello-new.json"
  if ! mg_oracle__hello "$MG_ORACLE_TAG" "$staging/oracle" UTC "$hello"; then
    mg_failure hello "the new oracle did not answer hello: $(head -c 2000 "$hello" 2>/dev/null)"
    mg_status fail
    return 0
  fi
  # The conformance pin (conformance/oracle.json) moves with both lock files.
  local macos_lock
  macos_lock="$(mg_oracle__sha256 <"$staging/oracle/$MG_ORACLE_MACOS_LOCK")"
  mkdir -p "$staging/conformance"
  if [ -f "$MG_ROOT/conformance/oracle.json" ]; then
    jq --slurpfile h "$hello" --arg m "$macos_lock" \
      '.anki_version = $h[0].anki_version | .lock_sha256 = $h[0].lock_sha256 | .macos_lock_sha256 = $m' \
      "$MG_ROOT/conformance/oracle.json" >"$staging/conformance/oracle.json"
  else
    jq --arg m "$macos_lock" '{anki_version, lock_sha256, macos_lock_sha256: $m}' "$hello" >"$staging/conformance/oracle.json"
  fi
  mg_artifact pin "$staging/conformance/oracle.json" "staged conformance pin"
  # Re-recording actor scenarios needs the fault proxy (sync-ops.md S8.2).
  local actors
  actors="$(mg_oracle__actor_scenarios)" || return 0
  if [ -n "$actors" ]; then
    mg_oracle__sync_faults || return 0
  fi
  mg_oracle__harness "$MG_ORACLE_TAG" "$staging/oracle" "$staging/goldens" \
    record --scenarios /in/scenarios --goldens /in/goldens --jobs "$jobs" --report /out/record-report.json
  local rc=$?
  mg_oracle__sync_stop
  if [ ! -s "$MG_RUN_DIR/record-report.json" ] || [ "$rc" -ge 2 ]; then
    mg_failure harness "re-recording on anki $version exited with code $rc without a report; see log.txt"
    mg_status error
    return 0
  fi
  mg_oracle__run "$MG_ORACLE_TAG" "$staging/oracle" -u "$(id -u):$(id -g)" \
    -v "$MG_ROOT/conformance/goldens:/in/old:ro" -v "$staging/goldens:/in/new:ro" -v "$MG_RUN_DIR:/out" \
    oracle harness diff-report --old /in/old --new /in/new \
    --report /out/bump-report.json --markdown /out/bump-report.md </dev/null
  if [ ! -s "$MG_RUN_DIR/bump-report.json" ]; then
    mg_failure diff-report "the behavior-diff report was not written; see log.txt"
    mg_status error
    return 0
  fi
  mg_artifact report "$MG_RUN_DIR/bump-report.md" "behavior differences (markdown)"
  mg_artifact report "$MG_RUN_DIR/bump-report.json" "behavior differences"
  mg_artifact report "$MG_RUN_DIR/record-report.json" "re-recording report"
  mg_artifact goldens "$staging/goldens" "staged goldens"
  mg_detail bump "$(jq -nc --arg from "$old_tag" --arg to "$MG_ORACLE_TAG" --slurpfile h "$hello" \
    --slurpfile r "$MG_RUN_DIR/bump-report.json" --slurpfile rec "$MG_RUN_DIR/record-report.json" \
    '{from: $from, to: $to, hello: ($h[0] | del(.ok)), goldens: $r[0].totals,
      changed_observations: $r[0].changed_observations, recording: $rec[0].totals}')"
  local failed changed orphans total
  failed="$(jq '[.scenarios[] | select(.status != "recorded")] | length' "$MG_RUN_DIR/record-report.json")"
  changed="$(jq '.totals.changed // 0' "$MG_RUN_DIR/bump-report.json")"
  jq -c '.scenarios[] | select(.status != "recorded")
    | {id: .scenario, message: ("not recorded on the new oracle: " + (.error | tostring)),
       repro: ("tools/mg oracle record " + (.scenario | @sh))}' "$MG_RUN_DIR/record-report.json" >>"$MG_STATE/failures.jsonl"
  # A committed golden the new oracle did not re-record although its scenario
  # did not fail (that one is reported above): it has no scenario left.
  orphans="$(jq -c --slurpfile rec "$MG_RUN_DIR/record-report.json" --arg repro "$MG_DEFAULT_REPRO" '
    ($rec[0].scenarios | map(.scenario + ".json")) as $ran
    | .scenarios[] | select(.status == "not_recorded" and (.golden | IN($ran[]) | not))
    | {id: (.golden | rtrimstr(".json")), repro: $repro,
       message: ("golden " + .golden + " has no scenario, so the new oracle did not re-record it")}' \
    "$MG_RUN_DIR/bump-report.json")"
  [ -z "$orphans" ] || printf '%s\n' "$orphans" >>"$MG_STATE/failures.jsonl"
  orphans="$(printf '%s' "$orphans" | grep -c . || true)"
  total="$(jq '.scenario_count' "$MG_RUN_DIR/record-report.json")"
  mg_count total "$total"
  mg_count passed $((total - failed))
  mg_count failed "$failed"
  mg_count orphaned "$orphans"
  mg_count changed "$changed"
  if [ "$total" -eq 0 ]; then
    mg_failure no-scenarios "no scenario was recorded on anki $version, so nothing shows how it behaves"
    mg_message "oracle bump to $version: no scenario was recorded"
    mg_status fail
  elif [ "$failed" -gt 0 ] || [ "$orphans" -gt 0 ]; then
    mg_message "oracle bump to $version: $failed of $total scenarios could not be recorded, $orphans goldens have no scenario, $changed goldens changed; see bump-report.md"
    mg_status fail
  else
    mg_message "oracle bump to $version: $changed goldens changed; review bump-report.md, then adopt staging/oracle/requirements.in, requirements.lock, $MG_ORACLE_MACOS_LOCK, staging/conformance/oracle.json and staging/goldens in one change, with tools/mg harvest-inventory at the new tag (R1882; tools/mg harvest-report fails until then) and the overlays re-recorded on the new lock (tools/mg oracle record --platform apple)"
    mg_status pass
  fi
}

# test [pattern]: the driver self-tests inside the container (on the internal
# network), then the host checks in tools/tests/oracle_host_checks.sh. One TAP
# stream, numbered across both.
mg_oracle_test() {
  mg_oracle__own_tree "test copies oracle/ into its host checks" || return 0
  mg_oracle__docker_ok || return 0
  mg_oracle__tree && mg_oracle__build "$MG_ORACLE_DIR" || return 0
  local inner="$MG_RUN_DIR/driver.tap" outer="$MG_RUN_DIR/host.tap" tap="$MG_RUN_DIR/oracle-test.tap" rc1 rc2
  # The driver's sync tests create fault proxies (sync-ops.md S8.2).
  mg_oracle__sync_faults || return 0
  mg_oracle__run "$MG_ORACLE_TAG" "$MG_ORACLE_DIR" -e "ORACLE_SYNC_FAULTS=$MG_ORACLE_SYNC_FAULTS" \
    oracle selftest ${1+"$@"} </dev/null | tee "$inner"
  rc1=${PIPESTATUS[0]}
  mg_oracle__sync_stop
  if [ "$#" -eq 0 ]; then
    MG_ORACLE_ID="$MG_ORACLE_ID" MG_ORACLE_TAG="$MG_ORACLE_TAG" MG_ORACLE_DIR="$MG_ORACLE_DIR" \
      MG_COMPOSE_PROJECT="$(mg_compose_project)" /bin/bash "$MG_ROOT/tools/tests/oracle_host_checks.sh" | tee "$outer"
    rc2=${PIPESTATUS[0]}
  else
    : >"$outer"
    rc2=0
  fi
  awk '
    /^1\.\.[0-9]+$/ { plan += substr($0, 4); next }
    /^(not )?ok [0-9]+/ { n++; sub(/ok [0-9]+/, "ok " n); lines[++m] = $0; next }
    { lines[++m] = $0 }
    END { print "1.." plan; for (i = 1; i <= m; i++) print lines[i] }' "$inner" "$outer" >"$tap"
  mg_artifact tap "$tap"
  mg_tap_report "$tap" $((rc1 > rc2 ? rc1 : rc2)) ""
}
