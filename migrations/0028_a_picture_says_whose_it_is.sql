-- The credit a picture's licence asks for (a render derived from Z-Anatomy is
-- CC BY-SA), authored in the catalog beside the image. On the exercise rather
-- than the image row: the seed reconciles exercise scalars on every catalog
-- change, but rewrites an image only when its bytes change, so a corrected
-- credit on an unchanged picture would never land there.
ALTER TABLE exercises
    ADD COLUMN image_credit     VARCHAR(255) NULL,
    ADD COLUMN image_credit_url VARCHAR(512) NULL;
