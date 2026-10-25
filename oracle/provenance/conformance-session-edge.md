# Per-session host edge for mixed conformance runs

`conformance/sync-ops#S8.4` defines Meguri-owned host allocation and cleanup.
It adds no claim about Anki behavior and required no reference-source research.

The reviewed own-source audit is `/tmp/meguri-u107-actor-wiring-audit.md`.
At contract intake, S10.4 already reports the per-session actors endpoint.
Parties validates that response and installs an endpoint in Seat, while the
CLI's static optional edge cannot describe parallel dynamic proxy ports.
The MG conformance bootstrap already supplies the fault-service configuration
and labels run-owned containers for cleanup. The existing rig forwarder
supplies the internal/publishing-network and TCP-close preservation pattern;
the real-clock rig itself is excluded from scenario execution.
The actual MG live replay uses the host-native CLI; only T0 has a dev-container
lane. Thus the edge's host loopback and Seat's client namespace coincide.

The initial claim that no oracle response or driver change was needed was
incorrect. The bounded follow-up checked the actual Meguri-owned wrapper and
fault lifecycle, without reading Anki implementation. The pinned fault service
is plain toxiproxy. Its named listener persists until deletion, but the oracle
wrapper's FaultProxy.delete and SyncRun.close remove the proxy and stop the
per-session server. Worker EOF closes its active session independently of any
parent reap. The backend has an ephemeral port in the worker's container;
preserving only the proxy would leave upstream reuse possible too.

The frozen own implementation inspected is
`build/mg/cleanroom/20261025T175143Z-33572/source`: its pool checks out one
worker exclusively, each worker command starts a separate compose-run
container, and the proposed host wrapper only holds stdin/reap ordering.
It cannot retain the actual namespace after an independent driver exit.

S8.4 and S10.4 therefore add a neutral route ownership member, deferred mixed
proxy deletion from creation, and a container-local holder retaining each
worker namespace independently of the driver. The host helper validates
ownership and removes edge, proxy, then namespace. Failed removal leaves the
remaining route reserved. No general supervisor/control API or fault-service
lease extension is needed; concurrent workers keep distinct namespaces.
The independent contract review found that run membership alone did not bind
a reply to the actual checked-out worker. The trusted launch factory must
retain that worker's assigned identity before driver launch; Parties/helper
compare the reported route to it independently. A complete valid route for
another same-run worker is refused before publication or deletion.

Oracle-maintainer owns the driver-side token reply and mixed proxy cleanup,
plus the quarantined entrypoint portion of the holder. Infra owns MG launch,
resource labels, host allocation/release and fault-service restart admission.
The filtered implementer owns only neutral protocol decoding, helper request
fields and existing pool/session ownership. Implementers receive the neutral
contract/brief, never this provenance or oracle source. Each role keeps its
own implementation and tests; no freshness or execution result is inferred.

Session: s-20261025T132312Z-c047b13c
Unit context: U107; original batch base 1416a520542a8c5daa38a23bfe398da704c3d6c9.
This contract carries no unit acceptance or golden freshness claim.
