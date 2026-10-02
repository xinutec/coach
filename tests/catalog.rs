//! Guards on the curated catalog itself. The seeder trusts this file, and the app
//! shows what it says — so a bad link here is a dead "Watch demo" button in the
//! gym, discovered at the worst possible moment.

use std::path::Path;

use serde_json::Value;

fn catalog() -> Vec<Value> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("data/catalog/exercises.json");
    let bytes = std::fs::read(&path).expect("reading the catalog");
    serde_json::from_slice(&bytes).expect("parsing the catalog")
}

/// Every demo link must be a YouTube video the app can *play in the sheet* — the
/// frontend embeds it rather than linking out, and it can only do that if it can
/// pull a video id out of the URL. A typo like `youtube.be` (not a YouTube domain
/// at all) is a silently dead button.
#[test]
fn every_demo_url_is_an_embeddable_youtube_video() {
    let bad: Vec<String> = catalog()
        .iter()
        .filter_map(|ex| {
            let url = ex.get("demoUrl")?.as_str()?;
            video_id(url).is_none().then(|| {
                format!(
                    "{}: {url}",
                    ex.get("slug").and_then(Value::as_str).unwrap_or("?")
                )
            })
        })
        .collect();
    assert!(
        bad.is_empty(),
        "demo links the app cannot play:\n{}",
        bad.join("\n")
    );
}

/// A weighted lift has to name kit that can actually carry a weight, or there is
/// no honest load for it and the engine drops it from every session — silently,
/// from the athlete's side, since the "no weights registered" notice can only
/// speak about kit that *could* have had weights. The service logs a warning and
/// moves on; the catalog is where the mistake is, so this is where it should fail.
///
/// It is the check that lets `weighted` be a fact rather than a hope: mark the
/// cable machine as carrying load and its exercises may be weighted; forget to,
/// and this test names them.
#[test]
fn every_weighted_lift_declares_kit_that_carries_a_load() {
    let weighted_kit: Vec<String> = equipment()
        .iter()
        .filter(|e| e.get("weighted").and_then(Value::as_bool).unwrap_or(false))
        .filter_map(|e| Some(e.get("slug")?.as_str()?.to_string()))
        .collect();
    assert!(
        !weighted_kit.is_empty(),
        "no equipment carries a load — the flag is missing from equipment.json"
    );

    // Every metric that carries a weight, not just weighted *reps*. A carry is
    // loaded too, and a weighted plank is a hold with a plate on it; kit that
    // holds nothing would mean the coach silently never prescribes them.
    let loaded = ["weighted_reps", "weighted_hold"];
    let orphans: Vec<String> = catalog()
        .iter()
        .filter(|ex| {
            ex.get("metric")
                .and_then(Value::as_str)
                .is_some_and(|m| loaded.contains(&m))
        })
        .filter(|ex| {
            let kit = ex.get("equipment").and_then(Value::as_array);
            !kit.is_some_and(|kit| {
                kit.iter()
                    .filter_map(Value::as_str)
                    .any(|s| weighted_kit.iter().any(|w| w == s))
            })
        })
        .filter_map(|ex| Some(ex.get("slug")?.as_str()?.to_string()))
        .collect();
    assert!(
        orphans.is_empty(),
        "weighted lifts whose kit cannot hold a weight (so the coach can never \
         prescribe them):\n{}",
        orphans.join("\n")
    );
}

/// The kit a word in a movement's name means. A word with two answers ("box": a
/// plyo box or a bench) accepts either.
const NAMED_KIT: &[(&str, &[&str])] = &[
    ("barbell", &["barbell"]),
    ("dumbbell", &["dumbbell"]),
    ("kettlebell", &["kettlebell"]),
    ("trap bar", &["trap_bar"]),
    ("hex bar", &["trap_bar"]),
    ("cable", &["cable_machine"]),
    ("med ball", &["medicine_ball"]),
    ("medicine ball", &["medicine_ball"]),
    ("rings", &["gymnastic_rings"]),
    ("parallettes", &["parallettes"]),
    ("bench", &["bench"]),
    ("ghd", &["ghd"]),
    ("band", &["resistance_band"]),
    ("banded", &["resistance_band"]),
    ("battle rope", &["battle_rope"]),
    ("box", &["plyo_box", "bench"]),
];

/// A movement whose name says what it is done with must list that kit, or the coach
/// offers it where the kit is absent: round 10 planned battle-rope slams and an ab
/// rollout on a barbell for a hotel room with nothing in it.
#[test]
fn a_movement_named_after_its_kit_lists_that_kit() {
    // Lower-case words between single spaces, so " box " never matches "boxer".
    let words = |s: &str| -> String {
        let lower: String = s
            .to_lowercase()
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { ' ' })
            .collect();
        format!(
            " {} ",
            lower.split_whitespace().collect::<Vec<_>>().join(" ")
        )
    };
    let mut missing = Vec::new();
    for ex in catalog() {
        let field = |k: &str| ex.get(k).and_then(Value::as_str).unwrap_or("");
        let name = words(&format!("{} {}", field("name"), field("variation")));
        let kit: Vec<&str> = ex
            .get("equipment")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        for (word, any_of) in NAMED_KIT {
            if name.contains(&format!(" {word} ")) && !any_of.iter().any(|k| kit.contains(k)) {
                missing.push(format!("{}: says {word:?}, lists {kit:?}", field("slug")));
            }
        }
    }
    assert!(
        missing.is_empty(),
        "movements named after kit they do not list:\n{}",
        missing.join("\n")
    );
}

/// Every answer above is kit the catalog has: a slug nobody declares would make
/// the movement that needs it unplannable everywhere.
#[test]
fn the_kit_a_name_means_exists() {
    let slugs: Vec<String> = equipment()
        .iter()
        .filter_map(|e| Some(e.get("slug")?.as_str()?.to_string()))
        .collect();
    let unknown: Vec<&str> = NAMED_KIT
        .iter()
        .flat_map(|(_, kit)| kit.iter().copied())
        .filter(|k| !slugs.iter().any(|s| s == k))
        .collect();
    assert!(unknown.is_empty(), "not in equipment.json: {unknown:?}");
}

fn equipment() -> Vec<Value> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("data/catalog/equipment.json");
    let bytes = std::fs::read(&path).expect("reading the equipment catalog");
    serde_json::from_slice(&bytes).expect("parsing the equipment catalog")
}

/// The same rule the frontend's `parseYoutube` applies, kept deliberately narrow:
/// a `youtu.be/<id>` short link or a `youtube.com/watch?v=<id>`, with an 11-char id.
fn video_id(url: &str) -> Option<&str> {
    let rest = url.strip_prefix("https://")?;
    let (host, path) = rest.split_once('/')?;
    let id = match host {
        "youtu.be" => path.split(['?', '&']).next()?,
        "youtube.com" | "www.youtube.com" => path
            .strip_prefix("watch?")?
            .split('&')
            .find_map(|kv| kv.strip_prefix("v="))?,
        _ => return None,
    };
    (id.len() == 11
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'))
    .then_some(id)
}
