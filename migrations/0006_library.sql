-- coach 0006: the full training library — muscles, equipment, media, and the
-- many-to-many links that make an exercise self-describing, plus per-user
-- training locations (an equipment inventory you can be "at").
--
-- Designed from first principles (the NocoDB base it's migrated from was only a
-- data source, not a template). Append-only: never edit a shipped migration.
-- Signed ids so sqlx decodes to i64; utf8mb4; relationships by index, not FK
-- (matches the rest of the schema).
--
-- Reference data (muscle groups, muscles, equipment, exercises, links, images)
-- is loaded at boot by the seeder (src/seed) from data/catalog/, not seeded
-- here — keeping the big image blobs and the 119-row catalog out of SQL.

-- ---- anatomy ---------------------------------------------------------------

-- Anatomical grouping. `region` is the coarse body area for UI grouping; the
-- functional push/pull/legs/core axis lives on the exercise (`pattern`), not
-- here — separating "which muscle" from "which movement".
CREATE TABLE IF NOT EXISTS muscle_groups (
    id      BIGINT      NOT NULL AUTO_INCREMENT PRIMARY KEY,
    slug    VARCHAR(48) NOT NULL UNIQUE,
    name    VARCHAR(64) NOT NULL,
    region  ENUM('chest','back','shoulders','arms','forearms','core','legs') NOT NULL
) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci;

CREATE TABLE IF NOT EXISTS muscles (
    id              BIGINT       NOT NULL AUTO_INCREMENT PRIMARY KEY,
    slug            VARCHAR(48)  NOT NULL UNIQUE,
    name            VARCHAR(64)  NOT NULL,
    muscle_group_id BIGINT       NOT NULL,
    -- What the muscle does (e.g. "Hip extension"); shown in the library.
    function        VARCHAR(128) NULL,
    INDEX idx_muscles_group (muscle_group_id)
) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci;

-- ---- equipment -------------------------------------------------------------

-- The kit vocabulary. A location owns a subset of these; an exercise requires a
-- subset. "Bodyweight" is the empty set (no rows) — doable anywhere.
CREATE TABLE IF NOT EXISTS equipment (
    id        BIGINT      NOT NULL AUTO_INCREMENT PRIMARY KEY,
    slug      VARCHAR(48) NOT NULL UNIQUE,
    name      VARCHAR(64) NOT NULL,
    category  ENUM('free_weight','band','machine','ball','rig','bench') NOT NULL
) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci;

-- ---- exercise enrichment ---------------------------------------------------

-- The single coarse `equipment` enum is superseded by exercise_equipment (M:N,
-- fine-grained). Drop it and its consumers.
ALTER TABLE exercises DROP COLUMN equipment;

ALTER TABLE exercises
    -- The distinguishing variant of a movement ("single leg", "rings",
    -- "barbell") — display name is name + variation.
    ADD COLUMN variation  VARCHAR(64)  NULL AFTER name,
    ADD COLUMN position    ENUM('standing','seated','kneeling','half_kneeling',
                                'prone','supine','hanging','lunge') NULL,
    -- Coaching cue / how-to note.
    ADD COLUMN cue         VARCHAR(512) NULL,
    ADD COLUMN demo_url    VARCHAR(512) NULL,
    ADD COLUMN summary     VARCHAR(255) NULL,
    -- 1 (easiest) .. 5 (hardest); nullable, editable in the library.
    ADD COLUMN difficulty  TINYINT      NULL;

-- Which muscles an exercise trains, and how. `primary` = prime mover,
-- `secondary` = assists, `stabilizer` = isometric support.
CREATE TABLE IF NOT EXISTS exercise_muscle (
    exercise_id BIGINT NOT NULL,
    muscle_id   BIGINT NOT NULL,
    role        ENUM('primary','secondary','stabilizer') NOT NULL DEFAULT 'primary',
    PRIMARY KEY (exercise_id, muscle_id),
    INDEX idx_ex_muscle_muscle (muscle_id)
) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci;

-- Equipment an exercise requires (ALL of it). No rows = bodyweight.
CREATE TABLE IF NOT EXISTS exercise_equipment (
    exercise_id  BIGINT NOT NULL,
    equipment_id BIGINT NOT NULL,
    PRIMARY KEY (exercise_id, equipment_id),
    INDEX idx_ex_equip_equip (equipment_id)
) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci;

-- Demo image, stored in-DB as a blob (no file storage). One per exercise.
-- `etag` is the sha256 of the bytes, for HTTP caching on the image route.
CREATE TABLE IF NOT EXISTS exercise_images (
    exercise_id  BIGINT       NOT NULL PRIMARY KEY,
    content_type VARCHAR(64)  NOT NULL,
    bytes        MEDIUMBLOB   NOT NULL,
    byte_size    INT          NOT NULL,
    etag         CHAR(64)     NOT NULL
) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci;

-- ---- locations -------------------------------------------------------------

-- A place you train, defined by the kit available there (home, office gym, a
-- hotel room = nothing). The pacing engine uses the current location to decide
-- what's doable and to substitute when a goal's kit is missing.
CREATE TABLE IF NOT EXISTS locations (
    id         BIGINT       NOT NULL AUTO_INCREMENT PRIMARY KEY,
    user_id    VARCHAR(255) NOT NULL,
    name       VARCHAR(128) NOT NULL,
    is_default BOOLEAN      NOT NULL DEFAULT 0,
    created_at DATETIME     NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME     NULL,
    deleted_at DATETIME     NULL,
    INDEX idx_locations_user (user_id)
) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci;

CREATE TABLE IF NOT EXISTS location_equipment (
    location_id  BIGINT NOT NULL,
    equipment_id BIGINT NOT NULL,
    PRIMARY KEY (location_id, equipment_id)
) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci;

-- ---- history enrichment ----------------------------------------------------

-- Band colour/resistance for banded sets (the analogue of load_kg for bands).
ALTER TABLE workout_sets ADD COLUMN band VARCHAR(16) NULL;

-- The 19 placeholder built-ins from 0002 are superseded by the imported
-- library. Deactivate them, and free their natural slugs (which the real
-- catalog reuses — e.g. nordic_curl, plank) by suffixing `_legacy`. This is
-- non-destructive: ids and any references survive; the rows just drop out of
-- the default catalog and stop shadowing the imported movements.
UPDATE exercises SET slug = CONCAT(slug, '_legacy'), is_active = 0 WHERE slug IN (
    'pull_up','chin_up','ring_row','weighted_pull_up','db_row','ring_dip',
    'ring_push_up','push_up','overhead_press','ring_support_hold','goblet_squat',
    'split_squat','pistol_squat','calf_raise','nordic_curl','hanging_leg_raise',
    'l_sit','plank','dead_hang'
);
