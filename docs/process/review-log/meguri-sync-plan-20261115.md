# Meguri Sync planning review — 2026-11-15

Plan: [Meguri Sync Server](../../plans/2026-11-15-1028-feat-meguri-sync-server-plan.md).

Starting commit: `38235917e6a025daa57625c582e90d253aae62e3`.
Spec-writer session: `s-20261115T090224Z-47bf0815`.

The requirements were confirmed in conversation. Go, MIT for independently
authored server code, separate collections per account, the minimal shadcn web
interface, and current Desktop/AnkiMobile interoperability remain settled.
The Meguri application is outside this work. No PR is authorized; local isolated
checkouts are authorized.

Two independent confidence reviews identified data-safety corrections. The plan
now defines publication lock ordering, credential-revision checks at issuance,
collection-generation pinning, durable setup ownership, and stable data-root
locking with WAL recovery before stopped backups. A restored server cannot
reliably detect every stale native media cursor. Restoration therefore requires
a verified per-client full-collection and media-reset procedure, with actual
Desktop and Mobile evidence before claiming support.

The subsequent full document review completed in seven separate read-only
contexts: coherence, feasibility, security, design, scope, product, and
adversarial. All returned empty findings, residual-risk, and deferred-question
arrays. No further plan edits were required by that review. This is planning
evidence, not implementation or interoperability acceptance.

The optional Claude pass was attempted through the isolated review worker. All
four legs stopped before egress because Node was unavailable to its launcher;
no cross-model review or document transfer occurred. The seven local reviews
provide the completed coverage.

Clean-room enforcement remains required before implementation: neutral specs and
fixtures only; isolated implementers and fixers; separate quarantined reference
drivers; provenance, privacy, similarity, independent source review, and release
content checks. Existing similarity tooling does not yet scan Go. Its extension
and calibration are part of the first implementation unit, not an assumed pass.

Runtime verification and actual device evidence remain outstanding. Exact review
returns and launcher outcomes were retained under the private temporary review
directory `/tmp/compound-engineering-501/ce-doc-review/meguri-sync-20261115`.
