-- The set shape rules, in the thing that holds the sets.
--
-- `LoggedSet::parse` (coach-pacing/src/domain.rs) is the only shape the API
-- writes, and after 0020–0024 the log finally agrees with it. But a parser is a
-- property of one code path, not of the data. The NocoDB importer bypasses it by
-- design — it needs the `band` column the API has no field for — and 65 of 357
-- sets were mis-shaped that way before the clean-up migrations. A rule enforced
-- at exactly one call site is a rule the next call site forgets, which is the
-- same reasoning that turned `NewSet::shape_error` into a parse in the first
-- place.
--
-- A CHECK can hold the metric-*independent* half: the value ceilings, and that a
-- set carries exactly one measurement. The metric-dependent half — which column
-- a given exercise may use at all — needs `exercises.metric`, and a CHECK cannot
-- subquery. That half stays with the parser, and stays the reason the parser
-- exists.

-- A weight of zero is not a weight, it is the absence of one, which the domain
-- spells `load_kg: None` — the empty-bar technique set the athlete chose not to
-- weigh. Two landmine rotations imported from NocoDB on 2024-09-18 wrote it as
-- 0, which is the stronger and false claim that the bar was weighed and found
-- weightless. Nothing else in the log trips these constraints.
UPDATE workout_sets SET load_kg = NULL WHERE load_kg = 0;

ALTER TABLE workout_sets
  -- Ceilings, not policy: generous enough that a real outlier day is never
  -- refused, tight enough that a number describing nothing a human did cannot
  -- reach the ability model. Round 3 of the field test stored a fat-fingered
  -- 3 530-second farmers walk (an append instead of a replace) and the carry
  -- estimate would have read it as a demonstrated max. These are MAX_REPS,
  -- MAX_HOLD_S, MAX_DISTANCE_M and MAX_LOAD_KG.
  ADD CONSTRAINT ck_sets_reps     CHECK (reps       IS NULL OR reps       BETWEEN 1 AND 100),
  ADD CONSTRAINT ck_sets_hold     CHECK (hold_s     IS NULL OR hold_s     BETWEEN 1 AND 600),
  ADD CONSTRAINT ck_sets_distance CHECK (distance_m IS NULL OR distance_m BETWEEN 1 AND 500),
  ADD CONSTRAINT ck_sets_load     CHECK (load_kg    IS NULL OR (load_kg > 0 AND load_kg <= 300)),
  -- The coach never asks for an RPE — it doesn't interrogate the athlete — but
  -- the importer can carry one, and the ability model reads it (rir = 10 − rpe)
  -- to sharpen an e1RM. A value off the scale would move that estimate quietly.
  -- Mirrors the bound in `NewSet::validate`.
  ADD CONSTRAINT ck_sets_rpe      CHECK (rpe        IS NULL OR rpe        BETWEEN 1 AND 10),
  -- Exactly one measurement. Every `LoggedSet` variant carries one and only one
  -- — reps, seconds, or metres. None of them is a row recording that something
  -- happened and nothing about what; two is a row that doesn't say which of them
  -- it means.
  ADD CONSTRAINT ck_sets_one_measurement CHECK (
    (reps IS NOT NULL) + (hold_s IS NOT NULL) + (distance_m IS NOT NULL) = 1
  );
