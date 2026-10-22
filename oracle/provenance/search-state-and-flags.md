# Due and buried search predicates

Read only the pinned 26.09.3 checkout. No source or test code copied.

| Neutral section | Pinned source | Behavior established |
|---|---|---|
| search/state-and-flags S1, S2 | `rslib/src/search/sqlwriter.rs`, state predicates | Queue-only selection; queues 2 and 3 compare stored due with elapsed days; queues 1 and 4 compare with current seconds plus learn-ahead; inclusive cutoffs; buried queues -2 and -3; no type or original-due substitution |
| search/state-and-flags S1, S2 | `rslib/src/card/mod.rs`, queue enum | Stored queue numbers |
| search/state-and-flags S3, S4 | Meguri Product Contract R1062, R1063, R1916 and search/query-model Q4–Q9 | External context, dependency declarations, result modes and composition; synthetic verification vectors |

S3's overflow rule is Meguri-defined safe arithmetic over the declared context domain. It does not claim a pinned-client overflow outcome. No oracle or scenario modifications were needed for this prerequisite contract.
