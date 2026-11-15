# Joined cloze report clarification

K19.2's scoped wording and M9.11's reference are Approved 2026-11-15 under R2066. Author: `s-20261115T194932Z-6e5ba736`. Independent reviewer: `s-20261115T195629Z-c275b002`. Starting commit: `f15de7bb`.

Primitive reports keep their existing canonical rules. A retained joined report preserves surrounding ordinal movement, including on Undo; Redo keeps its original report. Standard endpoints keep exact destination arity. Checked evidence must prove the inverse; it cannot invent origins or accept an approximation. No new durable ledger, replay, wire map or card enumeration is required.

The two executor obligations check actual cards and reports through two Undo/Redo cycles: a cloze-to-standard change followed by a template swap, and a three-template standard type moving through cloze to a two-template standard type. These are requirements, not executed evidence.

Independent review found no required P0–P2 findings. The integrator read the entire patch and review. Approval changes only the two scoped Status lines; other approvals and authority, error, WAL and COMMIT fences stay unchanged.

Reviewed draft SHA-256: `1b3e43980bb706424576eb94e04c1b6d9bb679513d43c5539b2fb5575621b34d`. Approved patch SHA-256: `f4c6184f928ad923b4762926a6c8f3d4b83faea892c59e3907d816d12fb30ba3`. The full independent record remains at `build/cloud-resume-diagnosis/U113-joined-cloze-inverse-independent-review-20261115.md`.

No implementation, test pass, Unit, P24, integration or release acceptance is claimed. Remaining framework work and live peer evidence stay open.
