-- coach 0019: `power` classification flag on exercises, catalog-authoritative.
-- Marks maximal-intent ballistic work — jumps, throws/slams, Olympic lifts, plyo
-- push-ups — whose *quality* degrades under fatigue, so the session orders them
-- first (fresh CNS), before strength compounds. Distinct from `skill` (gymnastic
-- ring/parallette work) and from conditioning "explosive" work (battle-rope slams,
-- thrusters) which is a finisher you *want* tired. Seeded + reconciled from
-- data/catalog/exercises.json (see src/seed). Append-only.

ALTER TABLE exercises
    ADD COLUMN power BOOLEAN NOT NULL DEFAULT 0;
