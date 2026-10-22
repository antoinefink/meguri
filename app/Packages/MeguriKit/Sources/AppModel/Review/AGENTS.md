# Local review

- The record is presentation history, never a replay queue. Core owns accepted-answer recovery. A commit failure can arrive before answer() returns or after later cards were shown.
- Keep transition admission synchronous. Lifecycle pause waits for the current operation before persisting; layout disappearance does not end or recreate pages.
- Collection media comes from CoreMediaService.trackedNames and its stored case keys. Refresh on media events and resume; reject stale generation/root bindings.
