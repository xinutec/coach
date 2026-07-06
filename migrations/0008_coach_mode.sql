-- coach 0008: the dynamic coach's knobs. `mode` is the high-level intent the
-- engine optimises for (switchable per session); `days_per_week` + `emphasis`
-- are the light dials. These replace the fixed-program model as the driver of
-- what to do — the engine now computes from history + mode, no plan required.
-- Append-only. `emphasis` region values mirror the muscle_groups.region ENUM.

ALTER TABLE settings
    ADD COLUMN mode ENUM('balanced','strength','skills','conditioning')
        NOT NULL DEFAULT 'balanced',
    ADD COLUMN days_per_week INT NOT NULL DEFAULT 4,
    ADD COLUMN emphasis ENUM('chest','back','shoulders','arms','forearms','core','legs') NULL;
