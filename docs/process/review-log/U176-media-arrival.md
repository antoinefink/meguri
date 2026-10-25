# U176 — local media arrivals

Resume base: `9190a5528ca794c83254604867b36b6aa156d649`, recorded in
`build/unit-starts/U176-media-arrival-resume-20261025.json` before work.
Integration base remains `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
This is a partial renderer attachment, with no Unit trailer.

The owner updates its media index but does not notify the active card of
arrivals. Reuse the existing in-place resource update; preserve the visible
DOM, input and scripts. No transfer protocol changes are included.

Implementer session `s-20261025T134357Z-235af9a2`, export
`213535Z-87592`, adds scoped arrival delivery and lifecycle checks.
Swift `214306Z-92971` passes 23 focused tests, including the generated-image
DOM case. Privacy `214326Z-94248` and similarity `214333Z-94517` pass.

Independent round one finds one P2: an already prepared next card can retain
a missing image after arrivals are consumed on the visible card. Repair and
a two-card regression remain required. Full integration, device media and
hardware audio evidence remain open. This WIP is not accepted.
