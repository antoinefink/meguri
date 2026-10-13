-- Calibration fixture for the similarity scanner: queries over the card and
-- note tables, written for Meguri from the file-format facts alone.

-- Learning cards that are due now, soonest first.
SELECT c.id, c.nid, c.did, c.due
FROM cards AS c
WHERE c.queue IN (1, 3)
  AND c.due <= :now_secs
ORDER BY c.due, c.id;

-- New cards of one deck in position order.
SELECT c.id
FROM cards AS c
WHERE c.did = :deck AND c.queue = 0
ORDER BY c.due, c.ord
LIMIT :cap;

-- Notes with no cards left (after a template was removed).
SELECT n.id
FROM notes AS n
LEFT JOIN cards AS c ON c.nid = n.id
WHERE c.id IS NULL;

-- Count review cards per deck for the deck list.
SELECT c.did, COUNT(*)
FROM cards AS c
WHERE c.queue = 2 AND c.due <= :today
GROUP BY c.did;

-- Undo a suspension on every card of the given notes.
UPDATE cards
SET queue = type, mod = :now_secs, usn = :usn
WHERE queue = -1 AND nid IN (SELECT value FROM json_each(:note_ids));
