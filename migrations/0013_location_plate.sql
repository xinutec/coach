-- coach 0013: plates belong to the location, not to a bar. Plates are a shared
-- pool — any plate fits any bar at that location — so they're entered once and
-- every loadable bar (barbell, trap bar) draws from the same set. Previously
-- plates were stored per bar (location_equipment_option kind='plate'), which
-- forced double entry and let the two bars' plate lists drift apart. Hoist the
-- existing per-bar plates up to the location (dedup), then drop the old rows.
CREATE TABLE IF NOT EXISTS location_plate (
    id          BIGINT NOT NULL AUTO_INCREMENT PRIMARY KEY,
    location_id BIGINT NOT NULL,
    load_kg     DOUBLE NOT NULL,   -- a plate size owned (kg, per plate)
    INDEX idx_lp_loc (location_id)
) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci;

INSERT INTO location_plate (location_id, load_kg)
    SELECT DISTINCT location_id, load_kg
    FROM location_equipment_option
    WHERE kind = 'plate' AND load_kg IS NOT NULL;

DELETE FROM location_equipment_option WHERE kind = 'plate';
