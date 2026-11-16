# Browser dependencies

Direct runtime dependencies are React 19.3.1 (components and state) and
React DOM 19.3.1 (browser mounting). The resolved runtime graph contains only
those packages and scheduler 0.28.0. Adapted shadcn components are local source;
their MIT notice is retained in THIRD_PARTY_NOTICES.txt.

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
npm audit reported zero vulnerabilities at every severity.

The notice scanner retains complete top-level license, copying, notice,
THIRD-PARTY-LICENSE, ThirdPartyNotice and CopyrightNotice texts. Every installed
package requires notice evidence. Four exact package/version/path supplements
use SHA-256-pinned files under notices/:

- @rolldown/binding-linux-arm64-gnu 1.2.19: official rolldown 1.2.19 archive
  LICENSE and THIRD-PARTY-LICENSE, matching version, repository and directory.
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
verification. The product font stack is unchanged; the user's Mac renders sans normally.
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
