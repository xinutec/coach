-- What the coach put on a card, as opposed to what happened.
--
-- R6-4: `partial` left after 60 % of the cards every session for eight weeks, and
-- Pallof press and Body saw were offered twenty times each and performed zero. No
-- group-level statistic can see that — the clearest case is Pistol squat, offered
-- eight times and never done, while Quadriceps was the best-served group in the
-- whole log. At group level there is nothing wrong.
--
-- It cannot be derived either: `workout_sets` is by construction the record of
-- what *did* happen. "Offered and not done" is a fact about cards, so the cards
-- have to be written down.
--
-- One row per (user, day, movement), so re-evaluating the verdict — which the
-- Android geofence poller does unprompted, many times a day — writes the same row
-- rather than accumulating. Whether a day counts as evidence is decided at read
-- time by whether it carries any logged set, which is what stops a day he never
-- opened the app from reading as a day he declined something.
CREATE TABLE plan_offers (
  user_id     VARCHAR(255) NOT NULL,
  offered_on  DATE         NOT NULL,
  exercise_id BIGINT       NOT NULL,
  created_at  DATETIME     NOT NULL DEFAULT CURRENT_TIMESTAMP,
  PRIMARY KEY (user_id, offered_on, exercise_id),
  KEY idx_offers_user_day (user_id, offered_on)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
