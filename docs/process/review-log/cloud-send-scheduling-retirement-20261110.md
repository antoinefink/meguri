# Original sends across scheduling changes

Source starting commit: `33fd4499`; preceding app checkpoint: `02fa2fa7`.
Fixer session: `s-20261110T214025Z-20fa15fd`.
Pending integration batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Contracts: sync/policy P4, P15; sync/cloudkit-records K12.4.

## Cause and correction

The clean MeguriKit run `20261110T213105Z-38198` failed four publication,
adoption and reopen cases when `ConfigureAutomatic` retired an older send's
engine. P4 allows that stop. It changes neither binding nor policy token.
An old send could still be reading its capability or preparing SDK entry,
then report that expected refusal as an app failure.

The adapter now retains exact original instances across capability creation and
metadata reads. That retention supplies no SDK admission. A missing engine at
action start is a no-op only after the original control guard validates.

Only a plain policy `RequestSend` may end without an error when its exact original
instance has stopped for scheduling. It must
be stopping or retired, and have no callback failure. The task must not be
cancelled; the owner must not be quiescing or suspended. Only typed cancellation,
stale-command or authority refusals qualify. The original Core control guard
must still validate, followed by a second owner-state check. An account, binding,
token or generation change still fails. There is no new SDK call, ACK, Saved
result, retry or policy outcome.

An actual send error is retained before later completion reporting. A later
authority refusal cannot replace that original failure and make it eligible
for the guarded-stop case. It changes no exchange or media outcome.

Core also applies P4's no-engine rule before preparing fetch/send capabilities.
It validates the original command first, then returns `NoOperation` when the
binding has no current running engine. A stopped SDK still named by policy
is not a running engine. Paused or otherwise stale guards still fail first.

## Checks and review

The new controlled fixture holds one real issued send before SDK entry. Before
the repair, `swift-test/20261110T214250Z-43459` fails its neutral-result check.
That run also exposed a fixture count that omitted an earlier setup fetch; the
check now compares actual call count at the hold boundary.

`swift-test/20261110T214904Z-49875` passes four selected tests. The new test has
four cases: scheduling, account change, original SDK failure and a stop completed
before capability metadata returns. It checks no extra old-engine network call,
original error identity and the replacement engine's next actually captured,
exactly accounted successful exchange. Existing account/token/generation
enqueue fences and exchange/configuration ordering also pass. Earlier focused
runs `44577`, `45657` and `46705` pass their narrower selections.

App integration `swift-test/20261110T214627Z-48346` is retained as failed:
36 of 38 tests pass. It exposed the metadata-read window above, and one Saved
removal fixture whose hold was outside event delivery. No assertion was removed.

That fixture now holds the unchanged Service reply inside delivery, after the
actual `eventWillDeliver` and before Core handles the result. The later edit's
pending updates therefore use the existing handler barrier. A new assertion
checks its pending version stays unchanged at that boundary. The default hook
is unchanged. `swift-test/20261110T215038Z-50988` passes all seven selected
original-ACK, negative-ACK, automatic-send and live-resume tests.

Combined run `swift-test/20261110T215212Z-52519` remains failed: 42 of 43 tests
pass. It exposed the no-running-engine capability window fixed in Core above.
Its Saved-removal assertion passes with the corrected delivery hold.

The first Rust fixture run `core-test/20261110T215451Z-54584` lacked a pending
edit and confirmed root. After adding those actual setup operations,
`core-test/20261110T215550Z-54918` fails the intended post-stop capability check.
With the Core correction, run `56237` passes all 94 affected FFI policy tests.
Intermediate run `55547` remains failed: six tests used a setup helper that
required every incidental network command to enqueue. That helper now asserts
agreement between capability preparation and queue admission, including zero
queue calls for `NoOperation`. The tests' selected-command and guard assertions
are unchanged.

Native run `swift-test/20261110T220047Z-57540` passes the four selected Swift
tests against the rebuilt Core. Run `swift-test/20261110T220419Z-58820` remains
failed: 42 of 43 pass; its positive earlier-automatic-ACK fixture had the same
outside-delivery hold. That positive hold now also runs inside delivery, before
the unchanged original result reaches Core. All proof assertions remain.

Final combined run `swift-test/20261110T220653Z-60386` passes all 43 affected
review, media, publication, peer-adoption, original-ACK, live-resume and reopen
tests. `core-lint/20261110T215948Z-56571` passes formatting, Clippy, layering and
the endpoint-symbol check. Privacy `56870` and similarity `57188` pass the full
repair before the last positive-fixture adjustment; that adjustment and final
record are checked again before commit.

One further source-review case should be controlled before acceptance: a send
requested after Runtime registration but before the replacement native factory
returns. Core has an instance id then, while the adapter may have no ready
native object. The current tests cover a stop before capability metadata,
not that distinct creation window. No pass is claimed for it.

Antoine requested direct work without other agents. Local review checked
original identity retention, guard revalidation, error precedence, cancellation,
actor isolation and the unchanged assertion limits. Independent review, a full
batch gate and the real signed iCloud round trip remain pending. No Unit trailer
or release acceptance is claimed.

## Native engine creation and final SDK validation

Starting commit `78e67b15`. A registered engine can still be waiting for its
native factory. The adapter revalidates the original command and returns without
an SDK call if it has no native instance for that command. The focused test
holds the replacement factory, checks both unchanged and changed account
authority, and checks that the ready replacement can run its own Sync Now.

Run `swift-test/20261110T221138Z-63037` exposed the factory gap. Run
`swift-test/20261110T221205Z-64085` passes the six selected native tests after the
adapter correction. App run `swift-test/20261110T221311Z-65155` passes six of
seven tests. The remaining test reaches the final SDK guard while the engine is
stopping. The FFI now preserves its typed authority error, so the existing
scheduling path can check the original control before treating it as a no-op.
An SDK error or a change of account, binding or generation still fails.

Rust run `core-test/20261110T221542Z-67569` passes the typed error check and 283
other tests. Five asset worker tests fail because the default unit-test build
compiles out the calls to their installed observers. Enable those three calls
under `cfg(test)` as well as the existing test-hooks feature. No test assertion
or timing limit changed. Run `core-test/20261110T221910Z-70332` passes all 289
selected FFI sync tests. Run `core-lint/20261110T222225Z-70916` passes.

Final native run `swift-test/20261110T222245Z-71219` passes all six selected
tests against the rebuilt Core. App run `swift-test/20261110T222356Z-72511`
passes the previously failing fixed-edit test and eight other tests. Two
failures remain. The manual original-ACK case confirms the durable revision
and handler return but lacks the original SDK pending-removal receipt. The
phone restoration test stops before adoption, during its source publication.
Its publication work reports changed authority with the recovery step still
at `sent`. Both failures are retained. The next checks must identify the
pending-list change and the publication authority change at their respective
boundaries. No full gate or signed round-trip pass is claimed.
