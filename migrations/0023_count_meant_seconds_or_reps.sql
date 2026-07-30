-- The rest of the NocoDB `Count` column, read per movement.
--
-- `Count` meant whatever the exercise counted, and the exporter wrote all of it
-- to `reps`. Migration 0021 handled the carries (metres). These are the other
-- 51 sets, split by what the movement actually is. Where the catalog's metric
-- was also wrong, it is corrected here and in data/catalog/exercises.json so a
-- reseed agrees.
--
-- Read against his real numbers (heaviest logged load 85 kg, average 18 kg).
-- The strongest single confirmation is Side plank (Copenhagen): Count = 20 in
-- 2024, and the one hold he has logged through the app since is a Copenhagen at
-- 25 s. Seconds, and a plausible two-year progression.

-- 1. Movements that really are timed holds, whose Count was seconds.
--    Dead hang 45, Support hold 30, Plank (single arm) 10, Side plank
--    (Copenhagen) 20, Pull-up (L-sit) 6 — the last is treated as a hold because
--    the catalog and the code both already call it one.
UPDATE workout_sets w
  JOIN exercises e ON e.id = w.exercise_id
   SET w.hold_s = w.reps, w.reps = NULL
 WHERE e.metric = 'hold'
   AND e.slug IN ('dead_hang_passive', 'plank_single_arm', 'side_plank_copenhagen',
                  'support_hold_rings_turned_out', 'pull_up_l_sit')
   AND w.reps IS NOT NULL
   AND w.hold_s IS NULL;

-- 2. A weighted plank: 20 s under 20 kg. Timed and loaded, so weighted_hold.
UPDATE exercises SET metric = 'weighted_hold' WHERE slug = 'plank';
UPDATE workout_sets w
  JOIN exercises e ON e.id = w.exercise_id
   SET w.hold_s = w.reps, w.reps = NULL
 WHERE e.slug = 'plank'
   AND w.reps IS NOT NULL
   AND w.hold_s IS NULL;

-- 3. Movements catalogued as holds that are actually counted in reps. A plank
--    *to side plank* is a transition, a *dynamic* side plank is a dip, and a
--    high-plank row is a renegade row — you do repetitions of all three. Their
--    Count was already in the right column; only the metric was wrong.
UPDATE exercises SET metric = 'reps'          WHERE slug = 'plank_to_side_plank';
UPDATE exercises SET metric = 'weighted_reps' WHERE slug IN ('side_plank_dynamic', 'row_high_plank');

-- 4. Movements he loads, catalogued as bodyweight. The weight is real — 5-20 kg,
--    squarely inside his range — so the data stands and the metric moves.
UPDATE exercises SET metric = 'weighted_reps'
 WHERE slug IN ('good_morning_bench_seated', 'good_morning_ghd_prone',
                'rotational_downward_chop', 'split_squat_front_foot_elevated');
