# Master backup pushes

Antoine lifted the temporary local-only push restriction on 2026-11-17.
Resume normal pushes of master to origin without force. This backs up committed
work; it does not grant unit, batch, gate or release acceptance.

The outgoing privacy check `private/20261117T073232Z-77629` flagged only the
session ledger. Static check `static-checks/20261117T073236Z-78356` passed.

Independent non-author reviewer `/root/provider_public_snapshot_review` read
the complete appended record, including every field and path, and classified
it PUBLIC. It contains public operational metadata. No unresolved authorship
or licensing issue was identified for that record.

The integrator verified the unchanged 1,014-line, 421,959-byte prefix against
the previously admitted SHA-256
`1134a229cd83506fbc3405017f11401493b74e2475556438546030adca86fb46`.
Original ledger-only append commit
`4f9f923942bf5f402bd9d266079a9b84951e1479` remains in history.
Its one-record, 346-byte suffix is the reviewed append.

The candidate ledger has 1,015 lines and 422,305 bytes, SHA-256
`be4e3d95b0853e39f2b7da1e33fcf4474da829a593b30835a87704cf284b316c`.
The existing public-file map receives only this ledger digest, as delegated in
provenance.md. Scanner logic, its private list and access rules are unchanged.
The failed app gate and its original batch base remain in the verification
register. Focused passes do not clear that gate.
