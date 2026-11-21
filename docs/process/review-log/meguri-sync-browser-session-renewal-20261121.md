# Meguri Sync dashboard sign-in

The work started at `b01b08e53`. The baseline test
`sync-server-test/20261121T135821Z-61128` confirmed the 24-hour browser
limit and verified that the native sync key still worked after browser expiry.

An independent spec-writer updated W2. The implementation export is
`cleanroom/20261121T135848Z-66438`. Independent review
`cleanroom/20261121T140404Z-83970` verified the authority checks and assessed
the three session records as public operational metadata. It found a
timing-sensitive cookie assertion and missing race coverage at renewal.

`sync-server-test/20261121T140425Z-87826` passed 69 selected server tests.
`sync-web-verify/20261121T140424Z-86176` passed nine frontend tests and 44
browser tests against the changed server. Server lint passed in
`sync-server-lint/20261121T140502Z-91573`.

The follow-up keeps exact lifetime checks in the auth suite and verifies
sign-out between the renewal read and writer. Independent review
`cleanroom/20261121T141044Z-36323` confirmed the feature-gated pause, found
an unbounded wait for it, and classified the fixer session record as public.
Export `cleanroom/20261121T141315Z-59587` bounds that wait and releases the
worker before reporting a timeout.

The 27 selected auth, HTTP contract and race cases passed in
`sync-server-test/20261121T141105Z-46620`. The final bounded race passed in
`sync-server-test/20261121T141429Z-65118`. Both server feature graphs passed
lint in `sync-server-lint/20261121T141428Z-64475`.
