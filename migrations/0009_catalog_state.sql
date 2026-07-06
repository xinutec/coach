-- coach 0009: a 1-row fingerprint of the seeded catalog. The boot seeder stores
-- the SHA-256 of exercises.json here; next boot an unchanged hash short-circuits
-- the whole seed, and a changed hash triggers a reconcile of the M:N links
-- (equipment + muscle) so catalog corrections reach already-seeded exercises
-- instead of being skipped forever. Append-only.

CREATE TABLE catalog_state (
    id           TINYINT   NOT NULL PRIMARY KEY,
    catalog_hash CHAR(64)  NOT NULL,
    updated_at   TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP
);
