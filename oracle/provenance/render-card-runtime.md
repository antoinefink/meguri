# Card runtime provenance (U66)

Pinned release: 26.09.3. Prepared source research and observations are preserved by the merged spec/U66 history. This summary records the current evidence and limitations; it does not replace that history.

| Spec scope | Pinned object references | Evidence |
|---|---|---|
| C2/C3/C5/C10 | `59c829c475bfe4400b890358fe84cbe42cd5466d`, `f40ed09dde16d35c8a6884faa1c2cfdc30692eff`, `75024c22627d34e39df2d0bed9f040efa094a9b3` | Document classes, bridge reply format, callback null decision and review page behavior. |
| C4.1/C4.2 | `a8da766adb454b14f45eac4e1115e3c7ffe37610`, `940e7531ce399099cf7bdbe9b427da3cbd8579e8`, `bb2a50e0b8ff308853a9f988e6d9a0b74d3768fd`, `13c2c0ee1f4b3106a2830d112958f000e101c612`, `ad422aef6462c0f0316e25e269bffef603ead14a` | Constants and calculated values. |
| C4 base rules | `c48d5324a1dba2bc99020e099b8d1588f706c4b4`, `676056ba79c972ad858e09f3b7b6a34a676d1ea0` | Reviewer and IO element style behavior; current replay styling instead follows Meguri R483. |
| C6/C7/C10 | `3f9ad675625524b4f6e507f826e38b59e8a42756`, `2d3eaa8c5906e8318b8fa980e0cfa1e6c1c547aa`, `8f63b639a62cf9db772e01ff89af3a987e0ce56f`, `293d83d05df62d382c9316939c8d96f93f040185` | Update order, script completion events, source release, preload budgets and page interfaces. |
| Compiler identity | `d079ac2173a6cf84f454aa40fc656222f4942aec`, `726fa78ebcf74f2b7b732d63f32950cce51b97e4` | The direct stylesheet compiler resolves to version1.102.0. A separate plugin dependency is1.99.0 and is not the compiler for these values. |

Historical evidence used an installed release at26.9.2. Its input trees and dependency records were compared with26.09.3. Later calculations reproduced the numeric results. The later bounded compiler check below now confirms exact output text. Implementers receive neutral facts only; earlier observations do not establish authorship of code.

The exact17 computed color strings are now confirmed by an independent public compiler check. Neutral `specs/render/card-runtime-color-inputs.json` supplies color constants, adjustment channels and expected property/value strings. The tool mounts only those neutral inputs and independently authored calculations: no reference source tree, source history, stylesheet or compiled reference asset. It uses the official1.102.0 standalone compiler archive, SHA256 `82a5b53decbd2ed1d9ef3452e039e3f57ca936f80e96407dd1add1653c282763`.

Black-box result: `build/mg/css-reference/20261019T203044Z-77056/result.json`,17/17 passed,0 mismatches. Input SHA256 `c921b9b53ece1cc7fbb84896a42a546d3ea7adfcddd8209592f9b146a869db9b` matches the current neutral file. The associated `colors.json` contains property/value data only; its SHA256 is `84fbf2f519955d56ca142a0c22dac975a25696e4eeb6424717bf6bb08f6dfdae`. No compiler-generated stylesheet is a source for Meguri implementation. No npm package integrity claim is made by this standalone-archive verification.

Session s-20261019T202103Z-b8f1a55a:

- C2.1 follows the prepared S2 amendment: page at the origin root; bundled assets under the reserved prefix.
- C6.3/SU5 require release of direct video sources and streams. The pinned reference removes child nodes and reloads. D23 records the additional attribute and stream clearing required by R471.
- C6.5/SS12 preserve the previously observed WebKit matrix:13 fetched source forms and17 skipped forms. No fresh platform result is claimed by this intake.
- C7.4 keeps local dependencies ordered through load/error, and caps cumulative external waiting at800ms. Recovery belongs to U70.
- C4.4 follows only current R483 and the original Meguri replay graphic. No superseded graphic is restored.
- R507 and the U66 test wording now match the explicit human null callback decision.

Independent upstream library check: official https://code.jquery.com/jquery-3.7.1.min.js,87533bytes,SHA256fc9a93dd241f6b045cbff0481cf4e1901becd0e12fb45166a8f17f95823f0b1a. The prepared vendor file matches these bytes exactly. Keep its upstream MIT notice. No reference-app asset was used for this check.

## Deferred inline-module diagnostics (Meguri WebKit evidence)

Session s-20261019T202103Z-b8f1a55a. The iPhone simulator probe in the test VM used unchanged literal module text and native scheduling. Its diagnostic observation is recorded in `build/mg/unit-sim/20261019T205137Z-99291/unit.log`, tagged `U66_INLINE_MODULE_DIAGNOSTIC`; the enclosing receipt is `build/mg/unit-sim/20261019T205137Z-99291/result.json`. The module probe passed. The enclosing run failed on a separate C13 listener-registry assertion, so this is not a passing receipt for the full suite.

The log sequence was classic-before, classic-microtask, classic-after, inline-module. The error record held the module message, line3, phase `later`, null index and source `inline`. C6.5/C12/SS20 now describe that result. R522 still requires message, available engine location and card-template data. Only the unsupported promise of an index for every script was removed. The test changed neither module text nor timing. Classic errors and source-script errors keep an index when their script is known. The evidence comes from Meguri's WebKit runtime alone.
