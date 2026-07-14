-- A loaded carry is weight *and* time, and the metric taxonomy could say neither.
--
-- `hold` carries no load (the prescription is seconds, full stop) and
-- `weighted_reps` carries no clock — so the catalog's four carries (farmer's walk,
-- suitcase, waiter, overhead) were filed as weighted reps, and the coach duly
-- prescribed "Farmers walk (suitcase), 5 reps at 6 kg". Reps are not what a carry
-- is measured in; the athlete walks for a time, under a weight.
--
-- `weighted_hold` is the missing variant. The seeder reconciles `metric` from the
-- catalog, so the carries move over on the next boot.
ALTER TABLE exercises
    MODIFY metric ENUM('reps', 'weighted_reps', 'hold', 'weighted_hold') NOT NULL;
