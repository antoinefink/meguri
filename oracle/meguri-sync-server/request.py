# SPDX-License-Identifier: AGPL-3.0-or-later
"""A closed request vocabulary; neither commands nor arbitrary endpoints."""
from validation import require


def check_request(value):
    require(isinstance(value, dict) and set(value) == {'format','operation','client','peer'}, 'malformed harness request')
    require(value['format'] == 'meguri-sync-request-v1' and value['operation'] == 'smoke', 'unsupported harness request')
    require(value['client'] in {'desktop','mobile'}, 'unknown client target')
    require(value['peer'] in {'reference','candidate'}, 'unknown peer target')
    return value
