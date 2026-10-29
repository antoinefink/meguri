# CloudKit policy

- `step` reads only supplied facts and time. Never infer completion from an idle adapter, durable flight rows, or an API return without operation outcomes.
- Driver accepts session/tracking/binding/evidence only from Runtime. Device settings, reachability and SDK operation outcomes are supplied events. No Host or Collection is retained.
- Unknown provider facts stay optional; do not fill missing media or attention evidence with zero. Publication and capture authority remain in existing Runtime services.
