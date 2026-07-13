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
/// pull a video id out of the URL. Two entries once read `youtube.be` (a typo for
/// `youtu.be`, and a domain that isn't YouTube at all): a silently dead button.
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
