//! What the seeder stores is not what the bundle holds, and the difference is the
//! only thing standing between a picture and a severed head. The app's hero is
//! `aspect-ratio: 16/9; object-fit: cover`, so anything much taller than that gets
//! its top and bottom cropped away in the browser, where no test can see it.
//!
//! These pin the two decisions that keep a whole figure on screen: reshape by
//! shape, and override by name for the pictures whose shape lies.

use std::io::Cursor;
use std::path::Path;

use coach::seed::render::{PAD_ALWAYS, render};
use image::{ImageReader, RgbImage};

/// The app's hero shape, written out rather than imported — a test that reuses the
/// constant it is checking only proves the constant equals itself.
const HERO: f64 = 16.0 / 9.0;

/// Put a plain opaque picture of the given shape through the renderer at `slug`,
/// and report what came back.
fn rendered(w: u32, h: u32, slug: &str) -> (u32, u32, bool) {
    let mut raw = Vec::new();
    image::DynamicImage::ImageRgb8(RgbImage::from_pixel(w, h, image::Rgb([120, 120, 120])))
        .write_to(&mut Cursor::new(&mut raw), image::ImageFormat::Png)
        .expect("encoding the source");
    let out = render(&raw, "image/png", slug).expect("rendering");
    let untouched = out.bytes == raw;
    let (ow, oh) = ImageReader::new(Cursor::new(&out.bytes))
        .with_guessed_format()
        .expect("guessing the format")
        .into_dimensions()
        .expect("reading the dimensions");
    (ow, oh, untouched)
}

fn is_hero_shaped(w: u32, h: u32) -> bool {
    ((w as f64 / h as f64) - HERO).abs() < 0.01
}

#[test]
fn a_landscape_photograph_is_stored_exactly_as_it_came() {
    let (w, h, untouched) = rendered(800, 600, "an_exercise_that_is_not_listed");
    assert!(untouched, "4:3 opaque should pass through, got {w}x{h}");
}

#[test]
fn a_portrait_is_padded_to_the_hero_shape() {
    let (w, h, untouched) = rendered(400, 600, "an_exercise_that_is_not_listed");
    assert!(
        !untouched,
        "a portrait must be reshaped, not cropped in the browser"
    );
    assert!(is_hero_shaped(w, h), "expected 16:9, got {w}x{h}");
}

/// The whole point of the list: identical pixels, identical shape, and the only
/// difference is the name. A picture framed high in the frame loses its head to
/// the hero crop however landscape it is, and no shape rule can see that.
#[test]
fn a_listed_picture_is_padded_though_its_shape_would_pass() {
    for (slug, _why) in PAD_ALWAYS {
        let (w, h, untouched) = rendered(800, 600, slug);
        assert!(!untouched, "{slug} must be padded, not passed through");
        assert!(is_hero_shaped(w, h), "{slug}: expected 16:9, got {w}x{h}");
    }
}

/// A slug that no longer exists pads nothing and says nothing about it. Renaming
/// an exercise would quietly sever a head again, so the list is checked against
/// the catalog it names.
#[test]
fn every_listed_picture_is_still_in_the_catalog() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("data/catalog/exercises.json");
    let bytes = std::fs::read(&path).expect("reading the catalog");
    let catalog: Vec<serde_json::Value> =
        serde_json::from_slice(&bytes).expect("parsing the catalog");
    let slugs: Vec<&str> = catalog.iter().filter_map(|e| e["slug"].as_str()).collect();
    for (slug, _why) in PAD_ALWAYS {
        assert!(
            slugs.contains(slug),
            "PAD_ALWAYS names {slug}, the catalog does not"
        );
    }
}
