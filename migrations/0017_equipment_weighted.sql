-- Which kit can carry a load is a catalog fact, not a guess from its category.
--
-- The engine used to decide this with `category = 'free_weight'`, which is true of
-- a dumbbell and false of a cable stack — so a pulley machine, whose entire point
-- is the weight on it, could hold no weight, and every cable movement had to be
-- modelled as bodyweight reps. It also read as a category error in the other
-- direction: asking what weights are registered for a bench.
--
-- `weighted` says it outright: this is kit whose load the athlete registers, and
-- the coach may prescribe a weight for. `loadable` (0012) stays a narrower fact —
-- kit you build a load on from *plates* (a barbell, an adjustable handle), as
-- opposed to picking a whole weight off a rack or a pin out of a stack.
--
-- Backfill preserves today's behaviour exactly (free weights bore load, nothing
-- else did); the catalog seeder then reconciles the column from equipment.json,
-- which is what actually turns the cable stack on.
ALTER TABLE equipment ADD COLUMN weighted TINYINT(1) NOT NULL DEFAULT 0;
UPDATE equipment SET weighted = 1 WHERE category = 'free_weight';
