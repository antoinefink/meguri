# Meguri dev image: the Rust toolchain for build, test, lint, license gate and
# benches (stage `dev`), plus a pinned nightly for cargo-fuzz (stage `fuzz`).
# Build context is the repo root, filtered by dev.Dockerfile.dockerignore.
# Never pushed to a registry.

FROM rust:1.99-slim@sha256:2752b332db73fdbb7dc576f06c82ed1f312005784ef913d7e04a28f5f55dc581 AS dev

# C toolchain for bundled SQLite, zstd and aws-lc-sys; valgrind for
# instruction-count benches; bats, jq, git and python3-jsonschema for the
# tools/mg self-tests.
RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      bats ca-certificates clang cmake git jq pkg-config python3-jsonschema valgrind \
 && rm -rf /var/lib/apt/lists/*

ARG CARGO_NEXTEST_VERSION=0.9.148
ARG CARGO_DENY_VERSION=0.20.2
ARG CARGO_ABOUT_VERSION=0.9.2
RUN --mount=type=cache,id=meguri-dev-cargo-registry,target=/usr/local/cargo/registry \
    cargo install --locked --version "${CARGO_NEXTEST_VERSION}" cargo-nextest \
 && cargo install --locked --version "${CARGO_DENY_VERSION}" cargo-deny \
 && cargo install --locked --version "${CARGO_ABOUT_VERSION}" --features cli cargo-about \
 && cargo nextest --version && cargo deny --version && cargo about --version

# Install exactly what core/rust-toolchain.toml lists, so rustup never needs
# the network when cargo runs in /work/core. The Apple targets come along:
# their std is unused here but satisfies the toolchain file.
COPY core/rust-toolchain.toml /opt/meguri/rust-toolchain.toml
RUN cd /opt/meguri && rustup toolchain install && rustup show active-toolchain

# PyYAML for tools/mg design-docs (docs/design/screen-inventory.yaml). Its own
# layer, after the cargo installs, so their cached layers stay valid.
RUN apt-get update \
 && apt-get install -y --no-install-recommends python3-yaml \
 && rm -rf /var/lib/apt/lists/*

# gungraun's runner for the instruction-count benches (tools/mg bench-icount,
# U12). It must be exactly the version of the gungraun crate in
# core/Cargo.toml, or every bench exits with a version error.
ARG GUNGRAUN_RUNNER_VERSION=0.20.0
RUN --mount=type=cache,id=meguri-dev-cargo-registry,target=/usr/local/cargo/registry \
    cargo install --locked --version "${GUNGRAUN_RUNNER_VERSION}" gungraun-runner \
 && gungraun-runner --version

# File-level Bats parallelism; keep the expensive Rust install layers cached.
RUN apt-get update \
 && apt-get install -y --no-install-recommends parallel \
 && rm -rf /var/lib/apt/lists/*

ENV MG_CONTAINER=dev

FROM dev AS fuzz
ARG FUZZ_NIGHTLY=nightly-2026-10-01
ARG CARGO_FUZZ_VERSION=0.13.2
RUN --mount=type=cache,id=meguri-dev-cargo-registry,target=/usr/local/cargo/registry \
    rustup toolchain install "${FUZZ_NIGHTLY}" --profile minimal \
 && cargo install --locked --version "${CARGO_FUZZ_VERSION}" cargo-fuzz
ENV MG_CONTAINER=fuzz \
    MG_FUZZ_TOOLCHAIN=${FUZZ_NIGHTLY}
