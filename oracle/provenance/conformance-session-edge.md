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

No oracle response member or driver implementation change is needed. The
testkit/CLI owner wires the neutral allocation capability and scoped lease;
the MG/Docker infrastructure owner implements the host helper and forwarder
lifecycle. Implementers receive S8.4 and the neutral implementation brief,
not this provenance file or the oracle tree.

Session: s-20261025T132312Z-c047b13c
Unit context: U107; original batch base 1416a520542a8c5daa38a23bfe398da704c3d6c9.
This contract carries no unit acceptance or golden freshness claim.
