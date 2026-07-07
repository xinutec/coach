-- coach 0015: classification flags on exercises, catalog-authoritative.
-- `skill` replaces the hardcoded equipment-slug sniff in pacing/service.rs (ring/
-- parallette gymnastic work); `warmup` marks mobility/activation moves that the
-- warm-up block draws from and that credit no training volume. Both are seeded +
-- reconciled from data/catalog/exercises.json (see src/seed). Append-only.

ALTER TABLE exercises
    ADD COLUMN skill  BOOLEAN NOT NULL DEFAULT 0,
    ADD COLUMN warmup BOOLEAN NOT NULL DEFAULT 0;
