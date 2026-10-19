# Image tools (U8, R1952): ffmpeg for video frame extraction, scene-change and
# stall detection and contact sheets, and Python for the hierarchy check; for
# U10 also NumPy and Pillow (snapshot image diffs and the baseline guard),
# PyYAML (the screen-status ledger, the inventory) and jsonschema (critic
# output). The scripts are not copied in: they run from the bind-mounted
# checkout (tools/scripts/contact-sheet, hierarchy-check, image-diff,
# gallery, screen-status, critic). Build context
# is the repo root, filtered by image-tools.Dockerfile.dockerignore. Never
# pushed to a registry.

FROM debian:trixie-slim@sha256:a29215f6a35e51e22adffa17f89e9d2ef06214e64a2bad10d765c46aea49f11f AS image-tools

RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates ffmpeg fonts-dejavu-core jq python3 \
      python3-jsonschema python3-numpy python3-pil python3-yaml \
 && rm -rf /var/lib/apt/lists/* \
 && ffmpeg -hide_banner -version | head -n 1 \
 && for f in scdet drawtext tile; do ffmpeg -hide_banner -filters | grep -q " $f " || exit 1; done \
 && python3 -c 'import jsonschema, numpy, PIL, yaml'

ENV MG_CONTAINER=image-tools \
    PYTHONDONTWRITEBYTECODE=1
