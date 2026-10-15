#!/usr/bin/env bash
# Makes conformance/fixtures/storage/*.anki2 (specs/storage/schema-upgrade.md U16)
# from the Anki releases pinned in locks/ and the oracle image (26.09.3).
# AGPL-3.0-or-later (oracle/LICENSE). Oracle maintainers only; run on the host
# from a full checkout (it needs Docker and amd64 emulation for the old wheels):
#
#   oracle/fixtures/storage/make.sh [--tag <meguri-oracle image tag>]
#
# Without --tag it builds the oracle image with `tools/mg oracle build`. The
# fixtures are written only when every check passes: each release wrote the
# schema version its recipe needs, no file holds a piece of a stock note
# type's CSS or templates, and Anki 26.09.3 upgrades every converted object
# with at most one unknown key (README.md).
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../../.." && pwd)"
dest="$root/conformance/fixtures/storage"
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

work="$(mktemp -d "${TMPDIR:-/tmp}/meguri-fixtures.XXXXXX")"
trap 'rm -rf "$work"' EXIT
user="$(id -u):$(id -g)"

legacy() {
  local release="$1"
  shift
  docker run --rm --platform linux/amd64 --network none -u "$user" -e HOME=/tmp \
    -v "$here:/maker:ro" -v "$work:/out" "meguri-fixtures-legacy:$release" /maker/run.sh "$@"
}
current() {
  docker run --rm --network none -u "$user" -e HOME=/tmp --entrypoint /maker/run.sh \
    -v "$here:/maker:ro" -v "$work:/out" "meguri-oracle:$tag" "$@"
}

for release in 2.1.26 2.1.33 2.1.40 2.1.44; do
  docker build --platform linux/amd64 -q -f "$here/Dockerfile.legacy" --build-arg "RELEASE=$release" \
    -t "meguri-fixtures-legacy:$release" "$here" >/dev/null
done

# 2.1.26 notetypes carry two legacy keys (tags, vers): drop one (README.md).
legacy 2.1.26 standard /out/schema-14.anki2 --expect-ver 14 --drop-keys '{"notetype": ["tags"]}'
legacy 2.1.33 standard /out/schema-15.anki2 --expect-ver 15 --ease-cases
legacy 2.1.40 standard /out/schema-16.anki2 --expect-ver 16
legacy 2.1.44 standard /out/schema-17.anki2 --expect-ver 17
current standard /out/schema-18.anki2 --expect-ver 18
current standard /out/schema-11.anki2 --expect-ver 11 --downgrade
current names /out/schema-11.anki2 /out/schema-11-names.anki2
for v in 10 12 13; do
  current version /out/schema-11.anki2 "/out/ver-$v-delete.anki2" "$v"
done
current version /out/schema-18.anki2 /out/ver-19-delete.anki2 19 --journal delete
for v in 10 12 13 19; do
  current journal "/out/ver-$v-delete.anki2" "/out/ver-$v-wal.anki2" wal
done
current v1-scheduler /out/schema-18.anki2 /out/v1-scheduler-18.anki2
current v1-scheduler /out/schema-11.anki2 /out/v1-scheduler-11.anki2

current upgrade-check /out/schema-11.anki2 /out/schema-14.anki2 /out/schema-11-names.anki2 >"$work/upgrade-check.jsonl"
if jq -e 'select(.problems != [])' "$work/upgrade-check.jsonl" >/dev/null; then
  echo "make.sh: an upgraded object keeps more than one unknown key:" >&2
  jq -c 'select(.problems != []) | {file, problems}' "$work/upgrade-check.jsonl" >&2
  exit 1
fi
files=()
for f in "$work"/*.anki2; do files+=("/out/$(basename "$f")"); done
# Each release checked its own file; this checks every finished file, the
# derived ones included, against 26.09.3's stock note types (U16).
current stock-check "${files[@]}" >/dev/null
current summary "${files[@]}" >"$work/summary.jsonl"
jq -e -s 'all(.[]; .sidecars == [])' "$work/summary.jsonl" >/dev/null

mkdir -p "$dest"
cp "$work"/*.anki2 "$dest/"
jq -c '{file, ver, wal_header}' "$work/summary.jsonl"
echo "make.sh: wrote ${#files[@]} fixtures to conformance/fixtures/storage" >&2
