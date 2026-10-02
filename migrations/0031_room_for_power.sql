-- Whether there is room at a location to jump or throw. A trainer does not have
-- someone broad-jumping in a hotel room; without this the coach could not know.
-- True by default, so every place set up before keeps its power work.
ALTER TABLE locations ADD COLUMN room_for_power BOOLEAN NOT NULL DEFAULT TRUE AFTER is_default;
