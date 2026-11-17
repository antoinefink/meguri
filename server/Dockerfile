# SPDX-License-Identifier: MIT
# MG supplies a frozen, allowlisted context with audited Cargo license JSON.
FROM node:26.13.1-bookworm-slim@sha256:5df79989ed685e09f0a98019db4cebfb22fb9df1a53c0161664a011f7ff6b0f1 AS web
WORKDIR /source/server/web
COPY server/web/ ./
RUN node -e 'if (process.version !== "v26.13.1" || process.release.lts !== "Lithium") process.exit(1)' \
 && npm ci --ignore-scripts --no-audit --no-fund --engine-strict --fetch-retries=0 \
 && mkdir /web-evidence \
 && node scripts/notices.mjs . /web-evidence \
 && MEGURI_WEB_NOTICES_FILE=/web-evidence/runtime-notices.txt npm run build \
 && MEGURI_WEB_NOTICES_FILE=/web-evidence/runtime-notices.txt node scripts/embed.mjs dist /prepared

FROM rust:1.99-slim@sha256:2752b332db73fdbb7dc576f06c82ed1f312005784ef913d7e04a28f5f55dc581 AS build
# musl-tools is build-only: a static executable avoids shipping a distribution
# runtime and its package manager. No additional application dependency is used.
# Keep Debian archive signatures; only historical Valid-Until is disabled.
# Snapshot/package availability is verified by the actual MG image build.
RUN rm -f /etc/apt/sources.list /etc/apt/sources.list.d/* \
 && printf '%s\n' \
    'deb [check-valid-until=no signed-by=/usr/share/keyrings/debian-archive-keyring.gpg] https://snapshot.debian.org/archive/debian/20261115T000000Z/ trixie main' \
    'deb [check-valid-until=no signed-by=/usr/share/keyrings/debian-archive-keyring.gpg] https://snapshot.debian.org/archive/debian/20261115T000000Z/ trixie-updates main' \
    'deb [check-valid-until=no signed-by=/usr/share/keyrings/debian-archive-keyring.gpg] https://snapshot.debian.org/archive/debian-security/20261115T000000Z/ trixie-security main' \
    > /etc/apt/sources.list \
 && apt-get update \
 && apt-get install -y --no-install-recommends \
    musl=1.2.5-3.1~deb13u1 musl-dev=1.2.5-3.1~deb13u1 musl-tools=1.2.5-3.1~deb13u1 \
 && mkdir /evidence \
 && dpkg-query -W > /evidence/static-packages.txt \
 && musl-gcc -v > /evidence/static-compiler.txt 2>&1 \
 && (dpkg-query -L musl musl-dev musl-tools | while IFS= read -r file; do \
       if [ -f "$file" ]; then sha256sum "$file"; fi; done; \
     sha256sum "$(readlink -f "$(command -v gcc)")" "$(musl-gcc -print-prog-name=cc1)" \
       "$(musl-gcc -print-file-name=libgcc.a)" "$(musl-gcc -print-file-name=libgcc_eh.a)") \
     > /evidence/static-inputs.sha256 \
 && rm -rf /var/lib/apt/lists/*
ENV RUSTUP_TOOLCHAIN=1.99.0 MEGURI_WEB_PREPARED_DIR=/prepared CARGO_NET_RETRY=0
WORKDIR /source
COPY core/Cargo.toml core/rust-toolchain.toml core/
COPY core/crates/meguri-model/ core/crates/meguri-model/
COPY server/Cargo.toml server/Cargo.lock server/rust-toolchain.toml server/build.rs server/
COPY server/src/ server/src/
COPY server/web/ server/web/
COPY --from=web /prepared/ /prepared/
COPY --from=web /web-evidence/ /evidence/web/
COPY package-inputs.json rust-notices.json covered-source.tar /evidence/
WORKDIR /source/server
RUN set -eu; \
    case "$(uname -m)" in x86_64) target=x86_64-unknown-linux-musl;; aarch64) target=aarch64-unknown-linux-musl;; *) exit 1;; esac; \
    rustup target add "$target"; \
    rustc -Vv > /evidence/rustc.txt; \
    cargo tree --locked --no-default-features --target "$target" --edges normal,build,features > /evidence/production-features.txt; \
    CC=musl-gcc RUSTFLAGS='-C linker=musl-gcc -C target-feature=+crt-static' \
      cargo build --locked --release --no-default-features --target "$target" --bin meguri-sync-server \
      --message-format json-render-diagnostics > /evidence/release.jsonl; \
    mkdir -p /runtime/usr/local/bin /runtime/usr/share/meguri-sync/licenses/rust /runtime/data/owner; \
    cp "target/$target/release/meguri-sync-server" /runtime/usr/local/bin/meguri-sync-server; \
    readelf -l /runtime/usr/local/bin/meguri-sync-server > /evidence/elf.txt; \
    ! grep -q INTERP /evidence/elf.txt; \
    sha256sum /runtime/usr/local/bin/meguri-sync-server | cut -d " " -f 1 > /evidence/executable.sha256; \
    cp /usr/share/doc/musl/copyright /runtime/usr/share/meguri-sync/licenses/musl.txt; \
    rust_docs="$(rustc --print sysroot)/share/doc/rust"; \
    sha256sum "$rust_docs/COPYRIGHT.html" "$rust_docs/COPYRIGHT-library.html" > /evidence/rust-notice-inputs.sha256; \
    cp "$rust_docs/COPYRIGHT.html" "$rust_docs/COPYRIGHT-library.html" /runtime/usr/share/meguri-sync/licenses/rust/; \
    cp /usr/share/common-licenses/GPL-3 /runtime/usr/share/meguri-sync/licenses/; \
    find /usr/share/doc -path '*gcc*' -name copyright -exec cat {} \; > /runtime/usr/share/meguri-sync/licenses/compiler-runtime.txt; \
    test -s /runtime/usr/share/meguri-sync/licenses/compiler-runtime.txt; \
    chmod 0700 /runtime/data /runtime/data/owner; \
    chown -R 10001:10001 /runtime/data; \
    printf '%s\n' "$target" > /evidence/target.txt
COPY server/LICENSE /runtime/usr/share/meguri-sync/licenses/server-MIT.txt
COPY server/web/LICENSE /runtime/usr/share/meguri-sync/licenses/web-MIT.txt
COPY server/web/THIRD_PARTY_NOTICES.txt /runtime/usr/share/meguri-sync/licenses/imported-components.txt
COPY server/web/upstream-records.json /runtime/usr/share/meguri-sync/upstream-records.json
COPY server/NOTICE /runtime/usr/share/meguri-sync/NOTICE
RUN cp -R /evidence/. /runtime/usr/share/meguri-sync/ \
 && cp /prepared/assets.json /runtime/usr/share/meguri-sync/assets.json

# Test tooling is a separate image target, never a runtime layer.
FROM build AS smoke
COPY --from=web /usr/local/ /usr/local/
RUN apt-get update && apt-get install -y --no-install-recommends openssl libatomic1 \
 && rm -rf /var/lib/apt/lists/*
COPY server/examples/package-native-smoke.rs examples/package-native-smoke.rs
RUN set -eu; target="$(cat /evidence/target.txt)"; \
    CC=musl-gcc RUSTFLAGS='-C linker=musl-gcc -C target-feature=+crt-static' \
    cargo build --locked --release --no-default-features --target "$target" --example package-native-smoke; \
    mkdir /smoke; cp "target/$target/release/examples/package-native-smoke" /smoke/native
COPY server/tools/package-smoke.mjs /smoke/package-smoke.mjs
ENTRYPOINT ["node", "/smoke/package-smoke.mjs"]

FROM scratch AS runtime
COPY --from=build /runtime/ /
USER 10001:10001
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/meguri-sync-server"]
