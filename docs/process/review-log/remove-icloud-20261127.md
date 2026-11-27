# Remove iCloud from Meguri

The unit started at `2b80682e8`. The user authorized removing iCloud and merging
the verified pull request. Meguri Sync remains available through the sync
settings on Mac, iPhone and iPad.

## Implementation and review

- Initial app removal used filtered session `s-20261127T065608Z-f736fb6a`.
- The collection recovery work used filtered session `s-20261127T065853Z-a0976797`.
- Review found that rewriting archival bindings breaks replacement history
  required by Meguri media recovery. Session `s-20261127T070953Z-653845b2`
  preserved that history. The focused run `core-test/20261127T072207Z-14982`
  passed all 15 recovery cases.
- The independent privacy reviewer read the complete N30 contract and
  classified it as PUBLIC. The verified SHA-256 is
  `cd47b8392bcb4edbe3c2cbfaf62585f0cc7d64e36f00f4b831cb340aa1041c4e`.
  The five initial session records contain only public operational metadata.

## Verification still in progress

The first app patch removes the native SDK transport, signing capabilities
and service choice. Remaining app workflow and FFI cleanup, Apple builds,
native journeys and hosted checks are in progress.
