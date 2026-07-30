-- The 2024 NocoDB import stored a generic `Count` column as `reps` for every
-- exercise (scripts/export-nocodb.mjs: `reps: r.Count ?? null`), whatever that
-- movement actually counted. For the fourteen Farmers walk sets it counted
-- **metres**: all fourteen are Count = 10 at weights from 12 to 32 kg, which is
-- one fixed 10 m carry done heavier over three weeks.
--
-- A distance in the reps column is not inert. `ability::Bests::feed` matches on
-- (load_kg, reps) without consulting the exercise's metric, so it puts 10 and
-- 32 kg through Epley and derives a 42.7 kg one-rep-max — for a carry. Ability
-- is a max over history, so that ceiling is one nothing later can lower.
--
-- Coach measures carries in seconds (dose::CARRY_BASE_S..CARRY_TOP_S) and has no
-- representation for distance, so there is no honest conversion: metres to
-- seconds needs a walking pace nobody recorded. What we do know is that the
-- number is not reps. So it moves to the note, where it stays readable and stops
-- being arithmetic. The weight is real and stays; the set still counts as volume.
UPDATE workout_sets w
  JOIN exercises e ON e.id = w.exercise_id
   SET w.note = TRIM(CONCAT(COALESCE(w.note, ''), ' ', w.reps, ' m (imported)')),
       w.reps = NULL
 WHERE e.metric = 'weighted_hold'
   AND w.reps IS NOT NULL
   AND w.hold_s IS NULL;
