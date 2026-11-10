# SPDX-License-Identifier: AGPL-3.0-or-later
"""AnkiConnect transcript recorder (specs/servers/transcripts, U136).

  python -m recorder --id <scenario id> --format yaml|json < scenario > golden.json

runs inside the oracle/ankiconnect image (entrypoint `record`), one scenario per
fresh container. `transcript` is the pure part (scenario checks, request bytes,
reply parsing, T10 normalization, entity tokens); `__main__` runs Anki, talks to
the meguri_recorder add-on and sends the exchanges.
"""
