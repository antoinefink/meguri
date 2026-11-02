#!/bin/bash
# v.sh <vncdo args...>: runs vncdo against the Recovery VNC server from tart's log.
S=/private/tmp/claude-501/-Users-antoine--herdr-worktrees-meguri-design/bcc4eb73-f59e-4573-8583-70b404dcd81d/scratchpad
U="$(grep -o 'vnc://[^ ]*' "$S/rec/tart.log" | tail -1)"
P="${U#vnc://:}"; PASS="${P%@*}"; HP="${P#*@}"; PORT="${HP##*:}"
exec "$S/vncenv/bin/vncdo" --force-caps -s "127.0.0.1::$PORT" -p "$PASS" -t 30 "$@"
