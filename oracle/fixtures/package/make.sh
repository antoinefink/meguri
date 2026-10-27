#!/usr/bin/env bash
# Makes conformance/fixtures/package/ (specs/conformance/package-ops.md E4)
# from conformance/fixtures/storage/, Anki 26.9.3 in the oracle image and
# Anki 2.1.26 in the storage maker's legacy image (both built here, with
# --apkg-only too).
# AGPL-3.0-or-later (oracle/LICENSE). Oracle maintainers only; run on the host
# from a full checkout:
#
#   oracle/fixtures/package/make.sh [--tag <meguri-oracle image tag>] [--apkg-only]
#
# Without --tag it builds the oracle image with `tools/mg oracle build`.
# --apkg-only writes only apkg/ and leaves the committed collection packages
# as they are (the stock check still imports the committed backup). The
# old-client packages (oldclient.py) come from Anki 2.1.26 in the storage
# maker's legacy image (linux/amd64, wheels pinned by hash). The
# packages (the collection packages and apkg.py's deck packages) are written
# only when every check passes: Anki's backup has E4's
# layout, the damaged page is what E4 says, and no SQLite file in any package
# (raw, or in one of the maker's own frames) nor the collection Anki imports
# from the backup fixture holds stock note type text (the storage maker's
# stock-check, R1959).
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../../.." && pwd)"
dest="$root/conformance/fixtures/package"
tag=""
apkg_only=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    --tag) tag="${2:?--tag needs a tag}"; shift 2 ;;
    --apkg-only) apkg_only=1; shift ;;
    *) echo "usage: $0 [--tag <meguri-oracle tag>] [--apkg-only]" >&2; exit 64 ;;
  esac
done
if [ -z "$tag" ]; then
  result="$("$root/tools/mg" oracle build | tail -n 1)"
  tag="$(jq -r '.details.image.tag' "$result")"
  tag="${tag#meguri-oracle:}"
fi
docker image inspect "meguri-oracle:$tag" >/dev/null

work="$(mktemp -d "${TMPDIR:-/tmp}/meguri-package-fixtures.XXXXXX")"
trap 'rm -rf "$work"' EXIT
user="$(id -u):$(id -g)"

# $1: the maker's directory (this one, or the storage maker's for stock-check).
current() {
  local maker="$1"
  shift
  docker run --rm --network none -u "$user" -e HOME=/tmp --entrypoint /maker/run.sh \
    -v "$maker:/maker:ro" -v "$root/conformance/fixtures/storage:/storage:ro" -v "$work:/out" \
    "meguri-oracle:$tag" "$@"
}

if [ -z "$apkg_only" ]; then
  current "$here" backup /storage/schema-18.anki2 /out/anki-backup.colpkg
  current "$here" build /storage /out/anki-backup.colpkg /out/package
else
  mkdir -p "$work/package/backup"
  cp "$dest/backup/standard.colpkg" "$work/package/backup/"
fi
current "$here" apkg /storage /out/package/apkg
docker build --platform linux/amd64 -q -f "$here/../storage/Dockerfile.legacy" --build-arg RELEASE=2.1.26 \
  -t meguri-fixtures-legacy:2.1.26 "$here/../storage" >/dev/null
docker run --rm --platform linux/amd64 --network none -u "$user" -e HOME=/tmp \
  -v "$here:/maker:ro" -v "$here/../storage:/storage-maker:ro" -v "$work/package/apkg:/out" \
  meguri-fixtures-legacy:2.1.26 /maker/oldclient-run.sh /out
current "$here" sqlite-files /out/package /out/sqlite
files=()
for f in "$work"/sqlite/*.anki2; do files+=("/out/sqlite/$(basename "$f")"); done
current "$here/../storage" stock-check "${files[@]}" >/dev/null

if [ -z "$apkg_only" ]; then
  for sub in backup variants; do
    mkdir -p "$dest/$sub"
    rm -f "$dest/$sub"/*.colpkg
    cp "$work/package/$sub"/*.colpkg "$dest/$sub/"
  done
fi
mkdir -p "$dest/apkg"
rm -f "$dest/apkg"/*.apkg
cp "$work/package/apkg"/*.apkg "$dest/apkg/"
echo "make.sh: wrote $(find "$dest" -name '*.colpkg' -o -name '*.apkg' | wc -l | tr -d ' ') packages to conformance/fixtures/package" >&2
