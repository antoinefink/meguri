# Xcode test cache isolation

Start: `502432b2`.
Full integration base: `81c207573c20c337c8834e4ae8b4ee8f29603dbf`.

Xcode check `20261025T095204Z-27977` failed five device phases after its Mac and Debug checks passed. Release compilation removed 138 retained Profile product paths from the shared test cache. The generated test metadata still matched its proof, but the strict product inventory did not. The guard correctly blocked further test transport.

The repair gives Profile and Performance builds a separate test cache. Every invocation checks both known roots before any test build or product copy. Product inventory, generated policy, proof identity, pending-state handling and no-op validation remain strict.

Independent reviews of both exports found no P0–P2 findings. The first exact Docker run passed 69 cases and failed one stale fake-SSH matcher. The matcher now covers both cache roots; its queued-state and no-execution assertions remain unchanged. The final exact run `20261025T104318Z-95227` passed all 70 cases without skips. The temporary MG test harness was removed.

The broader filtered-copy selftest failed because that environment intentionally omits agent configuration. It is retained as a failure.

Full gate `20261025T092407Z-37753` finished with this Xcode failure as its only technical blocker. Its T1 checks were skipped after that failure. All other executed checks passed. Failed Xcode logs were copied under that run’s `retained-logs`; the invalid cache was archived in place without reuse or deletion. Apple verification of the repaired source remains pending.

The integrated patch passed 70 selected main-checkout selftests in `20261025T110701Z-26974`, privacy scan `20261025T110637Z-26596`, and similarity scan `20261025T110702Z-27058`. No unit acceptance is claimed.
