# Delete tags and note types

Expose the collection's existing deletion operations in Browse and Note Types on Mac, iPad and iPhone. Use the tag row's native actions and the note type list's row actions. Offer the same commands from compact layouts. Keep the ordinary filter and note type selection actions intact.

1. Wire Browse tag deletion to the existing collection-wide job in `tags/tags` T9. The selected branch and its registrations disappear, notes and cards survive, and collection Undo restores the branch. Test a parent with children and a similarly named sibling.
2. Wire Note Types to W12 and `notetypes/structure-changes` S12. Show the actual note and card counts, require deliberate confirmation, protect the only remaining type, and use the existing backup and Meguri Sync structural job. Reconfirm greater losses. Exercise cancel, a populated type, empty and last types, and keep fresh selection after changes.
3. Use synthetic native journeys to inspect the visible commands, confirmation and outcomes. Review the source independently, run the affected verification jobs and publish the screenshots in one PR against master.
