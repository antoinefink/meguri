# Browser dependencies

Direct runtime dependencies are React 19.3.1 (components and state) and
React DOM 19.3.1 (browser mounting). The resolved runtime graph contains only
those packages and scheduler 0.28.0. Adapted shadcn components are local source;
their MIT notice is retained in THIRD_PARTY_NOTICES.txt.

The browser UI self-hosts the unmodified official Roman complete variable font
from `@ibm/plex-sans-variable@0.2.0`, IBM commit
`ceee82fa88781b8310b198fd302480efaeac609e`. Source archive:
`https://registry.npmjs.org/@ibm/plex-sans-variable/-/plex-sans-variable-0.2.0.tgz`.
The supplied seed was matched against npm SHA-512
`S8xew9E8CPPKFvitbu2gDtwNFVWwwhClheaIBeykDYg7DrVK3cV5Xq5MjMrWPP5Os0xyiRFjMOmtGBLzz3urTQ==`.
Archive entry `package/fonts/complete/woff2/IBM Plex Sans Var-Roman.woff2`
is retained as `src/IBMPlexSans-Variable.woff2` (230368 bytes), SHA-256
`e978248b6b56da9e372975322a98dd9a51135d1b375e32ed6d5ca69f3aab792d`.
The full exact `package/fonts/complete/woff2/license.txt` is appended to
`THIRD_PARTY_NOTICES.txt`, SHA-256
`91c25c350d3cac39da2736d74f7ba37ef648f5237a4e330a240615bc8d8c4360`.
Both upstream payloads retain their exact bytes. Vite emits a hashed same-origin
WOFF2 URL compiled into the Rust artifact with MIME `font/woff2`; no local font
lookup, external font request or npm dependency is added. Existing source,
notice, manifest membership and content-hash checks also cover this asset.

Direct development dependencies: TypeScript 7.0.2 checks types using its native
CLI; @types/node, @types/react and @types/react-dom supply declarations; Vite
builds the browser assets; Tailwind CSS and @tailwindcss/vite compile styles;
@playwright/test drives the actual Rust HTTPS fixture; @axe-core/playwright
audits accessibility. No programmatic compiler API is needed. The TypeScript
direction follows the [current native CLI production release announcement](https://devblogs.microsoft.com/typescript/announcing-typescript-7-0/)
and [TypeScript 6 maintenance for critical fixes](https://github.com/microsoft/TypeScript/issues/62963).

The manager's locked Linux installation has 114 lock entries including the root,
52 installed packages including bundled packages, and 61 absent platform-optional
packages. The exact generated lock and dependency graph are unchanged. The
manager reported no deprecated markers or non-registry URLs. Full locked-graph
license collection passed its missing-notice gate with 113 metadata entries;
npm audit reported zero vulnerabilities at every severity at that historical
installation. The current report has four high findings, assessed below.

The notice scanner retains complete top-level license, copying, notice,
THIRD-PARTY-LICENSE, ThirdPartyNotice and CopyrightNotice texts. Every installed
package requires notice evidence. Five exact package/version/path supplements
use SHA-256-pinned files under notices/:

- @rolldown/binding-linux-arm64-gnu 1.2.19: official rolldown 1.2.19 archive
  LICENSE and THIRD-PARTY-LICENSE, matching version, repository and directory.
- @rolldown/binding-linux-x64-gnu 1.2.19: the optional build-only AMD64 binding
  uses the same two pinned official rolldown 1.2.19 notice files. Its official
  archive matches the existing lock's SHA-512 and official metadata, contains
  only package.json, README.md and the native binding, and omits license text.
  The same-version rolldown package matches its repository URL and directory;
  both notice files match the retained files byte-for-byte. This supplements
  tooling inventory only; runtime notice selection is unchanged.
- @tailwindcss/oxide-wasm32-wasi 4.3.5: official @tailwindcss/oxide 4.3.5 MIT notice.
- Bundled @napi-rs/wasm-runtime 1.2.2: official repository root LICENSE at its
  matching package gitHead 5b344c367eeed81d1917f7c4d8e65b1adc16de12.
- Bundled @tybys/wasm-util 0.10.3: metadata declares MIT, but the old archive and
  gitHead omit LICENSE. Official immutable commit
  51836d7bf26fb7f43b05dd6477b3ca4260c945cf supplies Toyobayashi's MIT notice.
  The helper explicitly credits Node lib/path.js; the provided exact Node/Joyent
  MIT excerpt is retained too. No Node implementation code is included.

Acquisition URLs, archive integrity and immutable notice hashes are retained in
notices/acquisition-records.json; the manager verified acquired npm archives
with SHA-512. Bundled packages are covered by the parent oxide WASM archive,
not independent lock integrities. The generated full-graph inventory records
that coverage and supplemental qualifications. Full-graph notices are tooling
evidence, separate from distributed-code notices. The distributed notice set
includes React, React DOM, scheduler and shadcn, plus the selected installed Vite,
Rolldown and Tailwind CSS license files: the emitted preload helper, interop
helper and preflight CSS copy code from those build packages. Rolldown's
THIRD-PARTY-LICENSE includes the Rollup and Evan Wallace notices. This does not
change the three-package npm runtime graph or ship the full toolgraph notices.

The manager measured runtime-notices.txt at 106321 bytes across six selected
sections: React 1090, React DOM 1090, scheduler 1090, Tailwind 1073, Rolldown
3514 and Vite 98197 bytes (the Vite source license is 98196 bytes). For the
Vite 8.3.8 DOM-only preload helper, the scanner now selects the exact complete
1163-byte core MIT section before `# Licenses of bundled dependencies`.
Its copyright, permission and disclaimer remain intact. The manager verified
the exact npm archive's SHA-512; acquisition-records.json records that identity,
the full source SHA-256 and the core extract SHA-256. The scanner checks those
identities and hashes; the entire Vite license remains in full audit evidence.
Other runtime notices and the full toolgraph inventory are unchanged. New
runtime-notice and compiled chunk sizes await the manager's build measurement.

TypeScript 7.0.2's root NOTICE omits its bundled vscode-jsonrpc license. The
scanner now reads exactly vendor/vscode-jsonrpc/License.txt from that installed
development package, pinned to SHA-256
ec9ee83580841e8eb687aca9867f221503809ba6426c7f876ede17d91b9fcfd0
and the existing exact archive lock integrity. Its full text is retained in
tooling notices and its path in the inventory, outside runtime banners. This
embedded-notice collection remains part of the manager’s installation audit.

The manager reported that the previous source passed strict TypeScript 7 checks,
all five Node units, the Vite production build, the default Rust build with
prepared assets and one real HTTPS Playwright journey with nine axe audits and
nine PNG captures. The actual server binary had SHA-256
3717f2c8e29076952afea42862f33881ce0d3cf907351c7d25eed1c5d3bac3d3.
These results precede the current fixes.

The manager previously supplied a compiled-notice failure: both emitted JavaScript
chunks contained none of Copyright, Permission or shadcn, despite output.banner.
The earlier zero advisory count and full-graph notice collection did not prove
compiled license retention. The latest manager checks passed postBanner and
full-notice byte assertions, strict TypeScript 7 checks, all five Node units
(5 selected/completed/passed), the actual prepared Rust build and the real HTTPS
journey (1 selected/completed/passed, zero failed/skipped/flaky), with nine axe
audits and nine captures. These passes precede this bounded cleanup.

The build now uses output.postBanner after minification, following the
[Vite build configuration](https://vite.dev/config/build-options.html) and
[Rolldown output options](https://rolldown.rs/reference/Interface.OutputOptions).
Each production build collects distributed notices from its actual locked
installation. Before embedding, embed.mjs requires every emitted JavaScript
chunk to contain the full supplied shadcn and distributed notice bytes; missing,
empty or stripped notices fail. The Vite configuration also compares every
adapted source hash in upstream-records.json with the actual source bytes.

The focused browser audit records computed font stacks, bounded canvas widths
and Chrome platform fonts when CDP is available. Captures scroll to the top;
only a focused skip link moves focus to the existing main region. No errors or
product state are hidden. The ordinary image probe records fc-match results for
system-ui, ui-sans-serif and sans-serif, or explicitly records an unavailable
fc-match executable. The fixture's computed CSS uses the standard sans stack,
but CDP reported LiberationMono-Bold; all three fc-match requests resolved to
LiberationMono while repeatedly reporting `No writable cache directories`.
The image and runner set XDG_CACHE_HOME=/tmp/xdg-cache, created before runtime
font resolution. The manager confirmed the cache warnings stopped, while
generic matches remained LiberationMono. The fixture image now adds Debian
fonts-dejavu-core alongside fonts-liberation; the existing image probe also
records up to 40 unique font-family entries. Actual resolution awaits manager
verification. Those system-font observations precede the local IBM Plex Sans face.
The existing mount, read-only root, capability and network limits are retained.

The unit/check lanes retain Node TAP in /out/unit-results.tap; the browser lane
retains Playwright JSON in /out/browser-results.json. The local report binder
publishes selected, completed, passed, failed, skipped and flaky counts from
those reports before handling a nonzero process exit. Missing, empty,
inconsistent, retried or nonpassing reports fail; unit cancellations and todo
results also fail. Build/image/typecheck/dependency lanes remain process checks.

After this cleanup, the manager must run notice collection and
production asset-byte assertion, prepared Rust build and embedded-asset units,
real HTTPS journey/axe/captures and report binding checks. The prior 95 API tests
do not cover embedded assets. Known-delta review follows these fixes; no further
broad review or final acceptance is claimed. The official documentation links
were supplied with the fix request; DNS prevented fetching them in the filtered
runner. No Docker/MG jobs or installations ran in that runner.


## Fixed frontend build applicability assessment

The versioned `scripts/audit-assessment.json` records the exact npm report for
GHSA-fc5q-39xc-vjpf, source 1247710: picomatch 4.0.7 and its propagated fdir
6.5.0, tinyglobby 0.2.17 and Vite 8.3.8 findings. All four are development
packages. GitHub's advisory remains active HIGH with no withdrawal date and the
official CVE remains PUBLISHED. Snyk independently revoked its entry and the
upstream maintainer disputes the untrusted-pattern classification. There is no
published patched release. None of these facts establishes universal withdrawal
or a fixed dependency. The suggested Vite 6 downgrade is unsupported here.
Package manifests, lock bytes and dependency versions remain unchanged.

This assessment applies only to the recorded trusted frontend build boundary.
Independent read-only review of all 42 baseline copied web inputs found a closed
SPA import graph: user, account, collection and card-CSS inputs do not reach the
build-time matcher. Production runs the Rust executable and embedded static
assets; Node, npm and node_modules were absent from the historical packaged
inventory. A small Vite DOM preload helper is shipped, with its notice retained;
that does not expose the build-time matcher to browser input. The supplied
baseline is source 8733b6eae233e68fc45feb5f11a1a84a43e4bdf5; historical package
qualification belongs to source 224f7d8e03fcee5a7593bdb2e70b935442010d78.
Neither history nor this assessment proves fresh package execution passed.

The helper compares every copied input file and directory using copy-source's
root-only generated-directory omissions. Additions, removals, nested inputs,
links and nonregular inputs fail. Exact lock bytes and the entire parsed lock
bind all graph records, versions, integrities and development flags. MG stages
only the 14 fixed build/packaging control files after checking their actual host
bytes, and the container checks that staged set again. All baked scripts must
match mounted source, so these script changes require the ordinary
`sync-web-image` rebuild. MG pins and records the actual selected tool image ID.

The only self-description exclusions are the exact helper and assessment file
contents. Their paths remain in the enumerated input set; their bytes must match
the executing image and are included in emitted source and policy identities.
These two files are the explicit trust root, requiring independent exact patch
review and source/image binding. No file supplies an owner approval, caller
policy or arbitrary advisory selection. Later source, graph, control or report
changes require another review; there is no automatic freshness assumption.

Install, explicit resolve, audit and browser installation share the decision.
Raw npm JSON and stderr bytes, actual exit and tool versions are retained beside
`npm-audit-decision.json`. Only deep equality with the whole recorded report and
exit 1 yields `assessed-inapplicable`, retaining four raw highs. Exact exit-zero
clean data yields `raw-clean` only with the same source/graph/control boundary.
Unknown, partial, contradictory or malformed reports, diagnostics, new advisories,
changed tools or unexpected exits block. A different resolved lock is retained
for review but cannot acquire this disposition. This intentionally changes the
former unconditional raw-exit rule; it does not change Rust advisory policy or
make old failed receipts pass. Notices, licensing, cache/lock checks and zero
retries remain in place. The owning MG receipt exposes the decision, control
identity and image identity for downstream packaging evidence.

This bounded clean-room implementation has not run new tests, rebuilt its image
or qualified a fresh package. The orchestrator must run focused MG negatives,
independent exact patch/negative-behavior review, the ordinary image rebuild,
all four audit paths and downstream package/runtime proof before acceptance.

### M8 bounded input refresh (2026-11-22)

`sync-web-verify/20261122T124642Z-96725` failed because the packaging-control
bytes differed from this assessment. The M8 candidate changes browser API,
bootstrap/routing, connection/consent copy, policy, API fixtures and the adapted
source hash records. It adds `src/media-upload.tsx` and
`tests/browser/media-upload.spec.ts` to the sorted complete file map. The upload
screen now shows the signed-in identity and reuses account logout while keeping
an unconsumed refused link in memory for switching accounts. A targeted synthetic
browser fixture covers wrong-account refusal, logout, correct-account sign-in
and successful use of the same ID; existing browser assertions are retained.

After those inputs settle, this refresh updates only their exact byte lengths
and SHA-256 values and those of already-enumerated changed packaging controls
(`server/Dockerfile`, `server/src/httpapi/mod.rs`, `server/src/webassets/mod.rs`
and `server/tools/package-inventory.py`). The 14-control set, tool versions,
lock/graph identities, full advisory report, trust-root exclusions and complete
tree checks remain unchanged. No JavaScript dependency changes. The new screen
imports existing application modules only; runtime upload/account input does
not enter the build-time matcher. The adapted app hashes are refreshed after
the logout wiring changes, preserving the upstream input hashes and notices.

This is a bounded source/evidence correction, not a passing run. No build,
browser execution, dependency audit or image rebuild ran here. Retain the red
receipt; independent exact review, an ordinary `sync-web-image` rebuild and the
required web/package checks must bind the new bytes before acceptance.

### M8 automatic media transfer (2026-11-23)

The machine upload replaces the browser picker added above. The web
input inventory drops the picker and its browser tests. The server
HTTP tests cover the binary POST and note attachment without a browser.
The inventory records the exact browser input and packaging-control
changes. The dependency graph, upstream inputs and notices stay pinned.

### Backup web verification fixes (2026-11-25)

History rows are validated before rendering; restore succeeds only with a null
result. An unknown result requires an explicit history refresh before another
restore. Identity and storage reads use request ownership to reject stale
answers. Focused regressions cover lost responses after a simulated commit,
session retirement and slow storage; confirmation captures use the region.
The complete web input inventory and adapted app hashes are refreshed from
the final bytes. Dependency, advisory and trust-root rules are unchanged.
MG execution, image rebuilding and independent exact-content review remain
with the outer integrator; this refresh records no new passing evidence.
