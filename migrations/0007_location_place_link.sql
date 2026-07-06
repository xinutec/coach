-- coach 0007: link a training location to a health-sync "focus place" so the
-- app can auto-select where you are. Nullable + loosely coupled: an unset or
-- dangling link (place deleted in health's weekly rebuild) simply resolves to
-- null and the user picks the location manually. No FK — health owns the id in
-- a different service/DB. Append-only migration.

ALTER TABLE locations ADD COLUMN health_place_id INT NULL;
