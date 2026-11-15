# Accepted aggregate authority contract

The scoped S2.5 exception and its N5.2/G7 references are Approved 2026-11-15 under R2066. Author: `s-20261115T194932Z-6e5ba736`. Independent reviewer: `s-20261115T195629Z-c275b002`.

The exception preserves existing acceptance of exact incoming note-type aggregates. It supplies no ordinal map or cloud authority. Other writes, removals and type-change pairs remain guarded. Existing valid authority is tried first; validated missing maps use the existing collection choice. Capture loss, reopen and scans cannot create authority. Errors retain their own classes, and pending work stays preserved.

The author prepared the text and its tests. Independent review found no P0–P2 findings. It checked the exception, recovery, tests and Status scopes. The integrator read the full patch and report. Other approvals stay unchanged.

The first wording hit the privacy matcher. The author rephrased two paragraphs; independent comparison found no change in meaning. The guard is unchanged. Final reviewed draft SHA-256: `1e2a329bc2a97ef3cf788d79731d9e94cceb7439e2923d8a5ac0e328c883ea42`. Approved patch SHA-256: `6b43de5e5da2880e17a42417534258886d31a4073850aad686a3d7a15ada8ef4`. Approval changes only three Status lines in that final text.

The independent report remains at `build/cloud-resume-diagnosis/U113-incoming-aggregate-contract-independent-review-20261115.md`. Filtered source audit `cleanroom/20261115T194110Z-82472` found existing refusal paths and missing regression coverage. It ran no tests.

R2066 requires a written, reviewed contract. The Status convention permits approval after clean independent review. Owner-only setup actions are unchanged. This technical clarification adds no server feature or UI.

Implementation and all new focused regressions remain pending. The separate incoming note-mid case in N6.3 remains held and unexempted. No Unit, P24, gate or live-device acceptance is claimed.
