# shellcheck shell=bash
# The host lease's mutex (U8). Sourced by tools/mg.d/host.sh and
# tools/scripts/sim-record.sh; runs under macOS /bin/bash 3.2 and in the dev
# container.
#
# Taking a free lease is one mkdir. Every other change to the lease happens
# under this mutex, so that none of them interleaves with another: a nested
# slot's mkdir and holder.json, a recording's release or hand-over to its
# nested job, a slot's release, a hand-over to a recording worker and the
# removal of a stale slot. It is a flock(2) lock on <lease dir>.mutex, held
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
