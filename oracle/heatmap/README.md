# Heatmap add-on oracle image

AGPL quarantine (`oracle/LICENSE`; the add-on is AGPL-3.0, its LICENSE installed with it). Oracle maintainers and spec writers only. The Meguri oracle plus the Review Heatmap add-on 1.0.1, for the observations `heatmap_totals` and `heatmap_report` (`specs/conformance/stats-ops.md` H1, H4, H11). The driver module is `driver/observe/heatmap_addon.py`, in the base image's driver but loaded only here (`ORACLE_EXTRA_MODULES`). Every other kind runs here as in the base image, so a selection that holds a heatmap scenario can run on this image whole.

- Source: the public archive of tag v1.0.1 of github.com/glutanimate/review-heatmap (commit `5ab7964…`), pinned by the archive's sha256 in `Dockerfile` and by a digest over every installed file (its LICENSE and `src/review_heatmap/`) in `driver/observe/heatmap_addon.sha256`. That digest names no path: some upstream file names match the private scan's tokens, so never list them in the tree. The build and the module's import refuse any other file set. Never install it from a local copy or from AnkiWeb (R1876).
- A pin change changes the base driver hash (the pin file is in `driver/`); re-record every golden that observes either kind.

## By hand (no tools/mg job yet)

```
tools/mg oracle build                                    # prints meguri-oracle:<tag>
cd oracle/heatmap
docker build --build-arg ORACLE_BASE=meguri-oracle:<tag> -t meguri-oracle-heatmap:probe .
docker network create --internal mg-heatmap-probe
docker run --rm --init --network mg-heatmap-probe --tmpfs /scratch:exec,mode=1777 \
  -e TZ=UTC -e TMPDIR=/scratch -e ORACLE_SCRATCH_ROOT=/scratch \
  meguri-oracle-heatmap:probe selftest test_heatmap_addon
```

`harness record` can run there too for a probe, with `--goldens` in a scratch directory. Never write `conformance/goldens/` that way: goldens come only from `tools/mg oracle record`.

## What a tools/mg job needs

- `oracle record` and `compare` (and every caller of them: `conformance --live` and its re-recording): after `mg_oracle__build`, when any selected scenario observes `heatmap_totals` or `heatmap_report`, build this Dockerfile (context `oracle/heatmap/`, `--build-arg ORACLE_BASE=meguri-oracle:$MG_ORACLE_TAG`, network allowed for `ADD`) as `meguri-oracle:$MG_ORACLE_TAG-heatmap-<16 hex of the Dockerfile's sha256>` and pass that tag to `mg_oracle__harness`. The compose `oracle` service already runs `meguri-oracle:${MEGURI_ORACLE_TAG}` with `pull_policy: never`, so no compose change is needed. Report both tags in the result's `image` detail.
- `oracle test`: build the same image and run `selftest test_heatmap_addon` in it after the base run (the base run skips those tests).
- `--platform apple`: refuse heatmap scenarios (no native add-on; their values are integers and dates, so they need no overlays).
- No user step: the source is public. The `heatmap-addon` step and `MEGURI_HEATMAP_ADDON` that `stats-ops.md` H4 once named are not needed.
