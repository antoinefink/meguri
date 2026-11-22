---
title: Attach images, audio and PDFs to notes through MCP
type: feat
status: active
date: 2026-11-22
product_contract_source: ce-plan-bootstrap
---

# Attach images, audio and PDFs to notes through MCP

## Goal Capsule

A Meguri Sync member can ask their connected assistant to save a picture, pronunciation clip or PDF in a new or existing note and study the result offline after normal sync.

## Product Contract

### Problem Frame

The collection MCP saves field text but cannot transfer a file. A note can therefore contain a media reference that the sync server cannot supply to a device.

### Requirements

- R1. `create_note` and `edit_note` accept ChatGPT file inputs and their target note fields in one call. Keep the normal text-only calls and their results usable.
- R2. When the client cannot pass a file, give the member a signed-in Meguri upload link. The assistant can use the resulting upload ID in one or more note writes before it expires.
- R3. Accept JPEG, PNG, GIF, WebP, AVIF, and HEIC or HEIF images. Accept MP3, WAV, FLAC, AAC, M4A, AIFF, CAF, Ogg Opus and Vorbis audio. Accept PDF documents. Meguri generates the saved filenames and the field references. An upload cannot replace a different file or affect another account.
- R4. Retain the existing note revision, OAuth revocation, publication, reset, and native sync rules. A failed attachment leaves the note unchanged. A retry of the same upload reuses the media instead of adding a second copy.

### Acceptance Examples

- AE1. Attach a picture to Front and pronunciation to Back in one `create_note` call. After the next normal collection and media sync, the generated card shows the picture and plays the clip offline.
- AE2. Add a picture to an existing field through `edit_note`. An omitted field begins with its stored text. An edit with an old revision leaves the note unchanged.
- AE3. Open the fallback link while signed in to the consenting account, upload a clip, and attach it to two notes. A different account, different grant, expired request, revoked grant, or reset cannot redeem the upload.

### Key Decisions

- KTD1 (session-settled, user-approved). Keep the directly attached file in the create or edit call. A required separate tool call adds an extra failure point for clients that can supply the file.
- KTD2. The browser fallback stages an upload. The note write publishes it through the same media service as a direct file. The two data stores keep their own commits. Durable media may remain unused when the subsequent note commit fails.
- KTD3. Start with the existing write grant because attachments enter the collection through a note write. Explain attachment access in the consent and connection descriptions. Require browser sign-in to the same member account for the fallback.

### Scope Boundaries

Accept user-supplied image, audio and PDF files. Native card rendering handles the new references. Link import, media authoring, overwriting media and deletion are outside this change. Hosted file delivery for each client and format requires a live generated-account check before claiming support.

## Planning Contract

- Existing note tooling and consent are specified in [MCP M3–M7](../../specs/servers/meguri-collection-mcp.md#m3-consent-tokens-and-revocation). Extend the contract with an attachment section before implementation.
- [Media M2–M4](../../specs/sync/server-media.md#m2-bounds-and-names) defines private immutable blobs, verified equality, and media USNs. Reuse the media index and download path. Native upload can replace a logical filename. The new note attachment path needs insert-or-reuse semantics.
- [Lifecycle L4](../../specs/sync/server-lifecycle.md#l4-revocation-and-lock-order) governs publication. Capture the account data epoch and collection publication before staging. Keep downloads and byte validation outside its barrier. Recheck the original grant, publication, data epoch, and observed media rows inside it. Admit a normal round as before.
- [ChatGPT file inputs](https://developers.openai.com/plugins/reference#define-file-inputs) use top-level file objects with download URLs. Restrict egress to the approved exact HTTPS origin, enforce the public-address and redirect policy, and keep links out of diagnostics. The [Claude connector guide](https://claude.com/docs/connectors/building) does not establish a matching file input contract.

## Implementation Units

### U1. Attachments, publication and native sync

Extend the two tool descriptors and inputs, acquire bounded files, and place server-generated references in the requested fields. Reuse existing note construction, revision checking, media storage and native media listing. Wire publication before note commit, after note validation. Verify direct create and edit, malformed inputs, expiry or revocation during acquisition, stale edits, retry, occupied filenames, media storage failure and a real native sync readback.

### U2. Signed-in upload fallback

Add `prepare_media_upload`, a same-origin authenticated file picker and a bounded upload route. Bind expiring upload records to the requesting grant and account data epoch. Use the same validation and attachment path as U1. Verify the same-account browser journey, wrong-account and stale requests, and reuse across note writes.

### U3. Expanded formats and offline PDF viewing

Keep one format policy across ChatGPT file inputs and the signed-in fallback. Verify the accepted formats by their bytes and display or play them through the card media path. Keep a PDF in the collection media inventory and open its local bytes in a native viewer on explicit activation from the current card. Verify broad format acceptance, bound reader refusal and stale-card cancellation.

## Verification Contract

- Run `tools/mg sync-server-test`, `tools/mg sync-server-lint`, and the locked dependency audit for any changed runtime dependency. Run the repository browser verification for the fallback page.
- Use generated disposable accounts for hosted checks. Exercise PNG or JPEG and MP3, both as directly attached when supported and through the fallback. Verify the resulting note fields and media bytes through native sync and offline study.
- Open a ready PR against `master`, retain the local evidence and inherited failures, and resolve the AI review threads. Check the final commit's required jobs and review state.
