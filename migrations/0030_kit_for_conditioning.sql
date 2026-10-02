-- Battle ropes are neither a weight nor a frame; round 10 found the catalog had no
-- kit for them, so the coach offered rope slams in a hotel room.
ALTER TABLE equipment
  MODIFY category ENUM('free_weight','band','machine','ball','rig','bench','conditioning') NOT NULL;
