#!/usr/bin/env bash
# The process check of a fake test VM (mvl_procs; tests/mac_vm.bats,
# sim_vm.bats, perf.bats), linked as sysctl and ps into $FAKE_VM_BIN, which
# comes first on the fake VM's PATH (mac_vm_fake_ssh.sh). `sysctl -n
# kern.maxprocperuid kern.maxproc` gives FAKE_VM_MAXPROCPERUID (2900) and
# FAKE_VM_MAXPROC (4000); `ps -A -o uid=` lists FAKE_VM_PROCS processes of
# this user (a comma-separated list, one value per check, the last one
# repeating; 100) and 50 of another user, and appends the check's time to
# $FAKE_VM/procs.times. Any other call runs the real command.
set -u
case "$(basename "$0") $*" in
  "sysctl -n kern.maxprocperuid kern.maxproc")
    echo "${FAKE_VM_MAXPROCPERUID:-2900}"
    echo "${FAKE_VM_MAXPROC:-4000}"
    ;;
  "ps -A -o uid=")
    perl -MTime::HiRes=time -e 'printf "%.3f\n", time' >>"$FAKE_VM/procs.times"
    n=$(($(wc -l <"$FAKE_VM/procs.times")))
    IFS=, read -r -a counts <<<"${FAKE_VM_PROCS:-100}"
    [ "$n" -le "${#counts[@]}" ] || n="${#counts[@]}"
    awk -v n="${counts[$((n - 1))]}" -v u="$UID" 'BEGIN { for (i = 0; i < n; i++) print "  " u; for (i = 0; i < 50; i++) print "  " (u + 1) }'
    ;;
  *) PATH="${PATH#"$FAKE_VM_BIN":}" exec "$(basename "$0")" "$@" ;;
esac
