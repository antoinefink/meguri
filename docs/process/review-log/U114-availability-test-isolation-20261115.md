# Availability test isolation

Starting commit: `a099fba0`. Pending integration base:
`1416a520542a8c5daa38a23bfe398da704c3d6c9`.

The interrupted full Swift run `20261115T001757Z-78873` reported one probe
instead of more than one, after 60 ms. The 30 ms lower and 100 ms upper checks
passed. The production deadline starts before dispatch to the shared serial
ControlLane, so queue delay, execution and continuation scheduling consume it.
The earlier log does not identify the particular source of that delay.

Three lane-backed availability tests now join the existing serialized EventTests
suite. Their bodies are unchanged, including all probe counts, errors, generation
checks and 30/100 ms limits. The long cancellation test already shares that suite.
Pure fake-service, clock and capacity tests remain in CollectionServiceTests.
Production is unchanged. Full-package gates still include every test; focused
filters must use the method names or their new EventTests identities.

Filtered author: `cleanroom/20261115T012206Z-7559`.
Independent review: `20261115T012559Z-9316`, no P0–P2 findings. Other suites and
scheduler preemption remain possible; isolation is no guarantee against them.
`swift-test/20261115T012812Z-10300`: all ten selected tests across three suites
passed, with zero failures or skips. The deadline test passed in 31 ms.
The full package and required integration gate remain outstanding; no unit,
P24 item, milestone or release is accepted here.
