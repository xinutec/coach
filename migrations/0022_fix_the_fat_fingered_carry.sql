-- The 3 530-second farmers walk. Named in `domain::MAX_HOLD_S`'s comment as the
-- reason a plausibility ceiling exists at all — but the ceiling only guards the
-- write path, so the row that prompted it is still in the log and still feeds
-- `ability::better_carry`.
--
-- What it should say is recoverable from its neighbours. On 2026-07-16 at 12 kg:
--
--   15:14:59   35 s
--   15:15:00   3530 s   <- one second later
--   15:16:13   30 s
--
-- "3530" is "35" with "30" typed onto the end rather than over it — the append
-- the comment describes — so the set was 30 s, which is what the next set at that
-- weight also was.
--
-- It is not currently the demonstrated max (better_carry ranks weight first, and
-- there are 20 kg carries), so this changes no prescription today. It would the
-- moment 12 kg became his heaviest: a 59-minute carry is a ceiling nothing could
-- climb back to.
UPDATE workout_sets w
  JOIN exercises e ON e.id = w.exercise_id
   SET w.hold_s = 30
 WHERE e.metric = 'weighted_hold'
   AND w.load_kg = 12
   AND w.hold_s = 3530;
