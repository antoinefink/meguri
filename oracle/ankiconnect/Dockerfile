# syntax=docker/dockerfile:1.7
# AnkiConnect transcript recorder (U136): the pinned Anki desktop app
# (aqt 26.9.3) on the oracle image, offscreen under libfaketime, with the
# AnkiConnect add-on (GPL-3.0-or-later) pinned in ADDON_PIN. AGPL-3.0-or-later
# quarantine (LICENSE); built locally only, never pushed (R1869).
# Build context is oracle/ankiconnect/. ORACLE_BASE is a meguri-oracle image
# tag built by `tools/mg oracle build`; see README.md.
ARG ORACLE_BASE
FROM ${ORACLE_BASE}

USER root
# Shared libraries PyQt6 6.11 and QtWebEngine load even on the offscreen
# platform (aqt also dlopens libGL.so.1 on Linux). The aarch64 QtWebEngine
# wheel links the system snappy, opus, webp, minizip and lcms2.
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

COPY requirements.lock /opt/ankiconnect/requirements.lock
RUN pip install --no-cache-dir --disable-pip-version-check --root-user-action=ignore \
      --require-hashes --no-deps -r /opt/ankiconnect/requirements.lock \
 && python -c "import aqt, PyQt6.QtWebEngineCore"

# The add-on: upstream source archive pinned by hash (ADDON_PIN repeats both
# values), then every installed file checked against ADDON_PIN.
ADD --checksum=sha256:cf75820e00ebc28ba0cdcaedbbf646cac3a745121dc9de33b99020ca61a658da \
    https://git.sr.ht/~foosoft/anki-connect/archive/532a3c816e08e91c4367e2799ce3779403d70274.tar.gz \
    /tmp/anki-connect.tar.gz
COPY ADDON_PIN /opt/ankiconnect/ADDON_PIN
RUN mkdir -p /tmp/ac /opt/ankiconnect/addons21/2055492159 \
 && tar -xzf /tmp/anki-connect.tar.gz -C /tmp/ac --strip-components=1 \
 && cp /tmp/ac/plugin/* /opt/ankiconnect/addons21/2055492159/ \
 && cp /tmp/ac/LICENSE /opt/ankiconnect/addons21/2055492159/LICENSE.upstream \
 && cd /opt/ankiconnect/addons21/2055492159 \
 && grep -E '^[0-9a-f]{64}  ' /opt/ankiconnect/ADDON_PIN | sha256sum -c --strict - \
 && [ "$(ls | grep -v '^LICENSE.upstream$' | wc -l)" = "$(grep -cE '^[0-9a-f]{64}  ' /opt/ankiconnect/ADDON_PIN)" ] \
 && rm -rf /tmp/ac /tmp/anki-connect.tar.gz

COPY addons21/meguri_recorder/ /opt/ankiconnect/addons21/meguri_recorder/
COPY recorder/ /opt/ankiconnect/recorder/
COPY launch.py entrypoint.sh /opt/ankiconnect/

USER oracle
WORKDIR /scratch
ENTRYPOINT ["/opt/ankiconnect/entrypoint.sh"]
CMD ["anki"]
