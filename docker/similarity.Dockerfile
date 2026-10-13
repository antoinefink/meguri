# Similarity scanner image (U2, R1957): the Python winnowing scanner and JPlag
# for the nightly second opinion. The scanner's code is not copied in: it runs
# from the bind-mounted checkout (PYTHONPATH). Build context is the repo root,
# filtered by similarity.Dockerfile.dockerignore. Never pushed to a registry.

FROM eclipse-temurin:25-jre-noble@sha256:d9a39a23634650173f1e2bbc176227af9728587ecf0f4b62d53e9355cd7a19ab AS similarity

# git runs as root on bind-mounted repositories owned by the host user.
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates curl git python3 python3-numpy python3-yaml \
 && rm -rf /var/lib/apt/lists/* \
 && git config --system safe.directory '*'

ARG JPLAG_VERSION=6.3.0
ARG JPLAG_SHA256=5f2c21e8b88ed77134effcb3a5a3ab13d188f6a3e16d401387f7479e92db9aa2
RUN mkdir -p /opt/jplag \
 && curl -fsSL -o /opt/jplag/jplag.jar \
      "https://github.com/jplag/JPlag/releases/download/v${JPLAG_VERSION}/jplag-${JPLAG_VERSION}-jar-with-dependencies.jar" \
 && echo "${JPLAG_SHA256}  /opt/jplag/jplag.jar" | sha256sum -c - \
 && java -jar /opt/jplag/jplag.jar --help >/dev/null

# Unrelated Rust for the negative calibration: fsrs-rs (BSD-3-Clause), which
# implementers may read. Two releases give 30+ distinct source files.
ARG FSRS_CRATES="6.6.2=89836c0a26b347b585b23824034600b69bfc7f0b0112d0cd656791799eb3a717 1.4.9=a98432a03a213c1913372a40adb29132bec7dbd3562cd9c00cc440b477e5a904"
RUN mkdir -p /opt/calibration/fsrs \
 && for spec in ${FSRS_CRATES}; do \
      v="${spec%%=*}"; sum="${spec#*=}"; \
      curl -fsSL -o "/tmp/fsrs-$v.crate" "https://static.crates.io/crates/fsrs/fsrs-$v.crate" \
      && echo "$sum  /tmp/fsrs-$v.crate" | sha256sum -c - \
      && tar -xzf "/tmp/fsrs-$v.crate" -C /opt/calibration/fsrs \
      && rm "/tmp/fsrs-$v.crate" || exit 1; \
    done

ENV MG_CONTAINER=similarity \
    PYTHONPATH=/work/tools/similarity \
    PYTHONDONTWRITEBYTECODE=1 \
    JPLAG_JAR=/opt/jplag/jplag.jar \
    FSRS_CALIBRATION_DIR=/opt/calibration/fsrs
