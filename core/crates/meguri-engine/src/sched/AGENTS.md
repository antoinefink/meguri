# Scheduler engine

- Native lifecycle checks use `automatic_day_query`: no synced empty-check bookkeeping. `day_query` retains the explicit compatibility-query semantics; never call it from open, foreground or idle hooks.
- A bury operation must establish today's `lastUnburied` in the same transaction before writing buried queues. An empty automatic check keeps only an in-memory marker; it cannot protect a same-day reopen. See `specs/sched/day-clock.md` D8.
