-- coach 0014: remove the periodized-program feature. The adaptive pacer
-- recomputes from logged history + recovery each call and never read these
-- tables (see src/pacing/*), so they were dead weight. Drop the program tables
-- and the workout_sets.program_id stamp that tied a set to an "active program".
-- Append-only: this migration deletes; it does not touch 0003/0004's DDL.

DROP TABLE IF EXISTS program_pins;
DROP TABLE IF EXISTS program_targets;
DROP TABLE IF EXISTS programs;

ALTER TABLE workout_sets DROP COLUMN program_id;
