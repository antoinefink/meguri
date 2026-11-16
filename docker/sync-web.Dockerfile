# SPDX-License-Identifier: MIT
FROM node:26.13.1-bookworm-slim@sha256:5df79989ed685e09f0a98019db4cebfb22fb9df1a53c0161664a011f7ff6b0f1 AS node
FROM rust:1.99-slim@sha256:2752b332db73fdbb7dc576f06c82ed1f312005784ef913d7e04a28f5f55dc581
COPY --from=node /usr/local/ /usr/local/
RUN apt-get update && apt-get install -y --no-install-recommends \
    libatomic1 ca-certificates openssl python3 jq clang cmake pkg-config \
    libnss3 libnspr4 libdbus-1-3 libatk1.0-0t64 libatk-bridge2.0-0t64 \
    libcups2t64 libdrm2 libxkbcommon0 libxcomposite1 libxdamage1 libxfixes3 \
    libxrandr2 libgbm1 libpango-1.0-0 libcairo2 libasound2t64 fonts-liberation fonts-dejavu-core \
    && rm -rf /var/lib/apt/lists/*
ENV RUSTUP_TOOLCHAIN=1.99.0 PLAYWRIGHT_BROWSERS_PATH=/browsers XDG_CACHE_HOME=/tmp/xdg-cache
RUN node --version && npm --version && rustc --version \
    && node -e 'if (process.version !== "v26.13.1" || process.release.lts !== "Lithium") process.exit(1)'
COPY server/web/scripts/ /opt/sync-web/
ENTRYPOINT ["bash", "/opt/sync-web/run.sh"]
