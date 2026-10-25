# Meguri oracle image: the pinned `anki` wheel run headless under libfaketime,
# driven only through the JSON-lines protocol in conformance/schema/protocol.json.
# AGPL-3.0-or-later (oracle/LICENSE). Built locally as
# meguri-oracle:<anki version>-<hash of the image inputs> by `tools/mg oracle build`;
# never pushed to a registry (R1869).

FROM python:3.12-slim-trixie@sha256:a6e34c598f2467ed0e9a8d349809fcd8b5c603269512df273a0bb1784edc11b1

# libfaketime drives the clock; tzdata gives every worker its own TZ.
RUN apt-get update \
 && apt-get install -y --no-install-recommends libfaketime tzdata \
 && rm -rf /var/lib/apt/lists/*

COPY requirements.lock /opt/oracle/requirements.lock
RUN pip install --no-cache-dir --disable-pip-version-check --root-user-action=ignore \
      --require-hashes --no-deps -r /opt/oracle/requirements.lock \
 && python -c "import anki.collection"

RUN useradd --system --uid 10001 --home-dir /scratch --no-create-home oracle \
 && mkdir -p /scratch && chown oracle /scratch

COPY entrypoint.sh /opt/oracle/entrypoint.sh
COPY worker_holder.py /opt/oracle/worker_holder.py
COPY driver/ /opt/oracle/driver/

ENV PYTHONPATH=/opt/oracle \
    PYTHONDONTWRITEBYTECODE=1 \
    PYTHONUNBUFFERED=1 \
    ORACLE_HOME=/opt/oracle \
    TZ=UTC

USER oracle
WORKDIR /scratch
ENTRYPOINT ["/opt/oracle/entrypoint.sh"]
CMD ["worker"]
