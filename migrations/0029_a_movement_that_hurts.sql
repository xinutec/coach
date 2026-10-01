-- "This hurts" on a card. A fact about a movement, not an effort rating: the coach
-- rests it for two weeks and brings it back eased. The set log cannot hold it, since
-- a movement that hurts is one he stops logging.
--
-- One row per tap; the engine reads the latest per movement. Taking it back deletes
-- the rows still inside the rest, so a mis-tap leaves nothing behind.
CREATE TABLE hurts (
  id          BIGINT       NOT NULL AUTO_INCREMENT PRIMARY KEY,
  user_id     VARCHAR(255) NOT NULL,
  exercise_id BIGINT       NOT NULL,
  reported_at DATETIME     NOT NULL,
  KEY idx_hurts_user (user_id, reported_at)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
