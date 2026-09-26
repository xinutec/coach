//! Render a catalog picture into the form the app can actually display.
//!
//! The catalog bundle is the **source**: a picture keeps whatever it arrived with,
//! alpha included, because a source that has already been flattened cannot be
//! un-flattened. Two kinds of picture live there, and only one of them is ready to
//! be shown:
//!
//! - **Photographs** — a person doing the movement. Opaque, roughly landscape,
//!   framed. The app crops them to its 16:9 hero and they look right.
//! - **Anatomy diagrams** — dark line-art on a *transparent* background, portrait
//!   (241×338 is typical), with the working muscle highlighted. Shown as-is, they
//!   fail twice: the app is theme-aware, so on a dark surface dark ink on
//!   transparent is dark-on-near-black and the figure disappears; and a 16:9
//!   `object-fit: cover` crops a portrait figure to a band across its stomach —
//!   losing the movement and the very muscle the picture exists to show.
//!
//! So: transparency is composited onto white, and a picture whose shape is far from
//! the hero's is **padded, never cropped**. A picture that is already opaque and
//! roughly landscape is stored byte-for-byte as it came — most of the bundle, and
//! the cheap path in every sense: it isn't even decoded.
//!
//! A shape test cannot see where the head is, though, and a few pictures are
//! landscape enough to pass it while framed tightly enough that the hero's centre
//! crop takes the head off. Those are named one by one in `PAD_ALWAYS`, because
//! what makes them different is a fact about the photograph, not its dimensions.
//!
//! The shape test is deliberately about the *shape*, not about the alpha: an
//! opaque square diagram loses its head and feet to `cover` exactly like a
//! portrait does. Nor is it about the file type — a palette PNG carrying real
//! transparency is rendered like a diagram, because that is what its pixels say.
//!
//! This lives in the seeder rather than in the import script because it is a
//! *rendering* decision, and rendering decisions should have one implementation —
//! not one per tool that happens to add a file.

use anyhow::{Context, Result};
use std::io::Cursor;

use image::codecs::png::{self, CompressionType, PngEncoder};
use image::{
    DynamicImage, ExtendedColorType, ImageDecoder, ImageEncoder, ImageReader, Rgba, RgbaImage,
    imageops,
};

/// The hero's aspect ratio. Match it and the picture is never cropped there.
const ASPECT: f64 = 16.0 / 9.0;
/// Wide enough for a phone at 3× without being a megabyte.
const TARGET_W: u32 = 1200;
/// Narrower than this (width ÷ height) and `object-fit: cover` crops enough of the
/// figure to matter, so the picture is padded to 16:9 instead. Set below 5:4
/// (1.25), where photographs generally crop fine, and above square, which does
/// not; the exceptions are [`PAD_ALWAYS`].
const CROPS_BADLY_BELOW: f64 = 1.2;

/// Pictures the shape rule gets wrong. Each is wide enough to clear
/// `CROPS_BADLY_BELOW`, and each still loses a head to the hero's centre crop,
/// because the figure is framed high in the frame rather than centred. Padded
/// instead, so the whole figure shows.
///
/// Enumerated rather than generalised on purpose: the other pictures in the same
/// aspect band crop fine, so any rule wide enough to catch these would pad them
/// for nothing. Each entry says what the crop does to it, so it can be
/// checked against the picture instead of taken on trust.
pub const PAD_ALWAYS: &[(&str, &str)] = &[
    // 5:4.
    ("squat_front_rack_double_kettlebell", "cut across the eyes"),
    // 4:3, two figures side by side, and the crop takes both at the chin.
    ("banded_rotation_propulsive", "both figures cut at the chin"),
    // 4:3. The squatting figure survives it; the one on the box loses its head.
    ("box_jump", "the landing figure cut at the neck"),
];

/// The rendered picture, and the content type it is now in.
pub struct Rendered {
    pub bytes: Vec<u8>,
    pub content_type: String,
}

/// Render `raw` for display. Returns the original bytes untouched when it needs no
/// rendering — which is the common case, and keeps a re-seed from rewriting every
/// photograph in the bundle.
pub fn render(raw: &[u8], content_type: &str, slug: &str) -> Result<Rendered> {
    // Decide from the *header* first. Most of the bundle is landscape photographs
    // with no alpha channel at all, and fully decoding all of them just to learn
    // that they need nothing done is the difference between a seed that takes a
    // moment and one that takes a minute.
    let decoder = ImageReader::new(Cursor::new(raw))
        .with_guessed_format()
        .with_context(|| format!("reading {slug}"))?
        .into_decoder()
        .with_context(|| format!("decoding {slug}"))?;
    let (w, h) = decoder.dimensions();
    let crops_badly = f64::from(w) / f64::from(h.max(1)) < CROPS_BADLY_BELOW
        || PAD_ALWAYS.iter().any(|(s, _)| *s == slug);
    let may_be_transparent = decoder.color_type().has_alpha();
    if !crops_badly && !may_be_transparent {
        return Ok(Rendered {
            bytes: raw.to_vec(),
            content_type: content_type.to_string(),
        });
    }

    // It's the right shape *and* it merely has an alpha channel it doesn't use (an
    // RGBA photograph) → still nothing to do. This is the only case that needs the
    // pixels, so it's the only one that pays for them.
    let img = DynamicImage::from_decoder(decoder).with_context(|| format!("decoding {slug}"))?;
    if !crops_badly && !is_transparent(&img) {
        return Ok(Rendered {
            bytes: raw.to_vec(),
            content_type: content_type.to_string(),
        });
    }

    // Flatten always (that's why we're here), but only *reshape* what's misshapen —
    // a 16:9 diagram that merely has an alpha channel needs no resampling at all,
    // and resizing it to the same shape is pure cost.
    let flat = flatten_onto_white(&img);
    let canvas = if crops_badly {
        pad_to_aspect(&flat)
    } else {
        flat
    };

    let rgb = DynamicImage::ImageRgba8(canvas).to_rgb8();
    let mut bytes = Vec::new();
    // Fast compression: this is a cache in front of a database blob, not an asset
    // shipped over the wire a million times. Default compression tripled the seed.
    PngEncoder::new_with_quality(
        Cursor::new(&mut bytes),
        CompressionType::Fast,
        png::FilterType::Adaptive,
    )
    .write_image(
        rgb.as_raw(),
        rgb.width(),
        rgb.height(),
        ExtendedColorType::Rgb8,
    )
    .with_context(|| format!("encoding {slug}"))?;
    Ok(Rendered {
        bytes,
        content_type: "image/png".to_string(),
    })
}

/// Does the picture actually *use* its alpha channel? An RGBA photo whose every
/// pixel is opaque is a photo, not a diagram — the channel's presence proves
/// nothing, so this asks about the pixels.
fn is_transparent(img: &DynamicImage) -> bool {
    img.color().has_alpha() && img.to_rgba8().pixels().any(|p| p.0[3] < 255)
}

fn flatten_onto_white(img: &DynamicImage) -> RgbaImage {
    let src = img.to_rgba8();
    let mut out = RgbaImage::from_pixel(src.width(), src.height(), Rgba([255, 255, 255, 255]));
    imageops::overlay(&mut out, &src, 0, 0);
    out
}

/// A computed length in whole pixels.
fn px(x: f64) -> u32 {
    u32::try_from(coach_pacing::num::natural(x.round())).unwrap_or(u32::MAX)
}

/// Fit the whole figure into a 16:9 canvas: scale to fit, centre, pad with white.
/// Padding rather than cropping is the point — the figure is the content.
fn pad_to_aspect(src: &RgbaImage) -> RgbaImage {
    let target_h = px(f64::from(TARGET_W) / ASPECT);
    let scale = f64::min(
        f64::from(TARGET_W) / f64::from(src.width()),
        f64::from(target_h) / f64::from(src.height()),
    );
    let (w, h) = (
        px(f64::from(src.width()) * scale).max(1),
        px(f64::from(src.height()) * scale).max(1),
    );
    let fitted = imageops::resize(src, w, h, imageops::FilterType::Triangle);

    let mut canvas = RgbaImage::from_pixel(TARGET_W, target_h, Rgba([255, 255, 255, 255]));
    imageops::overlay(
        &mut canvas,
        &fitted,
        i64::from((TARGET_W - w) / 2),
        i64::from((target_h - h) / 2),
    );
    canvas
}
