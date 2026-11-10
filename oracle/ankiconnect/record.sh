#!/bin/sh
# Record AnkiConnect transcript goldens by hand (AGPL-3.0-or-later, see LICENSE).
# Not a tools/mg job; README.md says what one would need.
#
#   oracle/ankiconnect/record.sh <image> <scenario.yaml>...
#
# Each scenario runs in its own fresh container (servers/transcripts T16) on a
# new --internal network, with no published port; conformance/fixtures is
# mounted read-only. The scenario's id is its path under conformance/recordings/
# without the suffix (servers/ankiconnect/...), and the golden is written next
# to it as <name>.json. Only the recorder's own JSON lines reach stderr.
set -eu

[ "$#" -ge 2 ] || { echo "usage: record.sh <image> <scenario.yaml>..." >&2; exit 64; }
image="$1"; shift
root="$(cd "$(dirname "$0")/../.." && pwd)"
tag="ac-rec-$$-$(od -An -N4 -tx4 /dev/urandom | tr -d ' ')"
net="$tag-net"
cleanup() {
  docker rm -f "$tag" >/dev/null 2>&1 || true
  docker network rm "$net" >/dev/null 2>&1 || true
}
trap cleanup EXIT INT TERM
docker network create --internal "$net" >/dev/null
status=0
for file in "$@"; do
  abs="$(cd "$(dirname "$file")" && pwd)/$(basename "$file")"
  case "$abs" in
    "$root"/conformance/recordings/servers/ankiconnect/*.yaml) ;;
    "$root"/conformance/recordings/servers/ankiconnect/*.json) ;;
    *) echo "record.sh: $file is not under conformance/recordings/servers/ankiconnect/" >&2; status=1; continue ;;
  esac
  rel="${abs#"$root"/conformance/recordings/}"
  id="${rel%.*}"
  format=yaml; case "$abs" in *.json) format=json ;; esac
  out="${abs%.*}.json"
  [ "$format" = yaml ] || { echo "record.sh: $file: JSON inputs would share the golden's path" >&2; status=1; continue; }
  tmp="$out.tmp.$$"
  if docker run --rm -i --init --name "$tag" --network "$net" \
      --tmpfs /scratch:mode=1777,exec \
      -v "$root/conformance/fixtures:/fixtures:ro" -e ORACLE_FIXTURES=/fixtures \
      -e ORACLE_ANKICONNECT_IMAGE="$image" \
      "$image" record --id "$id" --format "$format" <"$abs" >"$tmp"; then
    mv "$tmp" "$out"
    echo "recorded $id" >&2
  else
    rm -f "$tmp"
    echo "record.sh: $id was not recorded" >&2
    status=1
  fi
done
exit "$status"
