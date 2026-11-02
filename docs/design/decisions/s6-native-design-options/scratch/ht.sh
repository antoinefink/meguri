ht () {
	printf 'move %s %s\n' $(( $1 + 1300 )) $(( $2 + 340 )) > $F
	sleep 0.4
	printf 'click\n' > $F
	sleep ${3:-2}
}
F=/private/tmp/claude-501/-Users-antoine--herdr-worktrees-meguri-design/bcc4eb73-f59e-4573-8583-70b404dcd81d/scratchpad/rec/vncd.fifo
