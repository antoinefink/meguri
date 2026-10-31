# Native settings search attachment

This slice starts at `ae542dc622ddd8d555f57e9aace672c8b88a4ad2`.
The N11.4 contract retains SwiftUI's native search control.

The filtered repair uses the public search cell's localized label to identify
one real accessibility button within its field. Independent review found no
required source fixes. Field identity, exact button count, native object
identity, remounts and retained text remain asserted.

`mac-hosted-test/20261031T205446Z-50756`: build passes; test fails.
`matches.count` is zero. Hierarchy diagnosis remains pending.

The next check should record bounded class, role, visibility and match facts.
Keep raw labels, values and object descriptions out of logs. Do not add waits,
retries, custom controls or weaker assertions. This is WIP, without acceptance.
