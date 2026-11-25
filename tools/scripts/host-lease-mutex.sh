# shellcheck shell=bash
# The host lease's mutex (U8). Sourced by tools/mg.d/host.sh and
# tools/scripts/sim-record.sh; runs under macOS /bin/bash 3.2 and in the dev
# container.
#
# Taking a free lease is one mkdir. Every other change to the lease happens
# under this mutex, so that none of them interleaves with another: a nested
# slot's mkdir and holder.json, a recording's release or hand-over to its
# nested job, a slot's release, a hand-over to a recording worker and the
# removal of a stale slot. Whatever ends the lease (its holder's release, the
# cleanup of a dead holder) goes through host_lease_drop, which hands it to a
# live nested job. The mutex is a flock(2) lock on <lease dir>.mutex, held
# through fd 6 of the calling shell (perl takes it on that open file and
# exits; the lock stays with the open file until the shell closes it). The
# kernel drops it when the holder dies, so it never goes stale. Hold it only
# around file operations: a process started meanwhile inherits fd 6 and keeps
# the lock while it lives.

# host_lease_lock <lease dir>: takes the mutex, waiting at most 30 s.
host_lease_lock() {
  exec 6>>"$1.mutex" || return 1
  if ! perl -MFcntl=:flock -e '
      open(my $f, ">&=", 6) or die "fd 6: $!\n";
      $SIG{ALRM} = sub { die "timed out after 30 s\n" };
      alarm 30;
      flock($f, LOCK_EX) or die "flock: $!\n";'; then
    exec 6>&-
    echo "host lease: could not lock $1.mutex" >&2
    return 1
  fi
}

host_lease_unlock() { exec 6>&-; }

# host_lease_drop <lease dir>: with the mutex held, ends the lease (not the
# nested slot). A live holder of the nested slot inherits it, without its
# `nested` mark; otherwise the lease is removed. Removing it with a live
# nested job would let another checkout take the host while that job runs.
host_lease_drop() {
  local inner
  [ ! -e "$1/vm-xcode-pending" ] || return 1
  [ ! -e "$1/sim-posture-pending" ] || return 1
  [ ! -e "$1/vm-shutdown-pending" ] || return 1
  inner="$(jq -r '.pid // empty' "$1/inner/holder.json" 2>/dev/null)"
  if [ -n "$inner" ] && kill -0 "$inner" 2>/dev/null &&
    jq 'del(.nested)' "$1/inner/holder.json" >"$1/holder.json.tmp" &&
    mv -f "$1/holder.json.tmp" "$1/holder.json"; then
    rm -rf "$1/inner"
    return 0
  fi
  rm -rf "$1"
}
