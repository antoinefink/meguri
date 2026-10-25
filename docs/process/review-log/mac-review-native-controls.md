# Mac review controls

Start: `d84c97ec`. The integration batch retains base
`81c207573c20c337c8834e4ae8b4ee8f29603dbf`. No unit acceptance is claimed.

Native run `ui-test/20261025T055547Z-29666` failed three cases because the
journeys still expected the old Mac controls. The approved Mac design has
three count columns and native Study-menu Undo/Redo. Touch keeps its joined
count text and Back control. The repair adds distinct count accessibility
identifiers, labels and numeric values without changing their appearance.
The journeys check exact counts and use the actual menu commands.

Fixer session `s-20261024T174115Z-511e3ad6` exported
`cleanroom/20261025T061713Z-41803`. Independent round 1 found no P0–P2.
Native run `ui-test/20261025T062300Z-46368` passed completion/restart and
ratings/undo/waiting/restart. The keyboard case reached its direct AX probe,
then failed with `trusted=false`, error `-25211`. The VM runner registration
was refreshed and verified in a separate process using frozen products.

Subsequent focused runs `063219Z-53563` and `064116Z-63318` exposed an invalid
added assertion about menu enablement while Close had AX focus. Menu admission
also depends on the window's actual first responder; AX focus alone does not
establish it. Opening a menu interrupted the sequence being tested. Round 3
export `cleanroom/20261025T064753Z-69869` verifies Undo and Redo first.
The Close key checks then run without opening a menu between their steps.
Every focus, card identity, saved-count and restart assertion remains.
Independent delta review found no P0–P2. Production command guards are unchanged.

After rebuilding, trust preflight `064936Z-72651` recorded the new runner's
missing consent. VM registration refresh `065042Z-74365` and separate fresh
trust proof `065217Z-75586` passed. The temporary setup markers and private
input were removed by the existing setup helper.

Focused `ui-test/20261025T065309Z-76774` passed the keyboard case with no skips.
It proves initial keyboard study, native menu Undo/Redo and restored card
identity, Close's question/answer Return ownership, Space dismissal, exactly
one retained answer, three due cards and persistence after restart.
The other two Mac journeys passed on the same application behavior above;
the later test-only edit isolated the keyboard case. The failed broad receipt
remains failed; it is not reclassified by this focused result.

The C17 repair's iPhone `054452Z-8891` and iPad `055000Z-13059` runs each
passed all three journeys, including the AX5 waiting and completion actions.
Snapshot run `snapshots/20261025T065800Z-82230` compared 20 unchanged images
and recorded 80 candidate images across iPhone, iPad and Mac. The capture job
passed; human baseline approval remains pending. Independent visual review
checked 19 images with no P0–P2 findings for review and sample overview.
Actions: see `ui-test` results above.

Antoine's subsequent desktop screenshots reject the current production Home
and sidebar composition: overridden row surfaces, wrapping names, detached
previous-session information and unclear disabled Study state. Those findings
remain open and are outside this terminal-image candidate approval. The full
gate from the original batch base and human visual acceptance remain pending.
