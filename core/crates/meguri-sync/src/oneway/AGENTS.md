# Manual collection transfers

- Only explicit prepare/confirm/recovery calls may reach the network. Final confirmation uses the persisted preview revision; retain the configured identity through redirects.
- Drop Sync leases before requests. Check review admission before prep and after exclusive drain. Never prepare accepted upload bytes again.
- PossiblyAccepted is globally fenced, including after cancellation/restart; stamps are not recovery proof. R6 evidence proves local activation only. MediaIncomplete is a partial error, never whole-operation success.
