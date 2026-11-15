# Candidate test boundary

Starting commit: `defdabcd9`.
Spec-writer session: `s-20261115T090224Z-47bf0815`.

The parent wrote the original candidate invocation contract from the agreed
plan. The reference-harness author checked it against the neutral input format;
the parent reviewed that format in turn. The fresh final harness review also
includes the contract and its implementation. Final source review and runtime
candidate evidence remain pending in the owning units.

The contract permits only a job-owned local peer, separate protected synthetic
credentials and bound build/execution evidence. It contains no reference code
or private inputs and establishes no client or release acceptance.

Document-only verification uses review and privacy/static checks. Initial
privacy receipt `build/mg/private/20261115T145450Z-24660/result.json` flagged two
prose lines. They were reworded without changing the contract; the follow-up
`build/mg/private/20261115T145537Z-25223/result.json` passes. No new exception
was added.
