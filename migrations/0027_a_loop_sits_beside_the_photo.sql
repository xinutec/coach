-- A generated 3D loop is a SECOND artifact, never a replacement. The sourced
-- photographs show what to do and took real work to gather; a loop that turns
-- out badly must not be able to cost us one. Separate table, so seeding a loop
-- cannot touch exercise_images.
CREATE TABLE IF NOT EXISTS exercise_loops (
    exercise_id  BIGINT       NOT NULL PRIMARY KEY,
    content_type VARCHAR(64)  NOT NULL,
    bytes        MEDIUMBLOB   NOT NULL,
    byte_size    INT          NOT NULL,
    etag         CHAR(64)     NOT NULL
) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci;
