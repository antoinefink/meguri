#!/usr/bin/env python3
"""Generated-only RF4 package proof; stdlib staging and strict public-output checks.

Inputs must be frozen, generated, owned build/ artifacts. This tool cannot establish
that their content is synthetic: the caller owns that responsibility. Never point
it at real collections. Only package + DB enter the readonly oracle fixture mount;
expected media stays local. Conservative fixed clock scope: UTC in 2000..2037
with creationOffset=0, or Europe/Paris only at 2030-01-15T10:00:00+01:00
with current offset +3600 seconds and creationOffset=-60 minutes west. This is
not general timezone or DST support. Both require valid rollover, schedVer=2,
creation before now. Both explicit timing queries must match D3 exactly, with no
clock movement or permitted timing difference. Raw dumps precede timing. Before
any client opens, an isolated libzstd decoder proves the package DB's exact raw
rows against the frozen expected DB (including stamps), and independently checks
the complete map and every payload against captured names/lengths/SHA-1s. No repair is run.
All SQLite reads stay in that pinned service; Python 3.9 host paths consume only
bounded neutral raw rows and clock evidence bound to the frozen input hashes.
The registered unicase callback only latches and refuses: no Unicode comparison
is implemented or certified. Raw scans use NOT INDEXED and explicit BINARY keys.

Conservative GENERATED-FIXTURE scope (not the app's intake limits): package and
individual source files <=512 MiB, DB <=256 MiB, SQLite cells/row payloads <=1 MiB, <=100,000
rows total, <=8,192 filesystem/archive entries, <=4 MiB central directory, no
ZIP64/multidisk/comment/preamble/data descriptors, stored ZIP entries only
(native P15 output), contiguous local records matching the central directory.
Zstd uses <=128 MiB window, <=256 MiB decoded DB and max(16 MiB, 1000*encoded
entry bytes) for every entry; decoded media <=512 MiB each, map <=64 MiB,
total decoded data <=16 GiB. Native proof requires exactly one ordinary frame
per entry, declared content size, checksum and no dictionary; libzstd checks
completion, content length and checksum. Compression level 3 has no frame field
and cannot be inferred from encoded bytes. These are utility scope restrictions,
not new product limits. Native map uint32 values must not use overflowing wire
encodings. The generic bounded stream primitive retains P4 support.
Staging stores at most package + two DBs (1 GiB) plus the <=64 MiB decoded map;
no media payload is staged. decoded_media_map_sha256 hashes the exact decoded
protobuf bytes, including unknown fields, not a reconstructed JSON inventory.
imported_media_inventory_sha256 hashes UTF-8 canonical() JSON of client inventory.
JSON <=16 MiB / 200,000 lexical tokens / depth 24; clock keys <=64 bytes.
Media names use NFC, exact case, Unicode 10 assignment (F1) and M8 only.
Apple source spelling is selected from the staging host's sys.platform, never
the package map. Source entries are never renamed.
"""
import argparse
import bisect
import ctypes
import datetime as dt
import hashlib
import io
import json
import math
import os
from pathlib import Path
import re
import sqlite3
import stat
import sys
import struct
import unicodedata
import zipfile

ID = 'rf4/package-interop'
BLOCKED = {'oracle', 'refs', 'local-data', 'private', 'credentials',
           'oracle-macos-venv', 'similarity-index'}
COLUMNS = {
    'col': 'id crt mod scm ver dty usn ls conf models decks dconf tags',
    'notes': 'id guid mid mod usn tags flds sfld csum flags data',
    'cards': 'id nid did ord mod usn type queue due ivl factor reps lapses left odue odid flags data',
    'revlog': 'id cid usn ease ivl lastIvl factor time type',
    'graves': 'oid type usn',
    'decks': 'id name mtime_secs usn common kind',
    'deck_config': 'id name mtime_secs usn config',
    'notetypes': 'id name mtime_secs usn config',
    'fields': 'ntid ord name config',
    'templates': 'ntid ord name mtime_secs usn config',
    'tags': 'tag usn collapsed config',
    'config': 'KEY usn mtime_secs val',
}
HEX64 = re.compile(r'[0-9a-f]{64}\Z')
HEX40 = re.compile(r'[0-9a-f]{40}\Z')
CHUNK = 1024 * 1024
SOURCE_LIMIT = 512 * 1024 * 1024
DB_LIMIT = 256 * 1024 * 1024
MAP_LIMIT = 64 * 1024 * 1024
TOTAL_LIMIT = 16 * 1024 * 1024 * 1024
CELL_LIMIT = 1024 * 1024
ROW_LIMIT = 100000
ENTRY_LIMIT = 8192
JSON_LIMIT = 16 * 1024 * 1024
JSON_TOKENS = 200000
JSON_DEPTH = 24
CLOCK_LIMIT = 64
NATIVE_TZ = 'Europe/Paris'
NATIVE_START = '2030-01-15T10:00:00+01:00'
NATIVE_NOW = 1894698000
KEYS = {'graves': ('oid', 'type'), 'fields': ('ntid', 'ord'),
        'templates': ('ntid', 'ord'), 'tags': ('tag',), 'config': ('KEY',)}
# Unicode 10 Cn ranges from Meguri's own generated unicode10_unassigned.rs;
# UnicodeData.txt SHA-256 52423e4d7492167b62f518f68d54db88930abbbff7f11edfcaec8f726498cab1.
UNASSIGNED10 = (
    (0x0378, 0x0379), (0x0380, 0x0383), (0x038B, 0x038B), (0x038D, 0x038D),
    (0x03A2, 0x03A2), (0x0530, 0x0530), (0x0557, 0x0558), (0x0560, 0x0560),
    (0x0588, 0x0588), (0x058B, 0x058C), (0x0590, 0x0590), (0x05C8, 0x05CF),
    (0x05EB, 0x05EF), (0x05F5, 0x05FF), (0x061D, 0x061D), (0x070E, 0x070E),
    (0x074B, 0x074C), (0x07B2, 0x07BF), (0x07FB, 0x07FF), (0x082E, 0x082F),
    (0x083F, 0x083F), (0x085C, 0x085D), (0x085F, 0x085F), (0x086B, 0x089F),
    (0x08B5, 0x08B5), (0x08BE, 0x08D3), (0x0984, 0x0984), (0x098D, 0x098E),
    (0x0991, 0x0992), (0x09A9, 0x09A9), (0x09B1, 0x09B1), (0x09B3, 0x09B5),
    (0x09BA, 0x09BB), (0x09C5, 0x09C6), (0x09C9, 0x09CA), (0x09CF, 0x09D6),
    (0x09D8, 0x09DB), (0x09DE, 0x09DE), (0x09E4, 0x09E5), (0x09FE, 0x0A00),
    (0x0A04, 0x0A04), (0x0A0B, 0x0A0E), (0x0A11, 0x0A12), (0x0A29, 0x0A29),
    (0x0A31, 0x0A31), (0x0A34, 0x0A34), (0x0A37, 0x0A37), (0x0A3A, 0x0A3B),
    (0x0A3D, 0x0A3D), (0x0A43, 0x0A46), (0x0A49, 0x0A4A), (0x0A4E, 0x0A50),
    (0x0A52, 0x0A58), (0x0A5D, 0x0A5D), (0x0A5F, 0x0A65), (0x0A76, 0x0A80),
    (0x0A84, 0x0A84), (0x0A8E, 0x0A8E), (0x0A92, 0x0A92), (0x0AA9, 0x0AA9),
    (0x0AB1, 0x0AB1), (0x0AB4, 0x0AB4), (0x0ABA, 0x0ABB), (0x0AC6, 0x0AC6),
    (0x0ACA, 0x0ACA), (0x0ACE, 0x0ACF), (0x0AD1, 0x0ADF), (0x0AE4, 0x0AE5),
    (0x0AF2, 0x0AF8), (0x0B00, 0x0B00), (0x0B04, 0x0B04), (0x0B0D, 0x0B0E),
    (0x0B11, 0x0B12), (0x0B29, 0x0B29), (0x0B31, 0x0B31), (0x0B34, 0x0B34),
    (0x0B3A, 0x0B3B), (0x0B45, 0x0B46), (0x0B49, 0x0B4A), (0x0B4E, 0x0B55),
    (0x0B58, 0x0B5B), (0x0B5E, 0x0B5E), (0x0B64, 0x0B65), (0x0B78, 0x0B81),
    (0x0B84, 0x0B84), (0x0B8B, 0x0B8D), (0x0B91, 0x0B91), (0x0B96, 0x0B98),
    (0x0B9B, 0x0B9B), (0x0B9D, 0x0B9D), (0x0BA0, 0x0BA2), (0x0BA5, 0x0BA7),
    (0x0BAB, 0x0BAD), (0x0BBA, 0x0BBD), (0x0BC3, 0x0BC5), (0x0BC9, 0x0BC9),
    (0x0BCE, 0x0BCF), (0x0BD1, 0x0BD6), (0x0BD8, 0x0BE5), (0x0BFB, 0x0BFF),
    (0x0C04, 0x0C04), (0x0C0D, 0x0C0D), (0x0C11, 0x0C11), (0x0C29, 0x0C29),
    (0x0C3A, 0x0C3C), (0x0C45, 0x0C45), (0x0C49, 0x0C49), (0x0C4E, 0x0C54),
    (0x0C57, 0x0C57), (0x0C5B, 0x0C5F), (0x0C64, 0x0C65), (0x0C70, 0x0C77),
    (0x0C84, 0x0C84), (0x0C8D, 0x0C8D), (0x0C91, 0x0C91), (0x0CA9, 0x0CA9),
    (0x0CB4, 0x0CB4), (0x0CBA, 0x0CBB), (0x0CC5, 0x0CC5), (0x0CC9, 0x0CC9),
    (0x0CCE, 0x0CD4), (0x0CD7, 0x0CDD), (0x0CDF, 0x0CDF), (0x0CE4, 0x0CE5),
    (0x0CF0, 0x0CF0), (0x0CF3, 0x0CFF), (0x0D04, 0x0D04), (0x0D0D, 0x0D0D),
    (0x0D11, 0x0D11), (0x0D45, 0x0D45), (0x0D49, 0x0D49), (0x0D50, 0x0D53),
    (0x0D64, 0x0D65), (0x0D80, 0x0D81), (0x0D84, 0x0D84), (0x0D97, 0x0D99),
    (0x0DB2, 0x0DB2), (0x0DBC, 0x0DBC), (0x0DBE, 0x0DBF), (0x0DC7, 0x0DC9),
    (0x0DCB, 0x0DCE), (0x0DD5, 0x0DD5), (0x0DD7, 0x0DD7), (0x0DE0, 0x0DE5),
    (0x0DF0, 0x0DF1), (0x0DF5, 0x0E00), (0x0E3B, 0x0E3E), (0x0E5C, 0x0E80),
    (0x0E83, 0x0E83), (0x0E85, 0x0E86), (0x0E89, 0x0E89), (0x0E8B, 0x0E8C),
    (0x0E8E, 0x0E93), (0x0E98, 0x0E98), (0x0EA0, 0x0EA0), (0x0EA4, 0x0EA4),
    (0x0EA6, 0x0EA6), (0x0EA8, 0x0EA9), (0x0EAC, 0x0EAC), (0x0EBA, 0x0EBA),
    (0x0EBE, 0x0EBF), (0x0EC5, 0x0EC5), (0x0EC7, 0x0EC7), (0x0ECE, 0x0ECF),
    (0x0EDA, 0x0EDB), (0x0EE0, 0x0EFF), (0x0F48, 0x0F48), (0x0F6D, 0x0F70),
    (0x0F98, 0x0F98), (0x0FBD, 0x0FBD), (0x0FCD, 0x0FCD), (0x0FDB, 0x0FFF),
    (0x10C6, 0x10C6), (0x10C8, 0x10CC), (0x10CE, 0x10CF), (0x1249, 0x1249),
    (0x124E, 0x124F), (0x1257, 0x1257), (0x1259, 0x1259), (0x125E, 0x125F),
    (0x1289, 0x1289), (0x128E, 0x128F), (0x12B1, 0x12B1), (0x12B6, 0x12B7),
    (0x12BF, 0x12BF), (0x12C1, 0x12C1), (0x12C6, 0x12C7), (0x12D7, 0x12D7),
    (0x1311, 0x1311), (0x1316, 0x1317), (0x135B, 0x135C), (0x137D, 0x137F),
    (0x139A, 0x139F), (0x13F6, 0x13F7), (0x13FE, 0x13FF), (0x169D, 0x169F),
    (0x16F9, 0x16FF), (0x170D, 0x170D), (0x1715, 0x171F), (0x1737, 0x173F),
    (0x1754, 0x175F), (0x176D, 0x176D), (0x1771, 0x1771), (0x1774, 0x177F),
    (0x17DE, 0x17DF), (0x17EA, 0x17EF), (0x17FA, 0x17FF), (0x180F, 0x180F),
    (0x181A, 0x181F), (0x1878, 0x187F), (0x18AB, 0x18AF), (0x18F6, 0x18FF),
    (0x191F, 0x191F), (0x192C, 0x192F), (0x193C, 0x193F), (0x1941, 0x1943),
    (0x196E, 0x196F), (0x1975, 0x197F), (0x19AC, 0x19AF), (0x19CA, 0x19CF),
    (0x19DB, 0x19DD), (0x1A1C, 0x1A1D), (0x1A5F, 0x1A5F), (0x1A7D, 0x1A7E),
    (0x1A8A, 0x1A8F), (0x1A9A, 0x1A9F), (0x1AAE, 0x1AAF), (0x1ABF, 0x1AFF),
    (0x1B4C, 0x1B4F), (0x1B7D, 0x1B7F), (0x1BF4, 0x1BFB), (0x1C38, 0x1C3A),
    (0x1C4A, 0x1C4C), (0x1C89, 0x1CBF), (0x1CC8, 0x1CCF), (0x1CFA, 0x1CFF),
    (0x1DFA, 0x1DFA), (0x1F16, 0x1F17), (0x1F1E, 0x1F1F), (0x1F46, 0x1F47),
    (0x1F4E, 0x1F4F), (0x1F58, 0x1F58), (0x1F5A, 0x1F5A), (0x1F5C, 0x1F5C),
    (0x1F5E, 0x1F5E), (0x1F7E, 0x1F7F), (0x1FB5, 0x1FB5), (0x1FC5, 0x1FC5),
    (0x1FD4, 0x1FD5), (0x1FDC, 0x1FDC), (0x1FF0, 0x1FF1), (0x1FF5, 0x1FF5),
    (0x1FFF, 0x1FFF), (0x2065, 0x2065), (0x2072, 0x2073), (0x208F, 0x208F),
    (0x209D, 0x209F), (0x20C0, 0x20CF), (0x20F1, 0x20FF), (0x218C, 0x218F),
    (0x2427, 0x243F), (0x244B, 0x245F), (0x2B74, 0x2B75), (0x2B96, 0x2B97),
    (0x2BBA, 0x2BBC), (0x2BC9, 0x2BC9), (0x2BD3, 0x2BEB), (0x2BF0, 0x2BFF),
    (0x2C2F, 0x2C2F), (0x2C5F, 0x2C5F), (0x2CF4, 0x2CF8), (0x2D26, 0x2D26),
    (0x2D28, 0x2D2C), (0x2D2E, 0x2D2F), (0x2D68, 0x2D6E), (0x2D71, 0x2D7E),
    (0x2D97, 0x2D9F), (0x2DA7, 0x2DA7), (0x2DAF, 0x2DAF), (0x2DB7, 0x2DB7),
    (0x2DBF, 0x2DBF), (0x2DC7, 0x2DC7), (0x2DCF, 0x2DCF), (0x2DD7, 0x2DD7),
    (0x2DDF, 0x2DDF), (0x2E4A, 0x2E7F), (0x2E9A, 0x2E9A), (0x2EF4, 0x2EFF),
    (0x2FD6, 0x2FEF), (0x2FFC, 0x2FFF), (0x3040, 0x3040), (0x3097, 0x3098),
    (0x3100, 0x3104), (0x312F, 0x3130), (0x318F, 0x318F), (0x31BB, 0x31BF),
    (0x31E4, 0x31EF), (0x321F, 0x321F), (0x32FF, 0x32FF), (0x4DB6, 0x4DBF),
    (0x9FEB, 0x9FFF), (0xA48D, 0xA48F), (0xA4C7, 0xA4CF), (0xA62C, 0xA63F),
    (0xA6F8, 0xA6FF), (0xA7AF, 0xA7AF), (0xA7B8, 0xA7F6), (0xA82C, 0xA82F),
    (0xA83A, 0xA83F), (0xA878, 0xA87F), (0xA8C6, 0xA8CD), (0xA8DA, 0xA8DF),
    (0xA8FE, 0xA8FF), (0xA954, 0xA95E), (0xA97D, 0xA97F), (0xA9CE, 0xA9CE),
    (0xA9DA, 0xA9DD), (0xA9FF, 0xA9FF), (0xAA37, 0xAA3F), (0xAA4E, 0xAA4F),
    (0xAA5A, 0xAA5B), (0xAAC3, 0xAADA), (0xAAF7, 0xAB00), (0xAB07, 0xAB08),
    (0xAB0F, 0xAB10), (0xAB17, 0xAB1F), (0xAB27, 0xAB27), (0xAB2F, 0xAB2F),
    (0xAB66, 0xAB6F), (0xABEE, 0xABEF), (0xABFA, 0xABFF), (0xD7A4, 0xD7AF),
    (0xD7C7, 0xD7CA), (0xD7FC, 0xD7FF), (0xFA6E, 0xFA6F), (0xFADA, 0xFAFF),
    (0xFB07, 0xFB12), (0xFB18, 0xFB1C), (0xFB37, 0xFB37), (0xFB3D, 0xFB3D),
    (0xFB3F, 0xFB3F), (0xFB42, 0xFB42), (0xFB45, 0xFB45), (0xFBC2, 0xFBD2),
    (0xFD40, 0xFD4F), (0xFD90, 0xFD91), (0xFDC8, 0xFDEF), (0xFDFE, 0xFDFF),
    (0xFE1A, 0xFE1F), (0xFE53, 0xFE53), (0xFE67, 0xFE67), (0xFE6C, 0xFE6F),
    (0xFE75, 0xFE75), (0xFEFD, 0xFEFE), (0xFF00, 0xFF00), (0xFFBF, 0xFFC1),
    (0xFFC8, 0xFFC9), (0xFFD0, 0xFFD1), (0xFFD8, 0xFFD9), (0xFFDD, 0xFFDF),
    (0xFFE7, 0xFFE7), (0xFFEF, 0xFFF8), (0xFFFE, 0xFFFF), (0x1000C, 0x1000C),
    (0x10027, 0x10027), (0x1003B, 0x1003B), (0x1003E, 0x1003E), (0x1004E, 0x1004F),
    (0x1005E, 0x1007F), (0x100FB, 0x100FF), (0x10103, 0x10106), (0x10134, 0x10136),
    (0x1018F, 0x1018F), (0x1019C, 0x1019F), (0x101A1, 0x101CF), (0x101FE, 0x1027F),
    (0x1029D, 0x1029F), (0x102D1, 0x102DF), (0x102FC, 0x102FF), (0x10324, 0x1032C),
    (0x1034B, 0x1034F), (0x1037B, 0x1037F), (0x1039E, 0x1039E), (0x103C4, 0x103C7),
    (0x103D6, 0x103FF), (0x1049E, 0x1049F), (0x104AA, 0x104AF), (0x104D4, 0x104D7),
    (0x104FC, 0x104FF), (0x10528, 0x1052F), (0x10564, 0x1056E), (0x10570, 0x105FF),
    (0x10737, 0x1073F), (0x10756, 0x1075F), (0x10768, 0x107FF), (0x10806, 0x10807),
    (0x10809, 0x10809), (0x10836, 0x10836), (0x10839, 0x1083B), (0x1083D, 0x1083E),
    (0x10856, 0x10856), (0x1089F, 0x108A6), (0x108B0, 0x108DF), (0x108F3, 0x108F3),
    (0x108F6, 0x108FA), (0x1091C, 0x1091E), (0x1093A, 0x1093E), (0x10940, 0x1097F),
    (0x109B8, 0x109BB), (0x109D0, 0x109D1), (0x10A04, 0x10A04), (0x10A07, 0x10A0B),
    (0x10A14, 0x10A14), (0x10A18, 0x10A18), (0x10A34, 0x10A37), (0x10A3B, 0x10A3E),
    (0x10A48, 0x10A4F), (0x10A59, 0x10A5F), (0x10AA0, 0x10ABF), (0x10AE7, 0x10AEA),
    (0x10AF7, 0x10AFF), (0x10B36, 0x10B38), (0x10B56, 0x10B57), (0x10B73, 0x10B77),
    (0x10B92, 0x10B98), (0x10B9D, 0x10BA8), (0x10BB0, 0x10BFF), (0x10C49, 0x10C7F),
    (0x10CB3, 0x10CBF), (0x10CF3, 0x10CF9), (0x10D00, 0x10E5F), (0x10E7F, 0x10FFF),
    (0x1104E, 0x11051), (0x11070, 0x1107E), (0x110C2, 0x110CF), (0x110E9, 0x110EF),
    (0x110FA, 0x110FF), (0x11135, 0x11135), (0x11144, 0x1114F), (0x11177, 0x1117F),
    (0x111CE, 0x111CF), (0x111E0, 0x111E0), (0x111F5, 0x111FF), (0x11212, 0x11212),
    (0x1123F, 0x1127F), (0x11287, 0x11287), (0x11289, 0x11289), (0x1128E, 0x1128E),
    (0x1129E, 0x1129E), (0x112AA, 0x112AF), (0x112EB, 0x112EF), (0x112FA, 0x112FF),
    (0x11304, 0x11304), (0x1130D, 0x1130E), (0x11311, 0x11312), (0x11329, 0x11329),
    (0x11331, 0x11331), (0x11334, 0x11334), (0x1133A, 0x1133B), (0x11345, 0x11346),
    (0x11349, 0x1134A), (0x1134E, 0x1134F), (0x11351, 0x11356), (0x11358, 0x1135C),
    (0x11364, 0x11365), (0x1136D, 0x1136F), (0x11375, 0x113FF), (0x1145A, 0x1145A),
    (0x1145C, 0x1145C), (0x1145E, 0x1147F), (0x114C8, 0x114CF), (0x114DA, 0x1157F),
    (0x115B6, 0x115B7), (0x115DE, 0x115FF), (0x11645, 0x1164F), (0x1165A, 0x1165F),
    (0x1166D, 0x1167F), (0x116B8, 0x116BF), (0x116CA, 0x116FF), (0x1171A, 0x1171C),
    (0x1172C, 0x1172F), (0x11740, 0x1189F), (0x118F3, 0x118FE), (0x11900, 0x119FF),
    (0x11A48, 0x11A4F), (0x11A84, 0x11A85), (0x11A9D, 0x11A9D), (0x11AA3, 0x11ABF),
    (0x11AF9, 0x11BFF), (0x11C09, 0x11C09), (0x11C37, 0x11C37), (0x11C46, 0x11C4F),
    (0x11C6D, 0x11C6F), (0x11C90, 0x11C91), (0x11CA8, 0x11CA8), (0x11CB7, 0x11CFF),
    (0x11D07, 0x11D07), (0x11D0A, 0x11D0A), (0x11D37, 0x11D39), (0x11D3B, 0x11D3B),
    (0x11D3E, 0x11D3E), (0x11D48, 0x11D4F), (0x11D5A, 0x11FFF), (0x1239A, 0x123FF),
    (0x1246F, 0x1246F), (0x12475, 0x1247F), (0x12544, 0x12FFF), (0x1342F, 0x143FF),
    (0x14647, 0x167FF), (0x16A39, 0x16A3F), (0x16A5F, 0x16A5F), (0x16A6A, 0x16A6D),
    (0x16A70, 0x16ACF), (0x16AEE, 0x16AEF), (0x16AF6, 0x16AFF), (0x16B46, 0x16B4F),
    (0x16B5A, 0x16B5A), (0x16B62, 0x16B62), (0x16B78, 0x16B7C), (0x16B90, 0x16EFF),
    (0x16F45, 0x16F4F), (0x16F7F, 0x16F8E), (0x16FA0, 0x16FDF), (0x16FE2, 0x16FFF),
    (0x187ED, 0x187FF), (0x18AF3, 0x1AFFF), (0x1B11F, 0x1B16F), (0x1B2FC, 0x1BBFF),
    (0x1BC6B, 0x1BC6F), (0x1BC7D, 0x1BC7F), (0x1BC89, 0x1BC8F), (0x1BC9A, 0x1BC9B),
    (0x1BCA4, 0x1CFFF), (0x1D0F6, 0x1D0FF), (0x1D127, 0x1D128), (0x1D1E9, 0x1D1FF),
    (0x1D246, 0x1D2FF), (0x1D357, 0x1D35F), (0x1D372, 0x1D3FF), (0x1D455, 0x1D455),
    (0x1D49D, 0x1D49D), (0x1D4A0, 0x1D4A1), (0x1D4A3, 0x1D4A4), (0x1D4A7, 0x1D4A8),
    (0x1D4AD, 0x1D4AD), (0x1D4BA, 0x1D4BA), (0x1D4BC, 0x1D4BC), (0x1D4C4, 0x1D4C4),
    (0x1D506, 0x1D506), (0x1D50B, 0x1D50C), (0x1D515, 0x1D515), (0x1D51D, 0x1D51D),
    (0x1D53A, 0x1D53A), (0x1D53F, 0x1D53F), (0x1D545, 0x1D545), (0x1D547, 0x1D549),
    (0x1D551, 0x1D551), (0x1D6A6, 0x1D6A7), (0x1D7CC, 0x1D7CD), (0x1DA8C, 0x1DA9A),
    (0x1DAA0, 0x1DAA0), (0x1DAB0, 0x1DFFF), (0x1E007, 0x1E007), (0x1E019, 0x1E01A),
    (0x1E022, 0x1E022), (0x1E025, 0x1E025), (0x1E02B, 0x1E7FF), (0x1E8C5, 0x1E8C6),
    (0x1E8D7, 0x1E8FF), (0x1E94B, 0x1E94F), (0x1E95A, 0x1E95D), (0x1E960, 0x1EDFF),
    (0x1EE04, 0x1EE04), (0x1EE20, 0x1EE20), (0x1EE23, 0x1EE23), (0x1EE25, 0x1EE26),
    (0x1EE28, 0x1EE28), (0x1EE33, 0x1EE33), (0x1EE38, 0x1EE38), (0x1EE3A, 0x1EE3A),
    (0x1EE3C, 0x1EE41), (0x1EE43, 0x1EE46), (0x1EE48, 0x1EE48), (0x1EE4A, 0x1EE4A),
    (0x1EE4C, 0x1EE4C), (0x1EE50, 0x1EE50), (0x1EE53, 0x1EE53), (0x1EE55, 0x1EE56),
    (0x1EE58, 0x1EE58), (0x1EE5A, 0x1EE5A), (0x1EE5C, 0x1EE5C), (0x1EE5E, 0x1EE5E),
    (0x1EE60, 0x1EE60), (0x1EE63, 0x1EE63), (0x1EE65, 0x1EE66), (0x1EE6B, 0x1EE6B),
    (0x1EE73, 0x1EE73), (0x1EE78, 0x1EE78), (0x1EE7D, 0x1EE7D), (0x1EE7F, 0x1EE7F),
    (0x1EE8A, 0x1EE8A), (0x1EE9C, 0x1EEA0), (0x1EEA4, 0x1EEA4), (0x1EEAA, 0x1EEAA),
    (0x1EEBC, 0x1EEEF), (0x1EEF2, 0x1EFFF), (0x1F02C, 0x1F02F), (0x1F094, 0x1F09F),
    (0x1F0AF, 0x1F0B0), (0x1F0C0, 0x1F0C0), (0x1F0D0, 0x1F0D0), (0x1F0F6, 0x1F0FF),
    (0x1F10D, 0x1F10F), (0x1F12F, 0x1F12F), (0x1F16C, 0x1F16F), (0x1F1AD, 0x1F1E5),
    (0x1F203, 0x1F20F), (0x1F23C, 0x1F23F), (0x1F249, 0x1F24F), (0x1F252, 0x1F25F),
    (0x1F266, 0x1F2FF), (0x1F6D5, 0x1F6DF), (0x1F6ED, 0x1F6EF), (0x1F6F9, 0x1F6FF),
    (0x1F774, 0x1F77F), (0x1F7D5, 0x1F7FF), (0x1F80C, 0x1F80F), (0x1F848, 0x1F84F),
    (0x1F85A, 0x1F85F), (0x1F888, 0x1F88F), (0x1F8AE, 0x1F8FF), (0x1F90C, 0x1F90F),
    (0x1F93F, 0x1F93F), (0x1F94D, 0x1F94F), (0x1F96C, 0x1F97F), (0x1F998, 0x1F9BF),
    (0x1F9C1, 0x1F9CF), (0x1F9E7, 0x1FFFF), (0x2A6D7, 0x2A6FF), (0x2B735, 0x2B73F),
    (0x2B81E, 0x2B81F), (0x2CEA2, 0x2CEAF), (0x2EBE1, 0x2F7FF), (0x2FA1E, 0xE0000),
    (0xE0002, 0xE001F), (0xE0080, 0xE00FF), (0xE01F0, 0xEFFFF), (0xFFFFE, 0xFFFFF),
    (0x10FFFE, 0x10FFFF),
)
DIR_FLAGS = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW
FILE_FLAGS = os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK


class Refusal(Exception):
    pass


def require(ok, code):
    if not ok:
        raise Refusal(code)


def signature(s):
    return [s.st_dev, s.st_ino, s.st_mode, s.st_uid, s.st_nlink,
            s.st_size, s.st_mtime_ns, s.st_ctime_ns]


def owned(s, directory=False):
    require(s.st_uid == os.getuid(), 'ownership')
    require(stat.S_ISDIR(s.st_mode) if directory else stat.S_ISREG(s.st_mode), 'entry-kind')
    if not directory:
        require(s.st_nlink == 1, 'hardlink')


def absolute(value):
    p = Path(value)
    if not p.is_absolute():
        p = Path.cwd() / p
    require('..' not in p.parts and not any(x in BLOCKED or x.startswith('similarity')
                                         for x in p.parts), 'path-scope')
    return p


def open_dir(path, owned_from=None):
    """Open every ancestor with nofollow; never resolve a user path through links."""
    path = absolute(path)
    fd = os.open('/', DIR_FLAGS)
    walked = Path('/')
    try:
        for part in path.parts[1:]:
            nxt = os.open(part, DIR_FLAGS, dir_fd=fd)
            os.close(fd)
            fd = nxt
            walked /= part
            if owned_from is not None and walked.is_relative_to(owned_from):
                owned(os.fstat(fd), True)
        return fd
    except BaseException:
        os.close(fd)
        raise


def source_path(root, value):
    p = absolute(value)
    require(p.is_relative_to(root / 'build') and p != root / 'build', 'path-scope')
    return p


def read_file(parent, name, destination=None, limit=SOURCE_LIMIT):
    """Stream file under pinned directory; detect replacement and in-place writes."""
    before = os.stat(name, dir_fd=parent, follow_symlinks=False)
    owned(before)
    require(0 <= before.st_size <= limit, 'source-size-scope')
    fd = os.open(name, FILE_FLAGS, dir_fd=parent)
    try:
        require(signature(os.fstat(fd)) == signature(before), 'source-changed')
        sha1, sha256, length = hashlib.sha1(), hashlib.sha256(), 0
        while length < before.st_size:
            data = os.read(fd, min(CHUNK, before.st_size - length))
            require(data, 'source-changed')
            sha1.update(data)
            sha256.update(data)
            length += len(data)
            if destination is not None:
                destination.write(data)
        require(not os.read(fd, 1), 'source-changed')
        require(length == before.st_size and signature(os.fstat(fd)) == signature(before)
                and signature(os.stat(name, dir_fd=parent, follow_symlinks=False))
                == signature(before), 'source-changed')
        return {'size': length, 'sha1': sha1.hexdigest()}, sha256.hexdigest(), signature(before)
    finally:
        os.close(fd)


def scan_tree(fd, prefix='', digest=False, budget=None, depth=0):
    """Snapshot every entry; flat media refuses directories as well as links."""
    require(depth <= 6, 'tree-depth-scope')
    budget = [0] if budget is None else budget
    initial = signature(os.fstat(fd))
    owned(os.fstat(fd), True)
    result = {'': initial}
    names = entry_names(fd)
    for name in names:
        budget[0] += 1
        require(budget[0] <= ENTRY_LIMIT, 'entry-count-scope')
        require(len(name.encode('utf-8')) <= 256, 'entry-name-scope')
        require(name not in BLOCKED and not name.startswith('similarity'), 'path-scope')
        name.encode('utf-8', errors='strict')
        s = os.stat(name, dir_fd=fd, follow_symlinks=False)
        key = prefix + name
        if stat.S_ISDIR(s.st_mode):
            owned(s, True)
            require(prefix != 'collection.media/', 'media-entry-kind')
            child = os.open(name, DIR_FLAGS, dir_fd=fd)
            try:
                require(signature(os.fstat(child)) == signature(s), 'source-changed')
                subtree = scan_tree(child, key + '/', digest, budget, depth + 1)
                result[key] = subtree.pop('')
                result.update(subtree)
            finally:
                os.close(child)
        else:
            owned(s)
            result[key] = signature(s)
            if digest:
                _, _, sig = read_file(fd, name)
                require(sig == signature(s), 'source-changed')
    require(names == entry_names(fd) and signature(os.fstat(fd)) == initial,
            'source-changed')
    return result


def clock(now, tz):
    if tz == NATIVE_TZ:
        require(now == NATIVE_START, 'clock-scope')
        return now, NATIVE_NOW
    require(tz == 'UTC', 'clock-scope')
    require(re.fullmatch(r'20\d\d-\d\d-\d\dT\d\d:\d\d:\d\d(?:Z|\+00:00)?', now)
            is not None, 'clock-scope')
    start = now.removesuffix('Z').removesuffix('+00:00')
    instant = dt.datetime.fromisoformat(start).replace(tzinfo=dt.timezone.utc)
    require(2000 <= instant.year <= 2037, 'clock-scope')
    return start, int(instant.timestamp())


def clock_offset(now, tz):
    # Reviewed fixture instant only; never infer an offset from ambient TZ or
    # assume this fixed January offset applies to another Paris instant.
    require(tz == 'UTC' or (tz == NATIVE_TZ and exact(now, NATIVE_NOW)), 'clock-scope')
    return 0 if tz == 'UTC' else 3600


def scenario(start, tz='UTC'):
    clock(start, tz)
    return {'id': ID, 'clock': {'tz': tz, 'start': start, 'mode': 'frozen'},
            'steps': [{'open': {'fixture': 'collection.anki2'}},
                      {'observe': [{'db_dump': {'raw': True}}, 'timing']},
                      {'import': {'format': 'colpkg', 'fixture': 'package.colpkg'}},
                      {'observe': [{'db_dump': {'raw': True}}, 'timing']},
                      {'observe': ['media_dir']}]}


def canonical(value):
    # Our scenario contains only ASCII keys/strings and booleans, no numbers;
    # sorted compact JSON is exactly its RFC 8785 serialization (D3).
    return json.dumps(value, sort_keys=True, separators=(',', ':'), ensure_ascii=False)


def entry_names(fd):
    names = []
    with os.scandir(fd) as entries:
        for entry in entries:
            require(len(names) < ENTRY_LIMIT, 'entry-count-scope')
            names.append(entry.name)
    return sorted(names)


def json_bytes(data):
    """Preflight depth/tokens before the stdlib parser allocates objects."""
    require(len(data) <= JSON_LIMIT, 'json-size-scope')
    depth = tokens = 0
    quoted = escaped = False
    for c in data:
        if quoted:
            if escaped:
                escaped = False
            elif c == 92:
                escaped = True
            elif c == 34:
                quoted = False
            continue
        if c == 34:
            quoted = True
            tokens += 1
        elif c in (123, 91):
            depth += 1
            tokens += 1
            require(depth <= JSON_DEPTH, 'json-depth-scope')
        elif c in (125, 93):
            depth -= 1
        elif c in (44, 58):
            tokens += 1
        require(tokens <= JSON_TOKENS, 'json-token-scope')
    def pairs(items):
        obj = {}
        for key, value in items:
            require(key not in obj, 'duplicate-json-key')
            obj[key] = value
        return obj
    return json.loads(data, object_pairs_hook=pairs,
                      parse_constant=lambda _: require(False, 'nonfinite-json'))


def write_json(fd, name, value, mode=0o600):
    # All producers are bounded; also stop encoding at the byte limit.
    encoded = bytearray()
    for piece in json.JSONEncoder(sort_keys=True, ensure_ascii=True).iterencode(value):
        piece = piece.encode('ascii')
        require(len(encoded) + len(piece) + 1 <= JSON_LIMIT, 'json-size-scope')
        encoded.extend(piece)
    encoded.extend(b'\n')
    json_bytes(encoded)
    out = os.open(name, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW,
                  mode, dir_fd=fd)
    with os.fdopen(out, 'wb') as stream:
        stream.write(encoded)


def load_json(fd, name, with_hash=False):
    f = os.open(name, FILE_FLAGS, dir_fd=fd)
    try:
        initial = os.fstat(f)
        owned(initial)
        require(initial.st_size <= JSON_LIMIT, 'json-size-scope')
        data = bytearray()
        while len(data) < initial.st_size:
            part = os.read(f, min(CHUNK, initial.st_size - len(data)))
            require(part, 'output-changed')
            data.extend(part)
        require(not os.read(f, 1), 'output-changed')
        require(signature(os.fstat(f)) == signature(initial)
                and signature(os.stat(name, dir_fd=fd, follow_symlinks=False)) == signature(initial),
                'output-changed')
        value = json_bytes(data)
        return (value, hashlib.sha256(data).hexdigest()) if with_hash else value
    finally:
        os.close(f)


def assigned10(c):
    cp = ord(c)
    i = bisect.bisect_right(UNASSIGNED10, (cp, 0x110000)) - 1
    return i < 0 or cp > UNASSIGNED10[i][1]


def safe_media_name(name):
    require(unicodedata.normalize('NFC', name) == name, 'unsafe-media-name')
    require(name and len(name.encode('utf-8')) <= 120 and
            not any(c in '[]<>:"/?*^\\|' or ord(c) < 32 or ord(c) == 127
                    or c == '\u00a0' for c in name) and
            not name.endswith((' ', '.')) and
            re.match(r'(?i)^(con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\.|$)', name,
                     flags=re.ASCII) is None, 'unsafe-media-name')
    require(all(assigned10(c) for c in name), 'unsafe-media-name')


def media_inventory(fd, apple):
    result = {}
    for name in entry_names(fd):
        # Validate even ignored entries: M8 is no escape for links/nonregulars.
        owned(os.stat(name, dir_fd=fd, follow_symlinks=False))
        folded = name.translate(str.maketrans('ABCDEFGHIJKLMNOPQRSTUVWXYZſ',
                                             'abcdefghijklmnopqrstuvwxyzs'))
        if re.fullmatch(r'(?::?thumbs.db|.ds_store)', folded, flags=re.DOTALL):
            continue
        nfc = unicodedata.normalize('NFC', name)
        require(apple or nfc == name, 'non-apple-non-nfc')
        require(nfc not in result, 'media-nfc-collision')
        safe_media_name(nfc)
        result[nfc] = read_file(fd, name)[0]
    return dict(sorted(result.items(), key=lambda item: item[0].encode('utf-8')))


def sources(root, package, expected):
    package, expected = source_path(root, package), source_path(root, expected)
    require(package.suffix == '.colpkg', 'package-suffix')
    require(not package.is_relative_to(expected), 'source-overlap')
    parent = open_dir(package.parent, root)
    exp = open_dir(expected, root)
    try:
        tree = scan_tree(exp)
        require('collection.anki2' in tree and 'collection.media' in tree, 'expected-layout')
        require(stat.S_ISREG(tree['collection.anki2'][2])
                and stat.S_ISDIR(tree['collection.media'][2]), 'expected-layout')
        require(not any(x.startswith('collection.anki2-') and
                        x in {'collection.anki2-wal', 'collection.anki2-shm', 'collection.anki2-journal'}
                        for x in tree), 'sqlite-sidecar')
        owned(os.stat(package.name, dir_fd=parent, follow_symlinks=False))
        return parent, exp, package, expected, tree
    except BaseException:
        os.close(parent)
        os.close(exp)
        raise


class RawCursor:
    def __init__(self, reader, cursor):
        self.reader, self.cursor = reader, cursor

    def fetchone(self):
        self.reader.check()
        try:
            return self.cursor.fetchone()
        finally:
            self.reader.check()

    def fetchall(self):
        return list(self)

    def __iter__(self):
        return self

    def __next__(self):
        row = self.fetchone()
        if row is None:
            raise StopIteration
        return row


class RawConnection:
    """Raw-cell access only; a collation attempt permanently poisons this handle."""
    def __init__(self, connection):
        self.connection = connection
        self.collation_invoked = False
        connection.create_collation('unicase', self.refuse_collation)

    def refuse_collation(self, left, right):
        # NEVER return a comparison, even for ASCII or identical strings. This
        # registration permits schema preparation, not semantic index access.
        self.collation_invoked = True
        raise Refusal('sqlite-collation-required')

    def check(self):
        require(not self.collation_invoked, 'sqlite-collation-required')

    def execute(self, sql, parameters=()):
        self.check()
        try:
            return RawCursor(self, self.connection.execute(sql, parameters))
        finally:
            # Also refuses if SQLite catches the callback exception internally.
            self.check()

    def close(self):
        try:
            self.connection.close()
        finally:
            self.check()

    def getlimit(self, category):
        return self.connection.getlimit(category)

    def set_trace_callback(self, callback):
        self.connection.set_trace_callback(callback)


def sqlite_reader(path):
    owned(path.lstat())
    require(0 < path.stat().st_size <= DB_LIMIT, 'sqlite-size-scope')
    conn = sqlite3.connect(path.as_uri() + '?mode=ro&immutable=1', uri=True)
    # SQLite itself refuses oversized row payloads before loading them, including
    # during CAST/length preflight. No client/library collection open occurs.
    conn.setlimit(sqlite3.SQLITE_LIMIT_LENGTH, CELL_LIMIT)
    conn.setlimit(sqlite3.SQLITE_LIMIT_SQL_LENGTH, 16384)
    reader = RawConnection(conn)
    try:
        reader.execute('PRAGMA query_only=ON')
        reader.execute('PRAGMA cache_size=-2048')
        reader.execute('PRAGMA temp_store=MEMORY')
        return reader
    except BaseException:
        reader.close()
        raise


def raw_rows(path):
    """One bounded row at a time; key-only sorts never contain cell payloads."""
    initial = signature(path.lstat())
    conn = sqlite_reader(path)
    total = 0
    try:
        for table, declaration in COLUMNS.items():
            columns = declaration.split()
            actual = [r[1] for r in conn.execute('PRAGMA table_info("' + table + '")')]
            require(len(actual) == len(columns) and set(actual) == set(columns), 'sqlite-columns')
            count = conn.execute('SELECT count(*) FROM "' + table + '" NOT INDEXED').fetchone()[0]
            total += count
            require(total <= ROW_LIMIT, 'sqlite-row-scope')
            keys = KEYS.get(table, ('id',))
            # Only the small keys enter the sorting buffer. The SQLite length
            # limit protects internal decoding; these checks precede Python fetch.
            for col in columns:
                quoted = '"' + col + '"'
                limit = 256 if col in keys else CELL_LIMIT
                oversized = conn.execute('SELECT 1 FROM "' + table + '" NOT INDEXED WHERE '
                    'length(CAST(' + quoted + ' AS BLOB)) > ? LIMIT 1', (limit,)).fetchone()
                require(oversized is None, 'sqlite-cell-scope')
            key_sql = ','.join('"' + k + '" COLLATE BINARY' for k in keys)
            where = ' AND '.join('"' + k + '" COLLATE BINARY IS ?' for k in keys)
            selection = ','.join('typeof("' + c + '"),"' + c + '"' for c in columns)
            last_key = None
            for key_values in conn.execute('SELECT ' + key_sql + ' FROM "' + table +
                                           '" NOT INDEXED ORDER BY ' + key_sql):
                require(all(type(v) is (str if k in ('KEY', 'tag') else int)
                            for k, v in zip(keys, key_values)), 'sqlite-key-scope')
                key = ','.join(str(v) for v in key_values)
                require(key != last_key, 'sqlite-duplicate-key')
                last_key = key
                cursor = conn.execute('SELECT ' + selection + ' FROM "' + table +
                                      '" NOT INDEXED WHERE ' + where + ' LIMIT 2', key_values)
                row = cursor.fetchone()
                require(row is not None and cursor.fetchone() is None, 'sqlite-duplicate-key')
                values = []
                for i in range(0, len(row), 2):
                    kind, value = row[i:i + 2]
                    require(kind in ('null', 'integer', 'real', 'text', 'blob'), 'sqlite-type')
                    require(kind != 'real' or math.isfinite(value), 'sqlite-nonfinite')
                    values.append((kind, value))
                yield table, key, values
    except sqlite3.DataError as error:
        raise Refusal('sqlite-cell-or-row-scope') from error
    finally:
        try:
            conn.close()
        finally:
            require(signature(path.lstat()) == initial, 'staged-input-changed')


def compare_databases(expected, decoded):
    """Require identical raw cells, storage classes, BLOB bytes and stamps."""
    left, right = raw_rows(expected), raw_rows(decoded)
    sentinel = object()
    try:
        while True:
            a, b = next(left, sentinel), next(right, sentinel)
            require((a is sentinel) == (b is sentinel), 'pre-client-database-difference')
            if a is sentinel:
                break
            require(exact(a[0], b[0]) and exact(a[1], b[1]) and
                    len(a[2]) == len(b[2]) and all(ka == kb and exact(va, vb)
                    for (ka, va), (kb, vb) in zip(a[2], b[2])), 'pre-client-database-difference')
    finally:
        left.close()
        right.close()


def bind_dump(expected, dump, import_ms=None):
    # check_dump validates both bounded inputs first. Expected cells come from
    # the independent raw DB proof; only import's declared col stamps differ.
    for table, rows in expected.items():
        require(rows.keys() == dump[table].keys(), 'database-difference')
        for key, values in rows.items():
            for col, value in values.items():
                if import_ms is not None and table == 'col' and col in ('mod', 'scm'):
                    value = import_ms
                require(exact(value, dump[table][key][col]), 'database-difference')


def write_expected(dest, path):
    """Decoder emits O8 raw rows within the byte budget before loading JSON."""
    out = os.open('expected.json', os.O_WRONLY | os.O_CREAT | os.O_EXCL |
                  os.O_NOFOLLOW, 0o400, dir_fd=dest)
    encoder = json.JSONEncoder(ensure_ascii=True)
    with os.fdopen(out, 'wb') as stream:
        length = 0
        def emit(value, literal=False):
            nonlocal length
            for piece in (value,) if literal else encoder.iterencode(value):
                data = piece.encode('ascii')
                require(length + len(data) <= JSON_LIMIT, 'json-size-scope')
                stream.write(data)
                length += len(data)
        emit('{', True)
        rows = raw_rows(path)
        pending = next(rows, None)
        try:
            for index, table in enumerate(COLUMNS):
                if index:
                    emit(',', True)
                emit(table)
                emit(':{', True)
                first = True
                while pending is not None and pending[0] == table:
                    _, key, values = pending
                    row = {col: {'sha256': hashlib.sha256(value).hexdigest()}
                           if kind == 'blob' else value
                           for col, (kind, value) in zip(COLUMNS[table].split(), values)}
                    if not first:
                        emit(',', True)
                    emit(key)
                    emit(':', True)
                    emit(row)
                    first = False
                    pending = next(rows, None)
                emit('}', True)
            require(pending is None, 'sqlite-columns')
            emit('}\n', True)
        finally:
            rows.close()
    # Preflight token/depth limits before allocating the bounded neutral object.
    check_dump(load_json(dest, 'expected.json'))


def zip_preflight(stream):
    """Reject unsupported finite fixture layout BEFORE ZipFile allocates its CD."""
    size = os.fstat(stream.fileno()).st_size
    require(22 <= size <= SOURCE_LIMIT, 'archive-size-scope')
    stream.seek(size - 22)
    end = stream.read(22)
    require(len(end) == 22 and end[:4] == b'PK\x05\x06', 'archive-layout-scope')
    _, disk, cd_disk, disk_count, count, cd_size, offset, comment = struct.unpack('<4s4H2IH', end)
    require(disk == cd_disk == comment == 0 and disk_count == count and
            0 < count <= ENTRY_LIMIT and cd_size <= 4 * 1024 * 1024 and
            offset + cd_size == size - 22 and offset != 0xffffffff,
            'archive-directory-scope')
    stream.seek(offset)
    consumed = 0
    for _ in range(count):
        fixed = stream.read(46)
        require(len(fixed) == 46 and fixed[:4] == b'PK\x01\x02', 'archive-directory')
        name, extra, note = struct.unpack_from('<HHH', fixed, 28)
        require(0 < name <= 256 and extra <= 256 and note == 0, 'archive-metadata-scope')
        consumed += 46 + name + extra + note
        require(consumed <= cd_size, 'archive-directory')
        require(len(stream.read(name + extra + note)) == name + extra + note, 'archive-directory')
    require(consumed == cd_size, 'archive-directory')
    stream.seek(0)
    require(stream.read(4) == b'PK\x03\x04', 'archive-layout-scope')
    stream.seek(0)


def proto_fields(data, depth=0):
    """P2 wire traversal; views avoid copying unknown length-delimited fields."""
    data = memoryview(data)
    pos, groups = 0, []
    def varint():
        nonlocal pos
        value = 0
        for index in range(10):
            require(pos < len(data), 'media-map-protobuf')
            byte = data[pos]
            pos += 1
            require(index < 9 or byte <= 1, 'media-map-protobuf')
            value |= (byte & 127) << (7 * index)
            if byte < 128:
                return value
        raise Refusal('media-map-protobuf')
    while pos < len(data):
        key = varint()
        number, wire = key >> 3, key & 7
        require(0 < number <= 536870911 and key <= 0xffffffff and wire < 6,
                'media-map-protobuf')
        if wire == 4:
            require(groups and groups.pop() == number, 'media-map-protobuf')
            continue
        require(depth + len(groups) < 100, 'media-map-protobuf')
        if wire == 0:
            value = varint()
        elif wire in (1, 2, 5):
            length = varint() if wire == 2 else (8 if wire == 1 else 4)
            require(length <= len(data) - pos, 'media-map-protobuf')
            value = data[pos:pos + length]
            pos += length
        else:
            value = None
        if not groups:
            yield number, wire, value
        if wire == 3:
            groups.append(number)
    require(not groups, 'media-map-protobuf')


def media_records(data):
    records, previous = [], b''
    for number, wire, value in proto_fields(data):
        if number != 1:
            continue
        require(wire == 2 and len(records) < ENTRY_LIMIT - 4, 'media-map-record')
        name, length, digest = '', 0, b''
        for field, kind, item in proto_fields(value, 1):
            require(field != 255, 'media-map-archive-index')
            if field in (1, 2, 3):
                require(kind == (0 if field == 2 else 2), 'media-map-record')
                if field == 1:
                    name = bytes(item).decode('utf-8', errors='strict')
                elif field == 2:
                    # Native output never wraps a source length through uint32.
                    require(item <= 0xffffffff, 'media-map-length')
                    length = item
                else:
                    digest = bytes(item)
        safe_media_name(name)
        require(len(digest) == 20, 'media-map-digest')
        encoded = name.encode('utf-8')
        require(encoded > previous, 'media-map-order-or-duplicate')
        require(length <= SOURCE_LIMIT, 'media-payload-scope')
        records.append((name, {'size': length, 'sha1': digest.hex()}))
        previous = encoded
    return records


def native_frame_size(header):
    """RFC 8878 header facts; level is not encoded in a frame."""
    require(len(header) >= 6 and header[:4] == b'\x28\xb5\x2f\xfd',
            'zstd-native-frame')
    descriptor = header[4]
    require(descriptor & 4 and not descriptor & 0x1b, 'zstd-native-settings')
    single = bool(descriptor & 32)
    flag = descriptor >> 6
    length = (1 if single else 0) if flag == 0 else (2, 4, 8)[flag - 1]
    require(length > 0, 'zstd-native-content-size')
    offset = 5 + (not single)
    require(len(header) >= offset + length, 'zstd-native-frame')
    return int.from_bytes(header[offset:offset + length], 'little') + (256 if length == 2 else 0)


class PayloadDigest:
    """Bounded sink: no filename is used as a path and no payload is retained."""
    def __init__(self):
        self.length = 0
        self.digest = hashlib.sha1()

    def write(self, data):
        self.length += len(data)
        self.digest.update(data)

    def inventory(self):
        return {'size': self.length, 'sha1': self.digest.hexdigest()}


def check_zip_records(stream, archive, members):
    """Require contiguous local records matching every central-directory entry."""
    offset = 0
    for info in members:
        require(info.header_offset == offset, 'archive-local-layout')
        stream.seek(offset)
        header = stream.read(30)
        require(len(header) == 30 and header[:4] == b'PK\x03\x04', 'archive-local-layout')
        flags, method = struct.unpack_from('<HH', header, 6)
        crc, compressed, size, names, extra = struct.unpack_from('<IIIHH', header, 14)
        require(flags == info.flag_bits and not flags & ~0x800 and
                method == info.compress_type and crc == info.CRC and
                compressed == info.compress_size and size == info.file_size and
                names == len(info.filename) and extra <= 256, 'archive-local-layout')
        require(stream.read(names) == info.filename.encode('ascii'), 'archive-local-layout')
        offset += 30 + names + extra + compressed
    require(offset == archive.start_dir, 'archive-local-layout')


def decode_zstd(source, destination, encoded_size, *, native=False, decoded_limit=None):
    """libzstd streaming API with caller-owned fixed input/output buffers.

    A zero return marks a complete frame (checksum checked by the library).
    Continue across frames; EOF with nonzero return is truncation. Garbage is
    fed to the decoder, never treated as an acceptable suffix.
    """
    lib = ctypes.CDLL('libzstd.so.1')
    class Buffer(ctypes.Structure):
        _fields_ = [('data', ctypes.c_void_p), ('size', ctypes.c_size_t),
                    ('pos', ctypes.c_size_t)]
    lib.ZSTD_createDStream.restype = ctypes.c_void_p
    lib.ZSTD_freeDStream.argtypes = [ctypes.c_void_p]
    lib.ZSTD_initDStream.argtypes = [ctypes.c_void_p]
    lib.ZSTD_initDStream.restype = ctypes.c_size_t
    lib.ZSTD_isError.argtypes = [ctypes.c_size_t]
    lib.ZSTD_isError.restype = ctypes.c_uint
    lib.ZSTD_DCtx_setParameter.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_int]
    lib.ZSTD_DCtx_setParameter.restype = ctypes.c_size_t
    lib.ZSTD_decompressStream.argtypes = [ctypes.c_void_p, ctypes.POINTER(Buffer),
                                         ctypes.POINTER(Buffer)]
    lib.ZSTD_decompressStream.restype = ctypes.c_size_t
    ctx = lib.ZSTD_createDStream()
    require(ctx, 'zstd-context')
    # ZSTD_d_windowLogMax (public zstd.h enum) = 100; P8's 2^27 window.
    initialized = lib.ZSTD_initDStream(ctx)
    result = lib.ZSTD_DCtx_setParameter(ctx, 100, 27)
    output = ctypes.create_string_buffer(64 * 1024)
    limit = min(DB_LIMIT if decoded_limit is None else decoded_limit,
                max(16 * 1024 * 1024, 1000 * encoded_size))
    declared = None
    consumed = written = 0
    remaining = 1
    try:
        require(not lib.ZSTD_isError(initialized) and not lib.ZSTD_isError(result),
                'zstd-window-limit')
        while consumed < encoded_size:
            chunk = source.read(min(64 * 1024, encoded_size - consumed))
            require(chunk, 'zstd-truncated')
            if native and consumed == 0:
                declared = native_frame_size(chunk)
                require(declared <= limit, 'zstd-decoded-scope')
            consumed += len(chunk)
            data = ctypes.create_string_buffer(chunk)
            incoming = Buffer(ctypes.cast(data, ctypes.c_void_p), len(chunk), 0)
            while True:
                # One extra output byte detects expansion before it is written.
                capacity = min(len(output), limit - written + 1)
                outgoing = Buffer(ctypes.cast(output, ctypes.c_void_p), capacity, 0)
                old_pos = incoming.pos
                remaining = lib.ZSTD_decompressStream(ctx, ctypes.byref(outgoing),
                                                     ctypes.byref(incoming))
                require(not lib.ZSTD_isError(remaining), 'zstd-invalid-stream')
                require(written + outgoing.pos <= limit, 'zstd-decoded-scope')
                destination.write(output.raw[:outgoing.pos])
                written += outgoing.pos
                if native and remaining == 0:
                    require(incoming.pos == incoming.size and consumed == encoded_size,
                            'zstd-native-single-frame')
                if incoming.pos == incoming.size and (remaining == 0 or outgoing.pos < capacity):
                    break
                require(incoming.pos != old_pos or outgoing.pos, 'zstd-no-progress')
        require(remaining == 0 and not source.read(1), 'zstd-incomplete-or-extra')
        require(not native or declared == written, 'zstd-native-content-size')
        return written
    finally:
        lib.ZSTD_freeDStream(ctx)


def decode(args):
    """Only invoked in the isolated no-network generated-input service."""
    start, now = clock(args.now, args.tz)
    offset = clock_offset(now, args.tz)
    package, expected, output = map(absolute, (args.package, args.expected, args.output))
    inventory_path = absolute(args.media_inventory)
    parent, dest, exp = open_dir(package.parent), open_dir(output), open_dir(expected.parent)
    inventory_parent = open_dir(inventory_path.parent)
    try:
        require(not entry_names(dest), 'fresh-decoder-output-required')
        expected_media, inventory_hash = load_json(inventory_parent, inventory_path.name, with_hash=True)
        require(type(expected_media) is dict, 'media-inventory-shape')
        _, expected_hash, expected_sig = read_file(exp, expected.name, limit=DB_LIMIT)
        _, package_hash, package_sig = read_file(parent, package.name)
        with os.fdopen(os.open(package.name, FILE_FLAGS, dir_fd=parent), 'rb') as stream:
            require(signature(os.fstat(stream.fileno())) == package_sig, 'source-changed')
            zip_preflight(stream)
            with zipfile.ZipFile(stream) as archive:
                members = archive.infolist()
                require(len({member.filename for member in members}) == len(members),
                        'archive-duplicate')
                for info in members:
                    require(info.compress_type == zipfile.ZIP_STORED and
                            not info.flag_bits & 1 and info.file_size <= SOURCE_LIMIT and
                            info.file_size == info.compress_size, 'archive-entry-scope')
                require({'meta', 'collection.anki21b', 'collection.anki2', 'media'} <=
                        {member.filename for member in members}, 'archive-members')
                check_zip_records(stream, archive, members)
                require(archive.getinfo('meta').file_size == 2, 'archive-native-meta')
                require(archive.read('meta') == b'\x08\x03', 'archive-native-meta')
                map_entry = archive.getinfo('media')
                with archive.open(map_entry) as payload:
                    sink = io.BytesIO()
                    map_length = decode_zstd(payload, sink, map_entry.file_size,
                                             native=True, decoded_limit=MAP_LIMIT)
                    map_bytes = sink.getvalue()
                records = media_records(map_bytes)
                require(exact(dict(records), expected_media), 'pre-client-media-map-difference')
                names = ['meta', 'collection.anki21b', 'collection.anki2'] + [
                    str(i) for i in range(len(records))] + ['media']
                require([member.filename for member in members] == names, 'archive-members')
                placeholder = archive.getinfo('collection.anki2')
                with archive.open(placeholder) as payload:
                    require(payload.read(16) == b'SQLite format 3\0', 'archive-placeholder')
                    while payload.read(64 * 1024):
                        pass  # Finish ZIP CRC verification without retaining the placeholder.
                collection = archive.getinfo('collection.anki21b')
                target = os.open('collection.anki2', os.O_WRONLY | os.O_CREAT | os.O_EXCL |
                                 os.O_NOFOLLOW, 0o400, dir_fd=dest)
                with os.fdopen(target, 'wb') as sink, archive.open(collection) as payload:
                    db_length = decode_zstd(payload, sink, collection.file_size, native=True)
                total = 2 + placeholder.file_size + db_length + map_length
                require(total + sum(facts['size'] for _, facts in records) <= TOTAL_LIMIT,
                        'archive-decoded-scope')
                for index, (_, facts) in enumerate(records):
                    info = archive.getinfo(str(index))
                    sink = PayloadDigest()
                    with archive.open(info) as payload:
                        length = decode_zstd(payload, sink, info.file_size, native=True,
                                             decoded_limit=min(SOURCE_LIMIT, facts['size']))
                    require(exact(sink.inventory(), facts), 'media-payload-difference')
                    total += length
                    require(total <= TOTAL_LIMIT, 'archive-decoded-scope')
                # Only the bounded map is retained, always under a fixed own name.
                target = os.open('media-map.bin', os.O_WRONLY | os.O_CREAT | os.O_EXCL |
                                 os.O_NOFOLLOW, 0o400, dir_fd=dest)
                with os.fdopen(target, 'wb') as sink:
                    sink.write(map_bytes)
                map_hash = hashlib.sha256(map_bytes).hexdigest()
            require(signature(os.fstat(stream.fileno())) == package_sig, 'source-changed')
        decoded = output / 'collection.anki2'
        _, decoded_hash, decoded_sig = read_file(dest, 'collection.anki2', limit=DB_LIMIT)
        compare_databases(expected, decoded)
        clock_values = {}
        timing = timing_for(expected, now, args.tz, clock_values=clock_values)
        require(exact(timing_for(decoded, now, args.tz), timing), 'collection-clock-scope')
        write_expected(dest, expected)
        _, raw_hash, _ = read_file(dest, 'expected.json', limit=JSON_LIMIT)
        _, final_hash, final_sig = read_file(exp, expected.name, limit=DB_LIMIT)
        require(final_hash == expected_hash and final_sig == expected_sig and
                signature(decoded.lstat()) == decoded_sig and
                signature(os.stat(package.name, dir_fd=parent, follow_symlinks=False))
                == package_sig, 'staged-input-changed')
        require(load_json(inventory_parent, inventory_path.name, with_hash=True)[1] ==
                inventory_hash, 'staged-input-changed')
        write_json(dest, 'proof.json', {'package_sha256': package_hash,
                    'expected_sha256': expected_hash, 'decoded_sha256': decoded_hash,
                    'raw_sha256': raw_hash, 'start': start, 'now': now,
                    'tz': args.tz, 'utc_offset_secs': offset, 'timing': timing,
                    'clock_values': clock_values,
                    'pre_client_database_equal': True,
                    'pre_client_media_equal': True,
                    'expected_media_inventory_sha256': inventory_hash,
                    'decoded_media_map_sha256': map_hash,
                    'pre_client_stamp_edits': 0}, 0o400)
    finally:
        os.close(parent)
        os.close(dest)
        os.close(exp)
        os.close(inventory_parent)


def validate_direct(run, manifest):
    """Host-only bounded evidence reads; SQLite APIs belong to decode only."""
    output = open_dir(run / 'interop/decoded')
    try:
        require(set(entry_names(output)) == {'collection.anki2', 'expected.json', 'proof.json',
                                              'media-map.bin'},
                'decoder-proof-required')
        proof = load_json(output, 'proof.json')
        work = open_dir(run / 'interop')
        try:
            media, media_hash = load_json(work, 'media-inventory.json', with_hash=True)
        finally:
            os.close(work)
        require(exact(media, manifest['media']) and
                media_hash == manifest['expected_media_inventory_sha256'],
                'media-inventory-changed')
        _, map_hash, _ = read_file(output, 'media-map.bin', limit=MAP_LIMIT)
        expected, raw_hash = load_json(output, 'expected.json', with_hash=True)
        check_dump(expected)
        _, digest, _ = read_file(output, 'collection.anki2', limit=DB_LIMIT)
        require(exact(proof, {'package_sha256': manifest['hashes']['package.colpkg'],
                    'expected_sha256': manifest['hashes']['collection.anki2'],
                    'decoded_sha256': digest, 'raw_sha256': raw_hash,
                    'start': manifest['start'], 'now': manifest['now'], 'timing': proof['timing'],
                    'tz': manifest['tz'], 'utc_offset_secs': manifest['utc_offset_secs'],
                    'clock_values': proof['clock_values'],
                    'pre_client_database_equal': True,
                    'pre_client_media_equal': True,
                    'expected_media_inventory_sha256': media_hash,
                    'decoded_media_map_sha256': map_hash,
                    'pre_client_stamp_edits': 0}), 'decoder-proof-changed')
        start, now = clock(manifest['start'], manifest['tz'])
        offset = clock_offset(now, manifest['tz'])
        timing = proof['timing']
        require(exact(start, manifest['start']) and exact(now, manifest['now']) and
                exact(offset, manifest['utc_offset_secs']) and
                type(timing) is dict and set(timing) ==
                {'days_elapsed', 'now', 'utc_offset_secs', 'next_day_at'} and
                all(type(v) is int for v in timing.values()) and timing['now'] == now and
                timing['utc_offset_secs'] == offset and
                0 <= timing['days_elapsed'] <= (now + offset) // 86400 and
                now < timing['next_day_at'] <= now + 86400, 'collection-clock-scope')
        # Bind the tiny original JSON BLOBs to the raw dump before doing D3 on
        # the host. Keep their exact bytes: whitespace/encoding are not repairs.
        values = proof['clock_values']
        require(type(values) is dict and set(values) ==
                {'creationOffset', 'rollover', 'schedVer'}, 'collection-clock-scope')
        decoded = {}
        for key, value in values.items():
            require(type(value) is str and len(value) <= CLOCK_LIMIT * 2 and
                    re.fullmatch(r'(?:[0-9a-f]{2})*', value) is not None,
                    'collection-clock-scope')
            raw = bytes.fromhex(value)
            require(exact(expected['config'][key]['val'],
                          {'sha256': hashlib.sha256(raw).hexdigest()}), 'decoder-proof-changed')
            decoded[key] = json_bytes(raw)
        require(exact(timing, day_timing(expected['col']['1']['crt'], decoded, now,
                                        manifest['tz'])), 'collection-clock-scope')
        return expected, timing, proof
    finally:
        os.close(output)


def timing_for(db_path, now, tz='UTC', *, clock_values=None):
    # Query the operation-owned DB copy with immutable read-only access.
    # The caller's source DB receives no SQLite connection or write.
    clock_offset(now, tz)
    conn = sqlite_reader(db_path)
    try:
        rows = conn.execute('SELECT id, crt, ver FROM col NOT INDEXED LIMIT 2').fetchall()
        require(len(rows) == 1 and rows[0][0] == 1 and rows[0][2] == 18, 'schema-scope')
        crt = rows[0][1]
        require(type(crt) is int and 0 <= crt <= now, 'clock-scope')
        decoded = {}
        for key in ('creationOffset', 'rollover', 'schedVer'):
            # Lazy BLOB length bounds clock values before selecting payloads.
            rows = conn.execute("SELECT typeof(val), CASE WHEN typeof(val)='blob' "
                                "THEN length(val) ELSE -1 END FROM config NOT INDEXED "
                                'WHERE "KEY" COLLATE BINARY=? LIMIT 2',
                                (key,)).fetchall()
            require(len(rows) == 1 and rows[0][0] == 'blob' and
                    0 <= rows[0][1] <= CLOCK_LIMIT, 'collection-clock-scope')
            value = conn.execute('SELECT val FROM config NOT INDEXED '
                                 'WHERE "KEY" COLLATE BINARY=?', (key,)).fetchone()[0]
            decoded[key] = json_bytes(value)
            if clock_values is not None:
                clock_values[key] = value.hex()
        return day_timing(crt, decoded, now, tz)
    finally:
        conn.close()


def day_timing(crt, decoded, now, tz):
    offset = clock_offset(now, tz)
    require(type(crt) is int and 0 <= crt <= now, 'clock-scope')
    require(type(decoded['creationOffset']) is int and
            decoded['creationOffset'] == -(offset // 60)
            and type(decoded['rollover']) is int and 0 <= decoded['rollover'] <= 23
            and type(decoded['schedVer']) is int and decoded['schedVer'] == 2,
            'collection-clock-scope')
    rollover = decoded['rollover']
    # D1/D3: config offsets are minutes WEST, current offset seconds EAST.
    creation_offset = -decoded['creationOffset'] * 60
    local_day = (now + offset) // 86400
    boundary = local_day * 86400 + rollover * 3600 - offset
    day = max(0, local_day - (crt + creation_offset) // 86400 - (now < boundary))
    return {'days_elapsed': day, 'now': now, 'utc_offset_secs': offset,
            'next_day_at': boundary + (86400 if boundary <= now else 0)}


def validate_run(root, value):
    run = source_path(root, value)
    require(run.is_relative_to(root / 'build' / 'mg' / 'package-interop-check'), 'run-scope')
    fd = open_dir(run, root)
    os.close(fd)
    return run


def mkdir(fd, name):
    os.mkdir(name, 0o700, dir_fd=fd)
    return os.open(name, DIR_FLAGS, dir_fd=fd)


def stage(args):
    root = absolute(args.root)
    start, now = clock(args.now, args.tz)
    offset = clock_offset(now, args.tz)
    run = validate_run(root, args.run)
    parent, exp, package, expected, initial = sources(root, args.package, args.expected)
    try:
        require(not (run.is_relative_to(expected) or expected.is_relative_to(run)
                     or package.is_relative_to(run)), 'source-overlap')
        if args.validate_only:
            # Dry runs still validate content and every input entry, never build.
            read_file(parent, package.name)
            scan_tree(exp, digest=True)
            return
        rfd = open_dir(run, root)
        try:
            work = mkdir(rfd, 'interop')
        finally:
            os.close(rfd)
        try:
            fixtures = mkdir(work, 'fixtures')
            try:
                hashes = {}
                for srcfd, srcname, target in ((parent, package.name, 'package.colpkg'),
                                               (exp, 'collection.anki2', 'collection.anki2')):
                    dest = os.open(target, os.O_WRONLY | os.O_CREAT | os.O_EXCL |
                                   os.O_NOFOLLOW, 0o400, dir_fd=fixtures)
                    with os.fdopen(dest, 'wb') as stream:
                        _, hashes[target], sig = read_file(srcfd, srcname, stream,
                            DB_LIMIT if target == 'collection.anki2' else SOURCE_LIMIT)
                    if target == 'package.colpkg':
                        package_sig = sig
            finally:
                os.close(fixtures)
            # Never put collection.media beside the fixture DB or in the oracle
            # session. F9 must prove import installed every payload itself.
            media_fd = os.open('collection.media', DIR_FLAGS, dir_fd=exp)
            try:
                media = media_inventory(media_fd, sys.platform == 'darwin')
            finally:
                os.close(media_fd)
            require(scan_tree(exp) == initial, 'source-changed')
            require(signature(os.stat(package.name, dir_fd=parent, follow_symlinks=False))
                    == package_sig, 'source-changed')
            scenarios = mkdir(work, 'scenarios')
            try:
                area = mkdir(scenarios, 'rf4')
                try:
                    write_json(area, 'package-interop.json', scenario(start, args.tz), 0o400)
                finally:
                    os.close(area)
            finally:
                os.close(scenarios)
            for name in ('goldens', 'out', 'decoded'):
                os.close(mkdir(work, name))
            write_json(work, 'media-inventory.json', media, 0o400)
            _, inventory_hash, _ = read_file(work, 'media-inventory.json', limit=JSON_LIMIT)
            manifest = {'expected_media_inventory_sha256': inventory_hash,
                        'package': str(package), 'expected': str(expected),
                        'package_signature': package_sig, 'tree': initial,
                        'media': media, 'hashes': hashes, 'start': start, 'now': now,
                        'tz': args.tz, 'utc_offset_secs': offset}
            write_json(work, 'manifest.json', manifest)
        finally:
            os.close(work)
    finally:
        os.close(parent)
        os.close(exp)


def exact(a, b):
    """JSON equality retaining SQLite integer/real and bool distinctions."""
    if type(a) is not type(b):
        return False
    if isinstance(a, dict):
        return a.keys() == b.keys() and all(exact(a[k], b[k]) for k in a)
    if isinstance(a, list):
        return len(a) == len(b) and all(exact(x, y) for x, y in zip(a, b))
    if type(a) is float:
        return a.hex() == b.hex()
    return a == b


def check_dump(dump):
    require(type(dump) is dict and dump.keys() == COLUMNS.keys(), 'incomplete-dump')
    for table, columns in COLUMNS.items():
        rows = dump[table]
        require(type(rows) is dict, 'malformed-dump')
        for key, row in rows.items():
            require(type(row) is dict and set(row) == set(columns.split()), 'incomplete-dump')
            for value in row.values():
                require(value is None or type(value) in (int, float, str) or
                        (type(value) is dict and set(value) == {'sha256'} and
                         type(value['sha256']) is str and HEX64.fullmatch(value['sha256'])),
                        'malformed-dump')
                require(type(value) is not float or math.isfinite(value), 'malformed-dump')
            parts = KEYS.get(table, ('id',))
            require(all(type(row[p]) is (str if p in ('tag', 'KEY') else int)
                        for p in parts), 'malformed-row-key')
            require(key == ','.join(str(row[p]) for p in parts), 'malformed-row-key')
    require(set(dump['col']) == {'1'} and type(dump['col']['1']['ver']) is int
            and dump['col']['1']['ver'] == 18, 'schema-scope')


def oracle_meta(value, pin):
    require(type(value) is dict and set(value) == {'anki_version', 'lock_sha256', 'driver_sha256'},
            'oracle-metadata')
    require(value['anki_version'] == pin['anki_version'] and
            value['lock_sha256'] == pin['lock_sha256'] and
            type(value['driver_sha256']) is str and HEX64.fullmatch(value['driver_sha256']),
            'oracle-pin')


def no_errors(value):
    if type(value) is dict:
        require(not any(k in ('error', 'errors') and v not in (None, [], {})
                        for k, v in value.items()), 'oracle-error')
        for v in value.values():
            no_errors(v)
    elif type(value) is list:
        for v in value:
            no_errors(v)


def verify(args):
    root = absolute(args.root)
    run = validate_run(root, args.run)
    work = open_dir(run / 'interop', root)
    try:
        # Validate all output entry types and ownership before reading reports.
        scan_tree(work)
        manifest = load_json(work, 'manifest.json')
        parent, exp, package, _, tree = sources(root, manifest['package'], manifest['expected'])
        try:
            require(tree == manifest['tree'] and signature(os.stat(package.name, dir_fd=parent,
                    follow_symlinks=False)) == manifest['package_signature'], 'source-changed')
        finally:
            os.close(parent)
            os.close(exp)
        fixtures = os.open('fixtures', DIR_FLAGS, dir_fd=work)
        try:
            require(set(entry_names(fixtures)) == {'collection.anki2', 'package.colpkg'},
                    'fixture-isolation')
            for name, digest in manifest['hashes'].items():
                _, got, _ = read_file(fixtures, name)
                require(got == digest, 'staged-input-changed')
        finally:
            os.close(fixtures)
        scenarios = open_dir(run / 'interop/scenarios/rf4', root)
        try:
            sc = load_json(scenarios, 'package-interop.json')
            require(exact(sc, scenario(manifest['start'], manifest['tz'])), 'scenario-changed')
        finally:
            os.close(scenarios)
        expected_dump, timing, archive_proof = validate_direct(run, manifest)
        if args.command == 'ready':
            opened_goldens = open_dir(run / 'interop/goldens', root)
            opened_out = open_dir(run / 'interop/out', root)
            try:
                require(not entry_names(opened_goldens) and not entry_names(opened_out),
                        'fresh-output-required')
            finally:
                os.close(opened_goldens)
                os.close(opened_out)
            return
        rootfd = open_dir(root / 'conformance', root)
        try:
            pin = load_json(rootfd, 'oracle.json')
        finally:
            os.close(rootfd)
        out = os.open('out', DIR_FLAGS, dir_fd=work)
        goldens = open_dir(run / 'interop/goldens/rf4', root)
        try:
            report = load_json(out, 'report.json')
            golden = load_json(goldens, 'package-interop.json')
            no_errors(report)
            require(type(report) is dict and report.get('command') == 'record'
                    and type(report.get('scenario_count')) is int and report['scenario_count'] == 1
                    and type(report.get('jobs')) is int and report['jobs'] == 1
                    and exact(report.get('totals'), {'recorded': 1}), 'oracle-report')
            results = report.get('scenarios')
            require(type(results) is list and len(results) == 1 and type(results[0]) is dict
                    and results[0].get('scenario') == ID and results[0].get('status') == 'recorded',
                    'oracle-status')
            oracle_meta(report.get('oracle'), pin)
            require(type(golden) is dict and set(golden) == {'scenario_id', 'scenario_sha256',
                    'oracle', 'clock', 'tz', 'observations'}, 'golden-shape')
            oracle_meta(golden['oracle'], pin)
            require(exact(report['oracle'], golden['oracle']), 'oracle-metadata')
            require(golden['scenario_id'] == ID and golden['tz'] == manifest['tz'] and
                    exact(golden['clock'], {'start': manifest['start'], 'mode': 'frozen'}) and
                    golden['scenario_sha256'] == hashlib.sha256(canonical(sc).encode()).hexdigest(),
                    'golden-identity')
            observations = golden['observations']
            require(type(observations) is dict and set(observations) == {'steps'}, 'observation-shape')
            steps = observations['steps']
            require(type(steps) is list and len(steps) == 5, 'observation-count')
            for i, keys in enumerate((set(), {'db_dump', 'timing'}, set(),
                                      {'db_dump', 'timing'}, {'media_dir'})):
                require(type(steps[i]) is dict and set(steps[i]) == keys, 'observation-shape')
            require(exact(steps[1]['timing'], timing) and
                    exact(steps[3]['timing'], timing), 'timing-difference')
            before, after = steps[1]['db_dump'], steps[3]['db_dump']
            check_dump(before)
            check_dump(after)
            bind_dump(expected_dump, before)
            bind_dump(expected_dump, after, manifest['now'] * 1000)
            media = steps[4]['media_dir']
            require(type(media) is dict, 'media-shape')
            for value in media.values():
                require(type(value) is dict and set(value) == {'sha1', 'size'} and
                        type(value['size']) is int and value['size'] >= 0 and
                        type(value['sha1']) is str and HEX40.fullmatch(value['sha1']), 'media-shape')
            require(exact(media, manifest['media']), 'media-difference')
            evidence = {'tables': 12, 'rows': sum(len(t) for t in before.values()),
                        'media_files': len(media), 'media_bytes': sum(v['size'] for v in media.values()),
                        'database_equal': True, 'media_equal': True, 'timing_equal': True,
                        'package_sha256': manifest['hashes']['package.colpkg'],
                        'pre_client_database_equal': True,
                        'pre_client_stamp_edits': 0,
                        'oracle_open_edits': 0,
                        'import_col_mod_scm_ms': manifest['now'] * 1000,
                        'start': manifest['start'], 'now': manifest['now'],
                        'tz': manifest['tz'], 'utc_offset_secs': manifest['utc_offset_secs'],
                        'pre_client_media_equal': True,
                        'decoded_media_map_sha256': archive_proof['decoded_media_map_sha256'],
                        'imported_media_inventory_sha256':
                            hashlib.sha256(canonical(media).encode('utf-8')).hexdigest()}
            write_json(work, 'evidence.json', evidence)
        finally:
            os.close(out)
            os.close(goldens)
    finally:
        os.close(work)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest='command', required=True)
    for command in ('stage', 'ready', 'check'):
        p = sub.add_parser(command)
        p.add_argument('--root', required=True)
        p.add_argument('--run', required=True)
        if command == 'stage':
            p.add_argument('--package', required=True)
            p.add_argument('--expected', required=True)
            p.add_argument('--now', required=True)
            p.add_argument('--tz', required=True)
            p.add_argument('--validate-only', action='store_true')
    p = sub.add_parser('decode')
    p.add_argument('--package', required=True)
    p.add_argument('--expected', required=True)
    p.add_argument('--output', required=True)
    p.add_argument('--media-inventory', required=True)
    p.add_argument('--now', required=True)
    p.add_argument('--tz', required=True)
    args = parser.parse_args()
    try:
        if args.command == 'decode':
            decode(args)
        elif args.command == 'stage':
            stage(args)
        else:
            verify(args)
    except (Refusal, OSError, ValueError, KeyError, TypeError, sqlite3.Error, UnicodeError,
            RecursionError, OverflowError, zipfile.BadZipFile, RuntimeError) as error:
        # Never disclose paths, filenames, SQL rows, JSON content or exception text.
        code = str(error) if isinstance(error, Refusal) else 'invalid-or-unsafe-input'
        print('package-interop-check: ' + code, file=sys.stderr)
        return 1
    return 0


if __name__ == '__main__':
    sys.exit(main())
