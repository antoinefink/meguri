# docker/

- Never push an image to a registry. Pin every base image by digest (`FROM name:tag@sha256:...`); `tools/mg selftest` checks it.
- Drive compose through `tools/mg` (`mg_compose`). It sets a per-checkout project name, so worktrees never share an image tag, network or `cargo-target` volume, while the cargo registry volumes are shared. It also sets `BUILDX_NO_DEFAULT_ATTESTATIONS=1`: with a provenance attestation, every rebuild gets a new image id even when all layers are cached, and compose 5.5 ignores `provenance: false`.
- SQLite files and other test scratch go to the container's tmpfs `/tmp` (`TMPDIR`), never to the `/work` bind mount: virtiofs locking is slow and unreliable. Rust output goes to the `cargo-target` volume (`CARGO_TARGET_DIR`), never to `core/target/`, which holds host builds.
- The dev image installs exactly what `core/rust-toolchain.toml` lists, so rustup never needs the network at run time. The `FROM rust:<x.y>` tag must match that file's channel; bump both together. Each Dockerfile has a `<name>.Dockerfile.dockerignore` allowlist, because the build context is the repo root, which holds `local-data/` and `refs/`.
- Pin new tool versions as `ARG`s and install them with `cargo install --locked --version`.
- `cargo install` exits 0 when a binary's required features are off, so check each installed binary runs.
