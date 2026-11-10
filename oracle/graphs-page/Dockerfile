# syntax=docker/dockerfile:1.7
# Graphs-page oracle (specs/conformance/stats-ops.md H14,
# stats/graph-presentation P22, stats/graphs-common G12 decision 2): the
# Meguri oracle plus Anki's desktop package aqt 26.9.3, whose built graphs
# page (the sveltekit bundle in the wheel) runs in an offscreen QtWebEngine
# for the stats_presentation observation (driver/observe/stats_presentation.py,
# whose runner and probe are in driver/observe/stats_presentation_page/).
# The page and its assets stay in this image (R1960). AGPL-3.0-or-later
# quarantine (oracle/LICENSE); built locally only, never pushed (R1869).
#
# Build context is oracle/ (`-f graphs-page/Dockerfile`; Dockerfile.dockerignore
# sends only the lock): the image installs oracle/ankiconnect/requirements.lock,
# aqt[qt] resolved on the base lock. ORACLE_BASE is the meguri-oracle:<tag>
# that `tools/mg oracle build` prints: its driver/ holds the observation
# module, so this image follows the base's driver. README.md says how to build
# and probe it and what tools/mg would need.
ARG ORACLE_BASE
FROM ${ORACLE_BASE}

USER root
# Shared libraries PyQt6 6.11 and QtWebEngine load even on the offscreen
# platform (the list of oracle/ankiconnect/Dockerfile).
RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      libgl1 libegl1 libopengl0 libglib2.0-0t64 libfontconfig1 libfreetype6 \
      libdbus-1-3 libxkbcommon0 libnss3 libasound2t64 libgbm1 libdrm2 \
      libxcomposite1 libxdamage1 libxrandr2 libxtst6 libxkbfile1 libxshmfence1 \
      libx11-6 libx11-xcb1 libxcb1 libxext6 libxfixes3 libxi6 libxrender1 \
      libxcb-cursor0 libxkbcommon-x11-0 libxcb-icccm4 libxcb-image0 \
      libxcb-keysyms1 libxcb-randr0 libxcb-render-util0 libxcb-shape0 \
      libxcb-xinerama0 libxcb-xkb1 fonts-dejavu-core \
      libsnappy1v5 libopus0 libwebp7 libwebpdemux2 libwebpmux3 libminizip1t64 \
      liblcms2-2 libpulse0 \
 && rm -rf /var/lib/apt/lists/*

COPY ankiconnect/requirements.lock /opt/oracle/graphs-page/requirements.lock
RUN pip install --no-cache-dir --disable-pip-version-check --root-user-action=ignore \
      --require-hashes --no-deps -r /opt/oracle/graphs-page/requirements.lock \
 && python -c "import _aqt, PyQt6.QtWebEngineCore"

ENV ORACLE_GRAPHS_PAGE=/opt/oracle/graphs-page \
    ORACLE_EXTRA_MODULES=driver.observe.stats_presentation

USER oracle
WORKDIR /scratch
# The module imports, and the wheel holds the page it serves.
RUN python -c "import driver.observe.stats_presentation as m; m._bundle_root()"
