# Test-only AGPL reference runtime; never a production build stage.
FROM python:3.12-slim-trixie@sha256:a6e34c598f2467ed0e9a8d349809fcd8b5c603269512df273a0bb1784edc11b1
COPY oracle/meguri-sync-server/requirements.lock /opt/reference/requirements.lock
RUN pip install --no-cache-dir --disable-pip-version-check --root-user-action=ignore \
    --require-hashes --no-deps --report /opt/reference/install-report.json \
    -r /opt/reference/requirements.lock \
 && python -c "import anki.collection, jsonschema, zstandard"
COPY oracle/meguri-sync-server/ /opt/reference/
COPY conformance/schema/meguri-sync-server.schema.json /opt/reference/schema.json
COPY conformance/catalog/meguri-sync-server.json /opt/reference/neutral/catalog.json
COPY conformance/fixtures/meguri-sync-server/original.json /opt/reference/neutral/fixture.json
COPY conformance/recordings/scenarios/meguri-sync-server/desktop-selection.json /opt/reference/neutral/selection.json
COPY conformance/recordings/scenarios/meguri-sync-server/desktop-edge-selection.json /opt/reference/neutral/edge-selection.json
COPY conformance/fixtures/meguri-sync-server/edges.json /opt/reference/neutral/edge-fixture.json
COPY conformance/recordings/scenarios/meguri-sync-server/candidate-full-selection.json /opt/reference/neutral/candidate-full-selection.json
COPY conformance/recordings/scenarios/meguri-sync-server/candidate-normal-selection.json /opt/reference/neutral/candidate-normal-selection.json
COPY conformance/recordings/scenarios/meguri-sync-server/candidate-media-selection.json /opt/reference/neutral/candidate-media-selection.json
COPY conformance/recordings/scenarios/meguri-sync-server/desktop-media-selection.json /opt/reference/neutral/media-selection.json
COPY conformance/fixtures/meguri-sync-server/media.json /opt/reference/neutral/media-fixture.json
COPY conformance/recordings/scenarios/meguri-sync-server/candidate-complete-selection.json /opt/reference/neutral/candidate-complete-selection.json
COPY conformance/recordings/scenarios/meguri-sync-server/candidate-final-additions.json /opt/reference/neutral/candidate-final-additions.json
COPY conformance/recordings/scenarios/meguri-sync-server/final-fixture-recipes.json /opt/reference/neutral/final-fixture-recipes.json
COPY conformance/fixtures/meguri-sync-server/recovery.json /opt/reference/neutral/recovery-fixture.json
COPY conformance/recordings/scenarios/meguri-sync-server/desktop-recovery-selection.json /opt/reference/neutral/recovery-selection.json
COPY conformance/fixtures/meguri-sync-server/recovery-dirty.json /opt/reference/neutral/recovery-dirty-fixture.json
COPY conformance/recordings/scenarios/meguri-sync-server/desktop-recovery-dirty-selection.json /opt/reference/neutral/recovery-dirty-selection.json
ENV PYTHONDONTWRITEBYTECODE=1 PYTHONUNBUFFERED=1 TMPDIR=/scratch TZ=UTC
WORKDIR /scratch
USER 10001:10001
ENTRYPOINT ["python", "/opt/reference/driver.py"]
