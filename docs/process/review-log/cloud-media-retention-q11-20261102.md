# CloudKit media retention Q11 proof

Status: partial U113; no Unit or milestone acceptance.
Component start: 39eddde7b0e54eb215f67c79321ff1d00cfb3435.
U113 start: 1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a.
Integration base: 1416a520542a8c5daa38a23bfe398da704c3d6c9.
Implementer session: s-20261101T235351Z-264d9cd0.

Q11's first row covers media cleanup through the real core and a generated
service. In cloud_media_cleanup.rs, a changed file can hit quota before its
name record is saved. The added case must prove failure at that record.
Existing coverage checks the old blob for 31 days, then the 30-day delay
after the name ACK changes; invalid files and fetched names are also covered.

This component adds explicit changed-name quota evidence after making the
new blob available. Keep existing upload-quota coverage, daily drains, cutoff
and fetched-name assertions. No production defect or live SDK result is claimed.
Filtered authoring starts from clean39eddde7, original20312, one test file only.

Filtered70296 completes with one new case. The same name is first uploaded
with the new content, then ACKed with its old content again. The test holds
the original changed-name request, verifies the blob and its successful save,
sets quota, and checks that exact request's error and unchanged prior ACK.
Independent review finds no P0–P2 issues and confirms existing coverage stays.
Core72126 passes19/19 without skips; privacy72394 and similarity72732 pass.
Lint73054 reports repeated enum suffixes. A naming-only fix remains required;
no lint suppression or test-limit change is permitted.

Fixer624641 exports75118 from clean52e39eaa. Only three enum names and their
five uses change. Independent comparison confirms identical behavior and
assertions. Final lint75689 passes4/4, selected case76176 passes1/1 and
privacy75688 passes. The earlier19-case pass remains valid for the unchanged
cases; no limits or assertions change. The initial sandboxed fixer start74677
cannot reach the runtime; authorized75118 completes with its boundaries checked.

The reviewer maps the exact first P24 retention item to the passing suite:
empty replacement for unsyncable files; upload, removal and changed-name quota;
the fetched-name builder and retained-batch checks; old-mark expiry; the
30-minute/2-hour stale-present repair; and ready, refused, intent and format
inbox protection. The spec-writer removes only that P24 item and keeps the
mandatory Q11 test text with this evidence. Independent review clears that
scoped closure. It does not close full Q5/Q6/Q11, signed media, U113 or a gate.

Final code/spec/log privacy and exact-source similarity76861 pass. The full
integration batch remains open; its original gate and other failures remain.
Final static77380 passes3/3 on the committed code and specification closure.
