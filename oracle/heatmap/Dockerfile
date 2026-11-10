# syntax=docker/dockerfile:1.7
# Heatmap add-on oracle (oracle/AGENTS.md "Add-on oracles";
# specs/conformance/stats-ops.md H1, H4, H11): the Meguri oracle plus the
# Review Heatmap add-on 1.0.1 (AGPL-3.0), whose own computation the
# heatmap_totals and heatmap_report observations run headless
# (driver/observe/heatmap_addon.py). AGPL-3.0-or-later quarantine
# (oracle/LICENSE); built locally only, never pushed (R1869).
#
# Build context is oracle/heatmap/ (this file alone). ORACLE_BASE is the
# meguri-oracle:<tag> that `tools/mg oracle build` prints: its driver/ holds
# the module and its pin file, so the add-on image follows the base's driver.
# README.md says how to build and test it and what tools/mg would need.
ARG ORACLE_BASE
FROM ${ORACLE_BASE}

USER root
# The add-on: the public source archive of tag v1.0.1, pinned by hash, then
# the installed files (the repository's LICENSE and src/review_heatmap/,
# nothing else) checked against driver/observe/heatmap_addon.sha256: the
# sha256 of their sorted sha256sum listing, and their count.
ADD --checksum=sha256:c75ed89223ad557ff51342dfa9e683c5267407a954a46443a710699913594397 \
    https://codeload.github.com/glutanimate/review-heatmap/tar.gz/5ab7964ac5bbab33358ef9cd5dd4f31517f43e48 \
    /tmp/review-heatmap.tar.gz
RUN mkdir -p /tmp/rh /opt/oracle/addons/review-heatmap/src \
 && tar -xzf /tmp/review-heatmap.tar.gz -C /tmp/rh --strip-components=1 --no-same-owner \
 && cp /tmp/rh/LICENSE /opt/oracle/addons/review-heatmap/LICENSE \
 && cp -R /tmp/rh/src/review_heatmap /opt/oracle/addons/review-heatmap/src/review_heatmap \
 && cd /opt/oracle/addons/review-heatmap \
 && [ -z "$(find . ! -type f ! -type d)" ] \
 && pin="$(grep -E '^[0-9a-f]{64}  [0-9]+$' /opt/oracle/driver/observe/heatmap_addon.sha256)" \
 && got="$(find . -type f -printf '%P\n' | LC_ALL=C sort | xargs -d '\n' sha256sum | sha256sum | cut -d' ' -f1)  $(find . -type f | wc -l)" \
 && { [ "$got" = "$pin" ] || { echo "review-heatmap: installed tree $got is not the pin $pin" >&2; exit 1; }; } \
 && chmod -R a+rX,go-w /opt/oracle/addons \
 && rm -rf /tmp/rh /tmp/review-heatmap.tar.gz

ENV ORACLE_HEATMAP_ADDON=/opt/oracle/addons/review-heatmap \
    ORACLE_EXTRA_MODULES=driver.observe.heatmap_addon

USER oracle
WORKDIR /scratch
# Importing the module checks the add-on against its pin again and loads its
# computation modules headless: the build fails on any other add-on build.
RUN python -c "import driver.observe.heatmap_addon"
