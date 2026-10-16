#!/usr/bin/env bash
# Makes conformance/fixtures/preservation/kitchen-sink.anki2
# (specs/conformance/normalized-dump.md N9) from the recipe scenario
# preservation/recipe/kitchen-sink, run on the oracle image.
# AGPL-3.0-or-later (oracle/LICENSE). Oracle maintainers only; run on the host
# from a full checkout:
#
#   oracle/fixtures/preservation/make.sh [--tag <meguri-oracle image tag>]
#
# Without --tag it builds the oracle image with `tools/mg oracle build`. The
# fixture is written only when every N9.2 check passes: schema 18 in WAL mode
# with nothing beside it, SQLite's integrity check, no piece of a stock note
# type left in the bytes, `tools/mg oracle check` finds no Check Database
# problem (its only change outside col and config is the tags' usn, README.md),
# and a fresh worker opening the file observes exactly the recipe golden's
# last step.
# Remaking changes the guids: re-record every preservation golden after it.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../../.." && pwd)"
dest="$root/conformance/fixtures/preservation"
recipe=preservation/recipe/kitchen-sink
# The instant scenarios that open the fixture start at (N9.3), and in epoch
# seconds.
now=2030-01-15T10:00:00Z
now_epoch=1894701600
tag=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    --tag) tag="${2:?--tag needs a tag}"; shift 2 ;;
    *) echo "usage: $0 [--tag <meguri-oracle tag>]" >&2; exit 64 ;;
  esac
done
if [ -z "$tag" ]; then
  result="$("$root/tools/mg" oracle build | tail -n 1)"
  tag="$(jq -r '.details.image.tag' "$result")"
  tag="${tag#meguri-oracle:}"
fi
docker image inspect "meguri-oracle:$tag" >/dev/null

work="$(mktemp -d "${TMPDIR:-/tmp}/meguri-preservation.XXXXXX")"
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/out" "$work/fixtures/preservation" "$work/check"
user="$(id -u):$(id -g)"

# Each command is its own container: the open check's worker never shares a
# process with the maker's secure_delete hook.
current() {
  docker run --rm --network none -u "$user" -e HOME=/tmp -e TMPDIR=/scratch \
    --tmpfs /scratch:exec,mode=1777 --entrypoint /maker/preservation/run.sh \
    -v "$here/..:/maker:ro" \
    -v "$root/conformance/scenarios:/in/scenarios:ro" \
    -v "$root/conformance/goldens:/in/goldens:ro" \
    -v "$work:/out" "meguri-oracle:$tag" "$@"
}

current recipe "/in/scenarios/$recipe.yaml" /out/out/kitchen-sink.anki2
current inspect /out/out/kitchen-sink.anki2 >"$work/summary.json"
current stock-check /out/out/kitchen-sink.anki2 "/in/scenarios/$recipe.yaml" >/dev/null
cp "$work/out/kitchen-sink.anki2" "$work/fixtures/preservation/kitchen-sink.anki2"
current open-check /out/fixtures /in/scenarios/preservation/fixture/open.yaml \
  "/in/scenarios/$recipe.yaml" "/in/goldens/$recipe.json"

# Check Database on a copy, at the fixture's first scenario day: no K5
# problem. It rebuilds the tags table at usn -1, so the synced fixture's tags
# (usn 9) show in changed_tables and K9 says fail; rebuild-check shows that
# this is the only change outside col and config (README.md).
current rebuild-check /out/out/kitchen-sink.anki2 "$now_epoch" >"$work/rebuild.json"
cp "$work/out/kitchen-sink.anki2" "$work/check/collection.anki2"
# tools/mg exits non-zero on K9's fail; the report decides here.
result="$("$root/tools/mg" oracle check "$work/check" --now "$now" --tz UTC | tail -n 1)" || true
report="$(jq -c '.details.check // empty' "$result")"
if [ -z "$report" ] || ! jq -e '(.check_db | has("error") | not) and all(.check_db[]; . == 0)
    and ([.changed_tables[] | select(. != "col" and . != "config")] - ["tags"] == [])' <<<"$report" >/dev/null; then
  echo "make.sh: tools/mg oracle check reports a problem; see $result" >&2
  exit 1
fi
[ "$(ls -A "$work/check")" = collection.anki2 ] || { echo "make.sh: the check left files beside the copy" >&2; exit 1; }

mkdir -p "$dest"
cp "$work/out/kitchen-sink.anki2" "$dest/kitchen-sink.anki2"
jq -c '{file, bytes, ver, journal_mode, integrity_check, freelist_count, counts, pending}' "$work/summary.json"
jq -c '{check_db: .check_db, changed_tables: .changed_tables}' <<<"$report"
jq -c '{differences}' "$work/rebuild.json"
echo "make.sh: wrote conformance/fixtures/preservation/kitchen-sink.anki2" >&2
