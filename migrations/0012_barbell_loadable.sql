-- coach 0012: loadable bars (barbell, trap bar). A dumbbell/kettlebell is a
-- fixed weight — you own discrete sizes. A barbell is *loadable*: its achievable
-- load is the empty bar plus plates, so the two facts that constrain it are the
-- bar's own weight (the floor — you can't go under the empty bar) and the plate
-- sizes you own (the step). `equipment.loadable` marks such kit; `kind` tags the
-- option rows that describe a bar setup ('bar' = the bar's weight, 'plate' = a
-- plate size owned) so they aren't mistaken for liftable fixed weights.
ALTER TABLE equipment ADD COLUMN loadable BOOLEAN NOT NULL DEFAULT 0;
UPDATE equipment SET loadable = 1 WHERE slug IN ('barbell', 'trap_bar');

ALTER TABLE location_equipment_option ADD COLUMN kind VARCHAR(8) NULL;
