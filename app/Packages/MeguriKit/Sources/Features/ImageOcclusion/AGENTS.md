# ImageOcclusion

- The canvas's primary task is drawing and arranging masks; keep the image visible, tools above it on Mac/regular width and below on compact width. Secondary transforms live in native menus/inspector.
- Retain `MaskEditorSession` above a fields/masks layout switch, or keep the four-argument view mounted. New image loads require a new session/reference size. Binding echoes must not reload the codec or history.
- `commit` validates with Rust before changing history/document/binding. Keep source indexes and unknown properties; never reparse the writer's output as the editing source.
- The codec synchronously enters its dedicated document executor; it never waits on collection lanes. Keep completed edits ordered and measure large-document latency before acceptance.
- Image decoding belongs to the caller of the SwiftUI.Image API. This module has no media URL/data access or independent downsampling path.
- Card colours, Arial metrics, shape dimensions, zoom factors and transform thresholds implement mask-editor M1–M6 and image-occlusion-format. Raw colour construction stays in MaskAppearance; its and MaskCanvas's content exceptions are in tools/lint-ui/allowlist.tsv.
- Selection alone never transforms geometry or writes. Navigation cancels all in-flight transforms through their final callback; handle and canvas callbacks share one lifecycle.
- Mask commands belong to focused canvas, tool and inspector controls. Hidden/disabled editors and text entry relinquish them; native Edit history prefers an enabled mask step, then the host route.
- Imported CSS fills are decoded only for drawing; keep their stored text. Custom collection palettes require a typed atomic K9 read/mutate operation; do not substitute device preferences or a read-then-write adapter.

- On Mac, keyboard focus is a sibling of the canvas, never a SwiftUI `.focusable()` ancestor: that wrapper can collapse the entire AX subtree. `NativeCanvasAccessibility` owns image/mask/handle AX frames in flipped canvas coordinates and returns nil from mouse hit testing; keep it outside hidden drawing/navigation layers. Initialize W/H only after a usable layout, then retain it across resizes.
