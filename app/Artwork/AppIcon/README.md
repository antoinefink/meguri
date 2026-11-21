# Meguri app icon

Antoine supplied this pomegranate image on 2026-11-18 after generating it with ChatGPT.

- `Pomegranate-original.png` preserves the supplied 1254 × 1254 image without changes.
- `../../Meguri/AppIcon.icon` is the editable Icon Composer document used by the app.
- `Preview-default.png` is the 1024 × 1024 default appearance exported from Icon Composer for review.

Original SHA-256: `194b6b332938255458281a64670803ea86c09e056ffe71bd515c7d056c69a883`.

The document fits the full image to the icon canvas. It keeps the original background and photographic shadow.
Additional layer glass effects, group shadows, specular highlights and translucency are disabled.
The system supplies the icon shape and appearance treatments.

The app selects `AppIcon` through `app/Config/Meguri.xcconfig`.
Keep originals and review exports here, outside the app's synchronized resource folder.

Antoine supplied `Pomegranate-transparent.png` on 2026-11-18: a 946 × 946 image with a transparent background, kept without changes.
SHA-256: `740fbc128ec58d78a7d0046c017987ca397469957248fd00a772c9df91b91255`.

Meguri Sync web icons in `server/web/src/` are scaled with `sips`: `favicon-64.png` and `favicon-512.png` from `Pomegranate-transparent.png`, and the opaque `apple-touch-icon.png` (180 × 180, masked by iOS) from `Pomegranate-original.png`.

Antoine approved the default icon preview on 2026-11-18.

## Development icon

Development builds (`.dev` ids: Debug, Profile, the personal preview) use `../../Meguri/AppIcon-Dev.icon`, named "Meguri Dev", so the test copy is never mistaken for the real one (Antoine, 2026-11-21: "maybe we could go with blue").
It is generated, never edited by hand: `tools/mg dev-icon` runs `tools/scripts/dev-icon/dev_icon.py`, which copies `icon.json` (converting any colour in it the same way) and converts each layer image.
Only the hue changes. In OKLCH, saturated pixels with hues from 350° to 65° move to the system blue family (236–264°). For bright pixels the upper limit drops to 45°. The main red, near 25°, ends near 249°; system blue `#0088FF` is at 254°. Lightness and chroma stay; chroma drops only where a colour would leave sRGB.
Neutral and bright pixels (background, pith) are unchanged. At the band's edges a partial change lowers chroma at the original hue and raises it at the new one, so it passes through grey, never through purple or teal. The output PNG carries pixels only, no metadata.
In the system's clear and tinted icon styles both icons are drawn monochrome, so there they differ only by name.
`tools/scripts/dev-icon/record.json` holds both documents' digests, the generator's and the parameters. Run `tools/mg dev-icon` after any change to `AppIcon.icon` or the generator; `tools/mg xcode-check` fails while the record does not match.
