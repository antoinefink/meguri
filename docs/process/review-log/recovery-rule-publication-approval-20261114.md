# Exact recovery rule approval — 2026-11-14

Antoine approved the proposed exception: “Approve this exact public rule”.

Literal UTF-8 SHA-256, without a newline:
`1223e97157bb7d43dc1ce067b92443dd37014a8424b1c7af45c6e3851d5cca06`.

The reviewed text and proposal are retained locally in
`build/cloud-resume-diagnosis/U113-public-rule-exception-proposal-20261114.md`.

Permit only this complete case-sensitive literal through the existing fixed
public-span mechanism. Keep every other match, including crossing matches,
checked. Retain decoded-variant coverage and the private corpus. No path, file,
prefix, bare-word or editable-list exemption is approved. Verify synthetic
positive and negative cases and obtain independent source review.

The approval covers this one rule and its narrow guard change. It supplies no
sync test, Unit, P24, gate or release acceptance.
