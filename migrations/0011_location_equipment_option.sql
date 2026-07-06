-- coach 0011: per-location equipment specifics. `location_equipment` says a kind
-- of kit is present; this says *which* — the discrete weights you own for a free
-- weight (so coach snaps its load suggestion to a weight you can actually load,
-- instead of inventing 12.5 kg), or the named variants you own for a band. A row
-- carries exactly one of `load_kg` (free weights) or `label` (bands/other).

CREATE TABLE IF NOT EXISTS location_equipment_option (
    id           BIGINT      NOT NULL AUTO_INCREMENT PRIMARY KEY,
    location_id  BIGINT      NOT NULL,
    equipment_id BIGINT      NOT NULL,
    load_kg      DOUBLE      NULL,   -- a discrete weight you own (free weights)
    label        VARCHAR(40) NULL,   -- a named variant you own (e.g. band tension)
    INDEX idx_leo_loc (location_id),
    INDEX idx_leo_loc_equip (location_id, equipment_id)
) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci;
