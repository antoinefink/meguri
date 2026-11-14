# Recovery policy decision — 2026-11-14

Antoine's direction:

> Yes we should always try to make this fully automatic.
> When not possible I guess hte user should say if they want to keep the local or the remove version like they do with Anki?

Here, remote means the iCloud collection.

Recover automatically whenever the retained evidence supports safe recovery.
When the ambiguity cannot be resolved, offer a choice of this device's
collection or the iCloud collection. Reuse the V13.5 choices and V14/V15
replacement flows in `specs/sync/cloudkit-recovery.md`.

Before confirmation, show the direction, relevant counts and unsent work.
Keep the safety backup when replacing local content and the previous iCloud
collection when replacing cloud content. The choice selects a collection;
it does not merge two collections. Cancel preserves the unresolved state.

The field-map ambiguity in U113 uses this fallback rather than guessing from
matching rows. Ordinary crash recovery remains an automatic-recovery target.
The retained native-intent draft still needs a complete contract and review.
Implementation, tests and the required evidence remain open. Existing repair
round limits remain in force.

Antoine separately permits deleting or replacing Meguri's current Mac and
iPhone test data: “Yes you can always delte the data on the Meguri app”.
This records local app-data permission; the other trial permissions stay as
recorded in its packet.
