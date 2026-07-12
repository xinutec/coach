-- coach 0016: model the kit you actually own, so a suggested load is one you can
-- physically assemble.
--
-- Three facts the previous model couldn't express, all of which bite on a home
-- dumbbell set (they were harmless in a gym, which is why they were absent):
--
--   1. A plate fits a *bar*. 0013 hoisted plates to the location on the rationale
--      "any plate fits any bar here" — true of Olympic discs across a barbell and a
--      trap bar, false of an adjustable dumbbell whose handle takes small discs
--      that will not go on an Olympic sleeve. `location_plate.equipment_id` NULL
--      keeps 0013's shared pool (the Olympic case, unchanged); set, it pins the
--      plate to one piece of kit.
--
--   2. You own a *finite number* of discs and implements. `reachable_loads`
--      assumed unlimited plates: fine for a gym rack, wrong for a home set, where
--      owning one pair of 2.5s means 2.5-per-side is reachable and 5-per-side is
--      not. And a pair of dumbbells *splits* the disc budget — four of each disc is
--      two per dumbbell when the movement needs two — so a both-arms press tops out
--      far below what the same discs reach on one goblet-squat dumbbell. `qty` NULL
--      still means "plenty".
--
--   3. A sleeve has finite space: past `plate_slots` discs a side, nothing more
--      fits however many you own. NULL = unlimited.
--
-- `implements` on the exercise is the other half of (2): a movement declares how
-- many of the implement it uses (a goblet squat takes one dumbbell, a dumbbell
-- bench press takes two), which is what decides how the discs are shared out.
ALTER TABLE location_plate
    ADD COLUMN equipment_id BIGINT NULL AFTER location_id,
    ADD COLUMN qty          INT    NULL AFTER load_kg,
    ADD INDEX idx_lp_equip (equipment_id);

-- For a 'bar' row: how many handles/bars you own (a both-arms dumbbell movement
-- needs two), and how many discs fit on each sleeve. For a fixed free weight: how
-- many of that weight you own (one 5 kg dumbbell can't do a two-dumbbell press).
ALTER TABLE location_equipment_option
    ADD COLUMN qty         INT NULL,
    ADD COLUMN plate_slots INT NULL;

-- How many of the implement the movement uses. 1 for a barbell, a goblet squat, a
-- single-arm row; 2 for a dumbbell bench press or a both-arms overhead press.
ALTER TABLE exercises
    ADD COLUMN implements TINYINT NOT NULL DEFAULT 1;
