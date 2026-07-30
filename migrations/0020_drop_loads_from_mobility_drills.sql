-- Two logged sets carry a load against a warm-up mobility drill: Side bend and
-- Squat sky reach, both 10 reps @ 4 kg on 2026-07-16.
--
-- These are the incident `LoggedSet::parse` exists to prevent — a stale hidden
-- form field on the log sheet posting a load the athlete never used — and they
-- predate the check, so they are still in the log. A mobility drill takes no
-- load: the reps are what happened, the weight is the falsehood, and the ability
-- model reads the column.
--
-- Scoped to `warmup = 1` deliberately. Other `metric = 'reps'` rows also carry
-- loads (Split squat, Good morning, Rotational downward chop, from the 2024
-- NocoDB import) and those are the opposite problem: the weight is real and the
-- catalog's metric is wrong. Dropping their loads would destroy honest history,
-- so they are left for a catalog fix.
UPDATE workout_sets w
  JOIN exercises e ON e.id = w.exercise_id
   SET w.load_kg = NULL
 WHERE e.metric = 'reps'
   AND e.warmup = 1
   AND w.load_kg IS NOT NULL;
