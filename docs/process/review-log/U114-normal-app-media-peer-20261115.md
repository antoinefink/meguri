# Normal-app media peer checkpoint

Both normal development images were built from clean `f046120fd7bb379eb0c948bc45c9f9d1c002b5d2`. The Mac uses the signed Personal Preview Release image; the physical iPhone uses the normal signed Debug image. Device identifiers stay only in ignored private receipts.

Evidence:

- `build-app/20261115T101523Z-23788`: Mac Release build passed. The old preview quit normally. The exact new product was opened; one matching process was observed.
- `cloud-media-probe/20261115T102730Z-31059`: guarded staging passed for one fresh synthetic file. Staging alone was not transfer evidence. The Mac then quit and reopened normally so its usual startup could discover the file.
- `run/20261115T102925Z-32565`: normal physical preparation passed, with source unchanged through build, zero build errors or warnings, and normal_launch_verified true. Preparation installs and launches; it does not fixture, reset, uninstall or copy collection data. Continuity remains unknown.
- `cloud-media-probe/20261115T103250Z-35345`: PASS, transfer_verified true. The file read from the iPhone matched the Mac's frozen 10,240 bytes and SHA-256 `f2af87a5876db57254ceff69fff300543e72c0c992423ab1184b993ffb0a2b9a`. The verifier only copies from the phone; it does not insert the file there.

One file reached the iPhone with the exact bytes. This does not establish retained review records or general recovery behavior.

Q7 and P24: pending. Release: pending.
