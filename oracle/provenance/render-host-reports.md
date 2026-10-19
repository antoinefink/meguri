# U67 isolated host-report contract provenance

Session: s-20261019T224053Z-a9f830d4. Spec: render/host-reports H1–H10, with media-serving M8/M9 links. This protocol is original Meguri design from the existing Product Contract U175/U176 and accepted C66/native security boundaries. No reference source, asset, fixture or oracle research was used.

The design supplies strict bounded observations, not a general native API. A distinct isolated handler and native pair opt-in prevent the existing page-event relay from impersonating host reports. The same Meguri-owned isolated world satisfies the existing own-content-world wording. Card id, generation, side and native geometry revision bind emission; uncertain binding disables reporting. Missing host/probe execution remains unavailable rather than pretending to be ready. Reply callbacks produce actual JavaScript null once for every outcome.

Native U67 work covers validation, context/probe calls and known-geometry invalidation. U176/U180/U181 build the observers and verify freshness. Synthetic tests cannot close hardware checks or prove a future producer. Existing S1/S2, device and reference-capture obligations are unchanged. The ignored design proposal records root approval; this spec records the agreed contract.
