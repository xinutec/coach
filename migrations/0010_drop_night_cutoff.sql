-- coach 0010: collapse the two evening lines into one. `night_cutoff_hour` was
-- redundant with `window_end_hour` (both defaulted to 21, and the window already
-- closes the nudge door) — the engine now treats the window's end as the single
-- "stop nudging + roll remaining volume to tomorrow" line. Training + logging stay
-- available outside the window; it only governs whether coach nudges you.

ALTER TABLE settings DROP COLUMN night_cutoff_hour;
