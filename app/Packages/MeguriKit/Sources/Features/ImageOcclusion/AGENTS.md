# ImageOcclusion

- The canvas's primary task is drawing and arranging masks; keep the image visible, tools above it on Mac/regular width and below on compact width. Secondary transforms live in native menus/inspector.
- Retain `MaskEditorSession` above a fields/masks layout switch, or keep the four-argument view mounted. New image loads require a new session/reference size. Binding echoes must not reload the codec or history.
- `commit` validates with Rust before changing history/document/binding. Keep source indexes and unknown properties; never reparse the writer's output as the editing source.
- The codec synchronously enters its dedicated document executor; it never waits on collection lanes. Keep completed edits ordered and measure large-document latency before acceptance.
- Image decoding belongs to the caller of the SwiftUI.Image API. This module has no media URL/data access or independent downsampling path.
- Card colours, Arial metrics, shape dimensions, zoom factors and transform thresholds implement mask-editor M1–M6 and image-occlusion-format. Raw colour construction stays in MaskAppearance; its and MaskCanvas's content exceptions are in tools/lint-ui/allowlist.tsv.
