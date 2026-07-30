-- Distance becomes something coach can say.
--
-- A farmer's walk is measured by how far you carried it, and until now the app
-- had no way to record that: `weighted_hold` is weight and *seconds*. So the
-- fourteen imported carries went into migration 0021's note as prose, which is
-- the same mistake NocoDB made from the other end — the unit living in text
-- beside the number instead of in the data.
--
-- `weighted_distance` is the metre twin of `weighted_hold`, with the same double
-- progression: climb the distance to a ceiling, then take the next weight up and
-- start the distance again.
ALTER TABLE workout_sets ADD COLUMN distance_m INT NULL AFTER hold_s;

ALTER TABLE exercises
  MODIFY COLUMN metric ENUM('reps','weighted_reps','hold','weighted_hold','weighted_distance')
  NOT NULL;

-- The three farmer's walk variations are the carries he does by distance.
--
-- This leaves the ten timed carries logged through the app in July 2026 (30 s
-- and 35 s) sitting under a distance metric, and they are deliberately not
-- converted: nobody recorded a walking pace, so metres from seconds would be an
-- invented number, and one larger than his real 10 m — it would raise the
-- ability ceiling on evidence that does not exist. They stay as the fact that
-- they are. The ability model reads the two independently (`carry` from seconds,
-- `carry_m` from metres), so they inform nothing about distance rather than
-- corrupting it.
--
-- Which is the same rule as everywhere else here: strict about what gets
-- written, tolerant of history that was written under a model we have since
-- improved on.
UPDATE exercises SET metric = 'weighted_distance'
 WHERE slug IN ('farmers_walk', 'farmers_walk_suitcase', 'farmers_walk_waiter');

-- Take the distance back out of the note and into its own column. 0021 wrote it
-- as "<n> m (imported)"; the weight was never in doubt.
UPDATE workout_sets w
  JOIN exercises e ON e.id = w.exercise_id
   SET w.distance_m = CAST(SUBSTRING_INDEX(TRIM(w.note), ' ', 1) AS UNSIGNED),
       w.note = NULL
 WHERE e.metric = 'weighted_distance'
   AND w.note LIKE '% m (imported)'
   AND w.distance_m IS NULL;
