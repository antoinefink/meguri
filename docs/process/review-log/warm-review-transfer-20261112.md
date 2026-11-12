# Warm review transfer observation

Clean source: `1ff8bee6`. Role: fixer.
Session: `s-20261110T214025Z-20fa15fd`.

A phone review previously arrived late on the Mac. At 19:49 UTC both devices
had four matching cards, three matching reviews and matching fixture media.
Compatibility USN is excluded from row equality. The original manual fetch
returned success without zone callbacks; a later automatic fetch delivered the
rows. Arrival time was not captured. Keep that delayed-delivery finding open.

A further warm transfer used normal signed products from the same clean source:
`build-app/20261112T195826Z-8994` on Mac and
`run/20261112T195855Z-10346` on the phone. The phone's status capture
`drive/20261112T200048Z-11848` passed three actions with verified continuity.
At 20:02 UTC the Mac saved one Easy grade. Its four review
records all had durable accepted acknowledgments when inspected at 20:04 UTC.

The phone's explicit Sync Now capture `drive/20261112T200320Z-12631` passed
three actions with verified continuity. A private copy at 20:05 UTC contains
four matching reviews and four matching card rows, apart from compatibility
USN. Both fixture media files still match byte for byte. This bounds observed
convergence; it does not isolate automatic delivery from the explicit request
or measure transport latency alone. Neither app was restarted during the leg.

Private app data, record identities and device values remain ignored. Image
rendering and media bytes match this fixture. Audio output has not been
verified. Delivery timing and status still need work. The final signed lanes
and milestone gates remain open. No unit or acceptance claim.
