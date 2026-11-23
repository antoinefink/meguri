# Local review

- The record is presentation history, never a replay queue. Core owns accepted-answer recovery. A commit failure can arrive before answer() returns or after later cards were shown.
- Keep transition admission synchronous. Lifecycle pause waits for the current operation before persisting; layout disappearance does not end or recreate pages.
- Only the person's input may set `.transition` over a card on screen. Event, deadline, day-check and media reads run with `underCard`: the side and its controls stay, and input is held, then given only to the same presentation (and, for an answer, the same token and labels). Every Meguri Sync round rebuilds the session, so a phase change there flashes the answer area after each answer.
- Collection media comes from CoreMediaService.trackedNames and its stored case keys. Refresh on media events and resume; reject stale generation/root bindings.
