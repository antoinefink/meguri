```json
{
  "role": "oracle-maintainer",
  "session": "s-20261021T005801Z-68080c49",
  "origin_commits": [
    "1511bc1e55eb0c47d8280796e3876aa14c210db7",
    "3b6d8dc71a049e8f88a433e03c2c357a901dfa38",
    "823ebef7e32acc3d425faabc60aa13b7488ec177"
  ],
  "prepared_blob": "32fcd67b64aa9afb95f3868510fffd5db1033981",
  "prepared_sha256": "ba8b6f5fe0787c0633fcb40648c55b5e500b8effb17558d82e647f717ebea608",
  "oracle_version": "26.09.3",
  "frozen_input_commit": "b65827bdb6f21655730f5acf486739ae6d1dd846",
  "recording_run": "20261021T010335Z-5746",
  "linux_compare_run": "20261021T010403Z-6551",
  "apple_compare_run": "20261021T010421Z-7278",
  "recorded_cases": 16,
  "compared_cases": 38,
  "preserved_pairs": 22,
  "adapter_changes": 0,
  "test_changes": 0,
  "public_upstream": {
    "repository": "https://github.com/ankitects/anki",
    "tag": "26.09.3",
    "commit": "29bb700b951e3f0c0cb69b77c0180fc1fe33e6ba"
  },
  "source_citations": [
    {
      "id": "s01",
      "path": "rslib/src/scheduler/answering/mod.rs",
      "blob": "ff81d86fa67f8cf24efce1c27d199f7e80522856",
      "sha256": "8b4be53a9f812699186922da8f898103457fe83e150eb3cd666210a64ddbde47",
      "file_line_range": [
        1,
        1044
      ],
      "url": "https://github.com/ankitects/anki/blob/29bb700b951e3f0c0cb69b77c0180fc1fe33e6ba/rslib/src/scheduler/answering/mod.rs#L1-L1044"
    },
    {
      "id": "s02",
      "path": "rslib/src/decks/stats.rs",
      "blob": "4169e6494953805f85beb662fa163e2190d61790",
      "sha256": "928120714eab04cb0f5b0de271e2054b9873d32678d482336fbee31485a61477",
      "file_line_range": [
        1,
        90
      ],
      "url": "https://github.com/ankitects/anki/blob/29bb700b951e3f0c0cb69b77c0180fc1fe33e6ba/rslib/src/decks/stats.rs#L1-L90"
    },
    {
      "id": "s03",
      "path": "rslib/src/decks/undo.rs",
      "blob": "a1ca3e171f12cee5d9254f0e7f2b7f074eec59ea",
      "sha256": "9843a7dcd67d9b6f860927609e0fbf813b576d227fcd20c7cd9f7518819359b9",
      "file_line_range": [
        1,
        98
      ],
      "url": "https://github.com/ankitects/anki/blob/29bb700b951e3f0c0cb69b77c0180fc1fe33e6ba/rslib/src/decks/undo.rs#L1-L98"
    },
    {
      "id": "s04",
      "path": "rslib/src/scheduler/answering/preview.rs",
      "blob": "586223af95c9909157e7fd165c0c1a4b22ab2323",
      "sha256": "19cf1867554b0a237d8cb89bcee05f987dbf799e9b61e7267f859057e6d5fc4e",
      "file_line_range": [
        1,
        139
      ],
      "url": "https://github.com/ankitects/anki/blob/29bb700b951e3f0c0cb69b77c0180fc1fe33e6ba/rslib/src/scheduler/answering/preview.rs#L1-L139"
    },
    {
      "id": "s05",
      "path": "rslib/src/scheduler/states/filtered.rs",
      "blob": "1fba3da9763eb7f394761531960f1f467b4d66f5",
      "sha256": "e200cfa0719882e7092e345d0be8e2357d9d8058be3da11601e81ffa4ebe29f4",
      "file_line_range": [
        1,
        46
      ],
      "url": "https://github.com/ankitects/anki/blob/29bb700b951e3f0c0cb69b77c0180fc1fe33e6ba/rslib/src/scheduler/states/filtered.rs#L1-L46"
    },
    {
      "id": "s06",
      "path": "rslib/src/scheduler/service/answering.rs",
      "blob": "778cf49a1b66d437a0d9e3d7749ae71fd21d47fd",
      "sha256": "5e1e6fa9e359da58c2f1f7ff9e45a28451d58200a8a22034595d307beca5045a",
      "file_line_range": [
        1,
        70
      ],
      "url": "https://github.com/ankitects/anki/blob/29bb700b951e3f0c0cb69b77c0180fc1fe33e6ba/rslib/src/scheduler/service/answering.rs#L1-L70"
    },
    {
      "id": "s07",
      "path": "qt/aqt/reviewer.py",
      "blob": "59c829c475bfe4400b890358fe84cbe42cd5466d",
      "sha256": "7be45f1c0da888a1bb530ea37f824f8f55a7e943ab2e35c5d8ff47e1ac33d363",
      "file_line_range": [
        1,
        1244
      ],
      "url": "https://github.com/ankitects/anki/blob/29bb700b951e3f0c0cb69b77c0180fc1fe33e6ba/qt/aqt/reviewer.py#L1-L1244"
    },
    {
      "id": "s08",
      "path": "pylib/anki/scheduler/v3.py",
      "blob": "3c1123d0b9dfd917bb648514b21da3f023c4d51c",
      "sha256": "32995159687a98a2020a204a26228abe948067945b0880d17e8c558b56f4b2ba",
      "file_line_range": [
        1,
        241
      ],
      "url": "https://github.com/ankitects/anki/blob/29bb700b951e3f0c0cb69b77c0180fc1fe33e6ba/pylib/anki/scheduler/v3.py#L1-L241"
    },
    {
      "id": "s09",
      "path": "proto/anki/scheduler.proto",
      "blob": "b056d1dd99b0c727c8aa1a264fdc5b932534e621",
      "sha256": "9ebdd150185cdaccef2a7957481361b869344e573cce4dc0be77489cac7b71e3",
      "file_line_range": [
        1,
        485
      ],
      "url": "https://github.com/ankitects/anki/blob/29bb700b951e3f0c0cb69b77c0180fc1fe33e6ba/proto/anki/scheduler.proto#L1-L485"
    },
    {
      "id": "s10",
      "path": "rslib/src/scheduler/bury_and_suspend.rs",
      "blob": "8511995086d210afe1a43803d5dfe9209c3fe278",
      "sha256": "6e155f0368de174f8e57ac773e8ddd3846605982b7e2b3cf0c89a69ee15dbeb9",
      "file_line_range": [
        1,
        204
      ],
      "url": "https://github.com/ankitects/anki/blob/29bb700b951e3f0c0cb69b77c0180fc1fe33e6ba/rslib/src/scheduler/bury_and_suspend.rs#L1-L204"
    },
    {
      "id": "s11",
      "path": "rslib/src/storage/card/siblings_for_bury.sql",
      "blob": "7731b45ec146c747cf500db561eb9a7007043915",
      "sha256": "5394b16ff7eac8c4f84633108976ca6e56a15e3375fca0b20ccf24d4929ccd52",
      "file_line_range": [
        1,
        19
      ],
      "url": "https://github.com/ankitects/anki/blob/29bb700b951e3f0c0cb69b77c0180fc1fe33e6ba/rslib/src/storage/card/siblings_for_bury.sql#L1-L19"
    },
    {
      "id": "s12",
      "path": "rslib/src/storage/card/mod.rs",
      "blob": "82ee61c1354eb7280578d4d7ae410b6b86d9d6df",
      "sha256": "082b9eee1d3dbc7ff0cc1bcc7ee2b88a95b34a591149cacf60a74172f728e3e6",
      "file_line_range": [
        1,
        1062
      ],
      "url": "https://github.com/ankitects/anki/blob/29bb700b951e3f0c0cb69b77c0180fc1fe33e6ba/rslib/src/storage/card/mod.rs#L1-L1062"
    },
    {
      "id": "s13",
      "path": "rslib/src/scheduler/queue/builder/burying.rs",
      "blob": "2bd843a948c2fdb7065bff4cc7d5e504286c456a",
      "sha256": "47d3beb27b6442b77e1b5709befbbf0fef506aa51dfb65f9dd57fabda48ea80e",
      "file_line_range": [
        1,
        94
      ],
      "url": "https://github.com/ankitects/anki/blob/29bb700b951e3f0c0cb69b77c0180fc1fe33e6ba/rslib/src/scheduler/queue/builder/burying.rs#L1-L94"
    },
    {
      "id": "s14",
      "path": "rslib/src/scheduler/states/normal.rs",
      "blob": "c41f7e9cb5d25ede957bcfd9daa706778b8c35fa",
      "sha256": "939d8157473cd445183f7b6f7179c87499667cb1057f47d728a6b4c03806c2c1",
      "file_line_range": [
        1,
        99
      ],
      "url": "https://github.com/ankitects/anki/blob/29bb700b951e3f0c0cb69b77c0180fc1fe33e6ba/rslib/src/scheduler/states/normal.rs#L1-L99"
    },
    {
      "id": "s15",
      "path": "rslib/src/scheduler/states/mod.rs",
      "blob": "9a4aa7c3c120a83b0cfda5360ceb5fed84f020ca",
      "sha256": "f4247b73f39403a1f8eb423d52bec91bf3e7f6a005f1b93767a24a0a0ee8f7a9",
      "file_line_range": [
        1,
        223
      ],
      "url": "https://github.com/ankitects/anki/blob/29bb700b951e3f0c0cb69b77c0180fc1fe33e6ba/rslib/src/scheduler/states/mod.rs#L1-L223"
    },
    {
      "id": "s16",
      "path": "rslib/src/deckconfig/schema11.rs",
      "blob": "b7ed0acaa806aac2ed0dddd0b8a61a0ab3322e56",
      "sha256": "4d62c791120c06813a206837475fb877a57107e918f07594c4d59a391f5534c9",
      "file_line_range": [
        1,
        627
      ],
      "url": "https://github.com/ankitects/anki/blob/29bb700b951e3f0c0cb69b77c0180fc1fe33e6ba/rslib/src/deckconfig/schema11.rs#L1-L627"
    },
    {
      "id": "s17",
      "path": "proto/anki/deck_config.proto",
      "blob": "181ed4e7d550772b989d126bce7290d00a831fac",
      "sha256": "ee07a4f2fd6f18e0924ea11c666e32b9427063d8313ac958ad920f9cb762e44e",
      "file_line_range": [
        1,
        265
      ],
      "url": "https://github.com/ankitects/anki/blob/29bb700b951e3f0c0cb69b77c0180fc1fe33e6ba/proto/anki/deck_config.proto#L1-L265"
    },
    {
      "id": "s18",
      "path": "ftl/core/studying.ftl",
      "blob": "a317a68bae8b14f95d62431206c293980dc8e4e0",
      "sha256": "033cef35dbb108e9b6e3d1e4f8248ca74e554ff7fdaedbdd492ab952ca57d1c0",
      "file_line_range": [
        1,
        72
      ],
      "url": "https://github.com/ankitects/anki/blob/29bb700b951e3f0c0cb69b77c0180fc1fe33e6ba/ftl/core/studying.ftl#L1-L72"
    },
    {
      "id": "s19",
      "path": "rslib/src/deckconfig/mod.rs",
      "blob": "3f4f888cce11aeb045d619593f63d3f9026fd385",
      "sha256": "ca7a64718d29067b6efd94fe8a440c42caa5f62420845d344a6ab9711f4624ec",
      "file_line_range": [
        1,
        327
      ],
      "url": "https://github.com/ankitects/anki/blob/29bb700b951e3f0c0cb69b77c0180fc1fe33e6ba/rslib/src/deckconfig/mod.rs#L1-L327"
    },
    {
      "id": "s20",
      "path": "rslib/src/scheduler/states/review.rs",
      "blob": "603f98b225cbd80e6f7860946046d067e6713289",
      "sha256": "b7414b79b7426816f131e7445b97570af21d98b11589bd010c3a704b17a7cec7",
      "file_line_range": [
        1,
        401
      ],
      "url": "https://github.com/ankitects/anki/blob/29bb700b951e3f0c0cb69b77c0180fc1fe33e6ba/rslib/src/scheduler/states/review.rs#L1-L401"
    },
    {
      "id": "s21",
      "path": "rslib/src/storage/revlog/add.sql",
      "blob": "fae7953b8650d8413f4cc11ad4314bf9e4ac4014",
      "sha256": "38f0917145b2afd05aa2afe6b188dcc8392a520bcad843665e6369427908f809",
      "file_line_range": [
        1,
        35
      ],
      "url": "https://github.com/ankitects/anki/blob/29bb700b951e3f0c0cb69b77c0180fc1fe33e6ba/rslib/src/storage/revlog/add.sql#L1-L35"
    },
    {
      "id": "s22",
      "path": "rslib/src/storage/revlog/mod.rs",
      "blob": "31ff613fa3ac32565acb4417c8311617f9534318",
      "sha256": "df60cf36cf4328939d67c467015c67ad1a564c69369e70f212e40485b25c4f22",
      "file_line_range": [
        1,
        221
      ],
      "url": "https://github.com/ankitects/anki/blob/29bb700b951e3f0c0cb69b77c0180fc1fe33e6ba/rslib/src/storage/revlog/mod.rs#L1-L221"
    },
    {
      "id": "s23",
      "path": "rslib/src/card/mod.rs",
      "blob": "8e1336ac72812f96908106bb92d0eb7834b1dab8",
      "sha256": "1ae92663f49d469486520c6f18aae12bd6604138dabd137606cde31e9c29edd9",
      "file_line_range": [
        1,
        813
      ],
      "url": "https://github.com/ankitects/anki/blob/29bb700b951e3f0c0cb69b77c0180fc1fe33e6ba/rslib/src/card/mod.rs#L1-L813"
    },
    {
      "id": "s24",
      "path": "rslib/src/config/bool.rs",
      "blob": "1be9b2556984386ecb5861f8ef64ec25a99bf96e",
      "sha256": "a29a3780e53c917b30bfc967d6a1b969929f90e94de2eceb8500031df54ea36e",
      "file_line_range": [
        1,
        100
      ],
      "url": "https://github.com/ankitects/anki/blob/29bb700b951e3f0c0cb69b77c0180fc1fe33e6ba/rslib/src/config/bool.rs#L1-L100"
    },
    {
      "id": "s25",
      "path": "rslib/src/scheduler/queue/builder/mod.rs",
      "blob": "1f12450b1970cdaac6f93aba781f67f7bda891e8",
      "sha256": "66c08d5f19a874c2c27a42391f3e34c550f283d06e9c6b89d1daeee4aa730633",
      "file_line_range": [
        1,
        551
      ],
      "url": "https://github.com/ankitects/anki/blob/29bb700b951e3f0c0cb69b77c0180fc1fe33e6ba/rslib/src/scheduler/queue/builder/mod.rs#L1-L551"
    },
    {
      "id": "s26",
      "path": "rslib/src/scheduler/states/load_balancer.rs",
      "blob": "86969af3f4ea6a518b9990387e26706830b42ec0",
      "sha256": "0ceaea124986bd3d107ca424e7b1993494423d012a285d833ccd7cb6ab8f42ea",
      "file_line_range": [
        1,
        456
      ],
      "url": "https://github.com/ankitects/anki/blob/29bb700b951e3f0c0cb69b77c0180fc1fe33e6ba/rslib/src/scheduler/states/load_balancer.rs#L1-L456"
    },
    {
      "id": "s27",
      "path": "rslib/src/scheduler/states/fuzz.rs",
      "blob": "73f6eda8ea09938f41fcc64efc543bfbf851b88c",
      "sha256": "1456b8e52d825e5b73d7beec4f548e53b1d96dcd30f33ded5d8dfd290e2e3e35",
      "file_line_range": [
        1,
        209
      ],
      "url": "https://github.com/ankitects/anki/blob/29bb700b951e3f0c0cb69b77c0180fc1fe33e6ba/rslib/src/scheduler/states/fuzz.rs#L1-L209"
    },
    {
      "id": "s28",
      "path": "rslib/src/scheduler/queue/undo.rs",
      "blob": "02046eebcff233e096bc92e353ce0786b7da49db",
      "sha256": "3e2e978b12f04db4c27a6c509d897794f76bc4ccfe4fcda5f81a0ade216a7b02",
      "file_line_range": [
        1,
        293
      ],
      "url": "https://github.com/ankitects/anki/blob/29bb700b951e3f0c0cb69b77c0180fc1fe33e6ba/rslib/src/scheduler/queue/undo.rs#L1-L293"
    },
    {
      "id": "s29",
      "path": "rslib/src/scheduler/queue/mod.rs",
      "blob": "6a8357142df04cc80e2b9a0a0e422402958b76ec",
      "sha256": "ecdb6d90c70a1a1c74d91f82655d0da84c4ece71dc103a66ec40fc6de1c6f5d7",
      "file_line_range": [
        1,
        300
      ],
      "url": "https://github.com/ankitects/anki/blob/29bb700b951e3f0c0cb69b77c0180fc1fe33e6ba/rslib/src/scheduler/queue/mod.rs#L1-L300"
    },
    {
      "id": "s30",
      "path": "rslib/src/collection/transact.rs",
      "blob": "2d98b6679599a9b2dc27b28661e9cd7e8141f60f",
      "sha256": "116b662bac90073ef04e40f43ff60e1546c0a5b6b0219466430aba1a31ae8be0",
      "file_line_range": [
        1,
        74
      ],
      "url": "https://github.com/ankitects/anki/blob/29bb700b951e3f0c0cb69b77c0180fc1fe33e6ba/rslib/src/collection/transact.rs#L1-L74"
    },
    {
      "id": "s31",
      "path": "rslib/src/ops.rs",
      "blob": "be27b649a6e0a0810529aa9af61b2b5fbaa7e25b",
      "sha256": "1e1ad13156fbc7ab83db173d52fd8312a66630772714f59f21b036f2617423a2",
      "file_line_range": [
        1,
        182
      ],
      "url": "https://github.com/ankitects/anki/blob/29bb700b951e3f0c0cb69b77c0180fc1fe33e6ba/rslib/src/ops.rs#L1-L182"
    },
    {
      "id": "s32",
      "path": "rslib/src/config/mod.rs",
      "blob": "1e507281ac8fd8a3d16ec02129f1aafb42be0cc9",
      "sha256": "4d288b43ebfb365e083f2fcf352e6c6a87b973c0e8fea8aedcef872db8bd03eb",
      "file_line_range": [
        1,
        365
      ],
      "url": "https://github.com/ankitects/anki/blob/29bb700b951e3f0c0cb69b77c0180fc1fe33e6ba/rslib/src/config/mod.rs#L1-L365"
    },
    {
      "id": "s33",
      "path": "rslib/src/scheduler/service/mod.rs",
      "blob": "073706ce81982afb217547c5759b6d8706d92db3",
      "sha256": "de65847d35d79aba01143053f55a3498ad2e007228edfad9ad9910e606569f3e",
      "file_line_range": [
        1,
        1755
      ],
      "url": "https://github.com/ankitects/anki/blob/29bb700b951e3f0c0cb69b77c0180fc1fe33e6ba/rslib/src/scheduler/service/mod.rs#L1-L1755"
    }
  ],
  "section_source_map": [
    {
      "prepared_group": 1,
      "sections": [
        "sched/answering#A2",
        "sched/answering#A13"
      ],
      "source_ids": [
        "s01"
      ]
    },
    {
      "prepared_group": 2,
      "sections": [
        "sched/answering#A6"
      ],
      "source_ids": [
        "s02",
        "s03"
      ]
    },
    {
      "prepared_group": 3,
      "sections": [
        "sched/answering#A12"
      ],
      "source_ids": [
        "s01",
        "s04",
        "s05"
      ]
    },
    {
      "prepared_group": 4,
      "sections": [
        "sched/answering#A15"
      ],
      "source_ids": [
        "s06",
        "s07",
        "s08",
        "s09"
      ]
    },
    {
      "prepared_group": 5,
      "sections": [
        "sched/sibling-burying#B1",
        "sched/sibling-burying#B2",
        "sched/sibling-burying#B3",
        "sched/sibling-burying#B4"
      ],
      "source_ids": [
        "s10",
        "s11",
        "s12",
        "s13"
      ]
    },
    {
      "prepared_group": 6,
      "sections": [
        "sched/leeches#L1",
        "sched/leeches#L2",
        "sched/leeches#L3",
        "sched/leeches#L4",
        "sched/leeches#L5"
      ],
      "source_ids": [
        "s01",
        "s14",
        "s15",
        "s16",
        "s17",
        "s07",
        "s18"
      ]
    },
    {
      "prepared_group": 7,
      "sections": [
        "sched/leeches#L1"
      ],
      "source_ids": [
        "s19",
        "s20",
        "s17"
      ]
    },
    {
      "prepared_group": 8,
      "sections": [
        "sched/answering#A1"
      ],
      "source_ids": [
        "s01"
      ]
    },
    {
      "prepared_group": 9,
      "sections": [
        "sched/revlog#E1",
        "sched/revlog#E2",
        "sched/revlog#E3",
        "sched/revlog#E4"
      ],
      "source_ids": [
        "s21",
        "s22",
        "s23"
      ]
    },
    {
      "prepared_group": 10,
      "sections": [
        "sched/load-balancer#W1"
      ],
      "source_ids": [
        "s24",
        "s25"
      ]
    },
    {
      "prepared_group": 11,
      "sections": [
        "sched/load-balancer#W2"
      ],
      "source_ids": [
        "s26",
        "s12"
      ]
    },
    {
      "prepared_group": 12,
      "sections": [
        "sched/load-balancer#W3",
        "sched/load-balancer#W4",
        "sched/load-balancer#W5",
        "sched/load-balancer#W6",
        "sched/load-balancer#W7"
      ],
      "source_ids": [
        "s26",
        "s27",
        "s01"
      ]
    },
    {
      "prepared_group": 13,
      "sections": [
        "sched/load-balancer#W8",
        "sched/load-balancer#W9"
      ],
      "source_ids": [
        "s01",
        "s28",
        "s29"
      ]
    },
    {
      "prepared_group": 14,
      "sections": [
        "conformance/answer-ops#H2",
        "conformance/answer-ops#H3",
        "conformance/answer-ops#H4"
      ],
      "source_ids": [
        "s29",
        "s30",
        "s31",
        "s32",
        "s06"
      ]
    }
  ],
  "prepared_summary_only_sections": [
    "sched/answering#A7",
    "sched/answering#A10",
    "sched/answering#A14"
  ],
  "historical_scenario_test_links": [
    {
      "source_id": "s28",
      "symbol": "undo",
      "line_anchors": [
        100
      ],
      "scenario_ids": [
        "sched/bury/ae22-undo",
        "sched/leech/suspend"
      ]
    },
    {
      "source_id": "s33",
      "symbol": "state_is_leech_reflects_review_leeched_flag",
      "line_anchors": [
        1409
      ],
      "scenario_ids": [
        "sched/leech/ae25-tag-only"
      ]
    },
    {
      "source_id": "s01",
      "symbol": "state_application",
      "line_anchors": [
        780
      ],
      "scenario_ids": [
        "sched/answer/state-kinds"
      ]
    },
    {
      "source_id": "s33",
      "symbol": "answer_again_moves_new_card_to_learn_queue",
      "line_anchors": [
        508
      ],
      "scenario_ids": [
        "sched/answer/new-card-ratings"
      ]
    },
    {
      "source_id": "s33",
      "symbol": "answer_hard_keeps_new_card_in_learning_with_longer_delay_than_again",
      "line_anchors": [
        524
      ],
      "scenario_ids": [
        "sched/answer/new-card-ratings"
      ]
    },
    {
      "source_id": "s33",
      "symbol": "answer_easy_graduates_new_card_to_review_queue",
      "line_anchors": [
        588
      ],
      "scenario_ids": [
        "sched/answer/new-card-ratings"
      ]
    },
    {
      "source_id": "s33",
      "symbol": "answer_good_repeatedly_graduates_card_through_learning_steps",
      "line_anchors": [
        555
      ],
      "scenario_ids": [
        "sched/answer/new-card-ratings",
        "sched/answer/state-kinds"
      ]
    },
    {
      "source_id": "s33",
      "symbol": "counts_for_deck_today_tracks_new_cards_studied",
      "line_anchors": [
        1489
      ],
      "scenario_ids": [
        "sched/answer/new-card-ratings",
        "sched/answer/deck-counters"
      ]
    }
  ],
  "prepared_origins_are_secondary": true
}
```
