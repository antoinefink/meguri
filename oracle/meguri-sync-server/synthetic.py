# SPDX-License-Identifier: AGPL-3.0-or-later
"""Original, public synthetic content; no stock assets or private inputs."""
FIXTURE = {"format": "meguri-sync-synthetic-v1", "notetype": "Aster recall",
           "fields": ["Cue", "Response"], "template": {"name": "Recall", "question": "{{Cue}}", "answer": "{{Response}}"},
           "css": "", "pending_grave_id": 424242, "notes": [["Aster cue", "Aster response"]],
           "edit": ["Aster cue", "Aster changed response"],
           "second_note": ["Birch cue", "Birch response"],
           "media": {"aster.bin": "Aster synthetic bytes", "birch.bin": "Birch synthetic bytes"}}

EDGE_FIXTURE = {'format':'meguri-sync-edges-v1','base':FIXTURE,'page_counts':[0,1,999,1000,1001],
                'media_prefix':'folio','media_payload_prefix':'Folio synthetic bytes ',
                'repeat_name':'repeat.bin','repeat_contents':'Replay synthetic bytes',
                'idle_seconds':121,'sdk_timeout_seconds':300,'idle_edit':'Aster idle response'}
