# Image occlusion authoring

U205 and U206 start from `d98894622`. The Add and Edit Note flows use the
existing Rust image occlusion format and collection operations. The native
canvas owns the image and mask interactions. The fields view owns the other
note fields and tags. Browse keeps the selected note inside its inspector.

## Session record privacy review

The independent privacy reviewer read each complete record and admitted the
following bytes as public operational metadata. The coordinator verified the
SHA-256 digests against the files in this checkout.

| Session | SHA-256 |
|---|---|
| s-20261126T090109Z-df59ef0b | c5009abeca25bfe68b68651d8656202a528d4c241f9d08f32af6eb3564b2c35b |
| s-20261126T090125Z-beeb9067 | 2711ca4c1c62ffe65ac08dcb3259e2f7ebab5429b742eda307d45497bb447eab |
| s-20261126T090405Z-f8c3bebf | c4c402eb6873e1d8eb4766f621efa0664290566749b08c310f783409a73f0e6c |
