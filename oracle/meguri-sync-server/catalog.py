# SPDX-License-Identifier: AGPL-3.0-or-later
"""Expand the neutral contract's named probe obligations without omitting targets."""

FAMILIES = {
    "P-T01": ("sync/server-transport#T2", "identity routes-methods header-members unicode-client exact-integers native-login"),
    "P-T02": ("sync/server-transport#T3", "raw-zstd no-argument-padding content-length chunked original-size null-result integer-result text-result zip-result"),
    "P-T03": ("sync/server-transport#T2", "missing-header malformed-header duplicate-members invalid-version unknown-key revoked-key empty-session changed-session endpoint-case endpoint-verb browser-cookie"),
    "P-T04": ("sync/server-transport#T4", "malformed-zstd truncated-zstd multiframe-zstd size-boundaries nesting-boundaries wide-ids signed-64-ids strict-tuples loose-tuples duplicate-envelope unsupported-schema"),
    "P-T05": ("sync/server-transport#T3", "private-https error-400 error-403 error-409 error-413 error-429 error-503 http-version no-redirect"),
    "P-F01": ("sync/server-collection#C1", "initial-stamps required-schema initial-meta fresh-fresh no-card-metadata"),
    "P-F02": ("sync/server-collection#C6", "full-round-trip upload-file-form download-file-form wal-self-contained required-migration upload-stamps download-stamps repeated-download no-double-preparation"),
    "P-F03": ("sync/server-collection#C1", "equal-mod-different-scm equal-scm-different-mod cards-neither cards-local cards-server cards-both metadata-without-cards media-without-cards existing-migration fresh-receiver direction-confirmation"),
    "P-F04": ("sync/server-collection#C6", "notes cards reviews scheduling decks notetypes fields templates presets config unknown-json unknown-protobuf-full unknown-protobuf-normal untouched-bytes changed-bytes unicase-quirk"),
    "P-F05": ("sync/server-collection#C6", "corrupt-file missing-table newer-schema required-older-schema harmless-foreign-object unsafe-trigger unsafe-view strict-refusal-no-mutation"),
    "P-F06": ("sync/server-lifecycle#L3", "upload-same-key-round upload-other-key-round download-same-key-round download-other-key-round stale-finish old-generation lost-download-reply media-preserved"),
    "P-N01": ("sync/server-lifecycle#L2", "account-binding same-key-start other-key-start wrong-session absent-session repeated-phase out-of-phase after-done abort-absent abort-wrong abort-matching malformed-input idle-expiry restart visibility error-retirement"),
    "P-N02": ("sync/server-collection#C2", "usn-below usn-equal usn-above negative-cursor ahead-cursor outgoing-snapshot response-before-merge tuples-0 tuples-249 tuples-250 tuples-251 tuples-500 last-empty-chunk grave-split exact-outgoing-ids"),
    "P-N03": ("sync/server-collection#C3", "grave-present grave-absent grave-pending-edited independent-note independent-card independent-deck repeated-type conflicting-type default-deck hierarchy deletion-then-tuple deletion-then-object grave-stamps"),
    "P-N04": ("sync/server-collection#C3", "pending-older pending-equal pending-newer nonpending-older nonpending-equal nonpending-newer changed-field-count changed-template-count missing-parent normal-filtered-replacement case-collision repeated-plus-collision preset-duplicate resulting-stamps"),
    "P-N05": ("sync/server-collection#C3", "tag-normalization tag-spelling tag-parents tag-collapsed tag-config lnewer-true lnewer-false conf-missing conf-null conf-present crt-missing crt-null crt-present whole-config-replacement config-stamps"),
    "P-N06": ("sync/server-collection#C4", "card-pending-older card-pending-equal card-pending-newer card-nonpending-older card-nonpending-equal card-nonpending-newer note-pending-older note-pending-equal note-pending-newer note-nonpending-older note-nonpending-equal note-nonpending-newer pending-usn-below pending-usn-equal pending-usn-above accepted-invalid-note ignored-invalid-note checksum sort-affinity ascii-controls nfc flags data missing-notetype field-count"),
    "P-N07": ("sync/server-collection#C4", "repeat-within-chunk repeat-across-chunks supplied-usn absent-id nonpending-id pending-id mod-30-10 mod-10-20 mod-10-30 invalid-then-valid first-review-collision distinct-tables"),
    "P-N08": ("sync/server-collection#C5", "sanity-due-zero sanity-grave-zero six-table-mismatch malformed-count retirement finish-stamps row-stamps same-clock-rounds abort-rollback error-rollback restart-rollback"),
    "P-N09": ("sync/server-collection#C5", "lost-finish-before-commit lost-finish-after-commit restart-retry review-replay grave-replay object-replay config-replay equal-stamp-divergence"),
    "P-N10": ("sync/server-collection#C2", "no-change one-review method-counts byte-counts tuple-counts media-counts no-full-transfer fresh-content fresh-history fresh-scheduling"),
    "P-M01": ("sync/server-media#M1", "five-methods envelopes empty-sk no-collection-session no-begin-required concurrent-normal exact-usn exact-hash"),
    "P-M02": ("sync/server-media#M2", "upload-array deletion-position download-object stored-zip deflated-zip crc missing-member repeated-member unknown-member repeated-real-name repeated-reference safe-filename"),
    "P-M03": ("sync/server-media#M2", "names-25 names-26 metadata-50kib name-255bytes file-empty file-100mib fill-crossing incompressible-overhead nfc case backslash controls path"),
    "P-M04": ("sync/server-media#M3", "add replace delete identical-noop absent-noop repeated-name processed-prefix processed-all usn-tombstones actual-sha1"),
    "P-M05": ("sync/server-media#M3", "page-0 page-1 page-999 page-1000 page-1001 same-name-update negative-cursor ahead-cursor equal-cursor intervening-writer equal-count-different-bytes missing-disk"),
    "P-M06": ("sync/server-media#M5", "ordered-prefix empty-request missing-after-known zip-bytes zip-metadata concurrent-replace concurrent-delete interrupted-reply lost-upload-ack replay restart"),
    "P-M07": ("sync/server-media#M1", "two-way server-wins deletion-before-confirmation pending-delete-live reset-rescan case-variants fresh-hashes no-change-no-retransmission"),
    "P-R01": ("sync/server-lifecycle#L6", "desktop-menu-procedure authoritative-direction renewed-credentials recovered-content recovered-media"),
    "P-R02": ("sync/server-lifecycle#L6", "mobile-device-procedure authoritative-direction renewed-credentials recovered-content recovered-media"),
    "P-R03": ("sync/server-lifecycle#L6", "delayed-reconnect equal-mod-different-scm cursor-ahead cursor-equal cursor-reused cursor-overtaken equal-count-different-bytes local-edits local-reviews old-hostkey"),
}


def catalog():
    cases = []
    for family, (spec_id, variants) in FAMILIES.items():
        clients = ["desktop", "mobile"]
        lanes = ["reference", "candidate"]
        if family.startswith("P-R"):
            lanes = ["candidate"]
        if family == "P-R01": clients = ["desktop"]
        if family == "P-R02": clients = ["mobile"]
        for variant in variants.split():
            for lane in lanes:
                for client in clients:
                    cases.append({"id": f"{family}:{client}.{lane}.{variant}", "family": family,
                                  "classification": "current_probe", "spec_ids": [spec_id],
                                  "variants": [variant], "lane_kind": lane, "client_kind": client,
                                  "status": "unobserved"})
    return {"format": "meguri-sync-catalog-v1", "baseline_release": "26.09.3",
            "family_count": len(FAMILIES), "required_case_count": len(cases), "cases": cases,
            "claim": "Selection obligations only; no execution or compatibility credit."}
