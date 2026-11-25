# Browser transport

- Browser admission is service-owned across routers; status/health bypass it, native families never take it. Acquire with try only, before body collection, and move the permit into the worker before spawning.
- Retain the incoming Body with the response and send HTTP/1 Connection: close on every browser outcome. Dropping an unread receiver earlier can enable Hyper background draining after admission ends. No drain tasks.
- Shutdown closes ingress before graceful HTTP shutdown, then awaits admitted workers/responses; never time out/drop the server future or release root ownership while work remains.
- Parsed JSON Value is preflight only. Typed schemas must deserialize original bytes to preserve duplicate-field errors. Unit-only pending bodies/deadline signals are the capacity proof; never add a live exhaustion driver.
- Restore caller liveness belongs to `handle`, not its detached ingress worker. Cancellation and account restore-lease acquisition share one synchronous lock; after admission, retain ingress/root ownership until the worker and response actually end.
