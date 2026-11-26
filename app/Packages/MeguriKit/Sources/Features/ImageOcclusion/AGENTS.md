# ImageOcclusion

- The canvas's primary task is drawing and arranging masks; keep the image visible, tools above it on Mac/regular width and below on compact width. Secondary transforms live in native menus/inspector.
- Retain `MaskEditorSession` above a fields/masks layout switch, or keep the four-argument view mounted. New image loads require a new session/reference size. Binding echoes must not reload the codec or history.
- `commit` validates with Rust before changing history/document/binding. Keep source indexes and unknown properties; never reparse the writer's output as the editing source.
- The codec's synchronous facade waits for an interactive-lane call. Do not call it from a core lane/callback. Measure wait latency with large documents before acceptance.
- Image decoding belongs to the caller of the SwiftUI.Image API. This module has no media URL/data access or independent downsampling path.
- Card colours, Arial metrics, shape dimensions, zoom factors and transform thresholds implement mask-editor M1–M6 and image-occlusion-format; they are authored card content, not chrome. Integrator: register required raw-colour/font lint exceptions in the allowlist after review.
- Integrator must register `image_occlusion_parse`, `ImageOcclusionDocument.reading`, `image_occlusion_write`, `image_occlusion_allocate`, `image_occlusion_numbers` in Instrumentation's BridgeCall registry and exact-registry test. Until then codec calls deliberately refuse; do not substitute another export's interval.
