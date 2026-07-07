//! The link picker only offers named places; unnamed health "Stay" clusters
//! are hidden (see `DetectedPlace::is_named`).

use coach::health::{DetectedPlace, UNNAMED_PLACE_LABEL};

fn place(id: i64, label: &str) -> DetectedPlace {
    DetectedPlace {
        id,
        label: label.to_string(),
        amenity_label: None,
        last_seen_ts: None,
    }
}

#[test]
fn named_places_are_kept() {
    assert!(place(1, "Home").is_named());
    assert!(place(2, "Work").is_named());
}

#[test]
fn unnamed_stay_clusters_are_hidden() {
    assert!(!place(3, UNNAMED_PLACE_LABEL).is_named());
    assert!(!place(4, "Stay").is_named());
}

#[test]
fn filtering_drops_only_the_unnamed() {
    let detected = vec![
        place(1, "Home"),
        place(2, "Stay"),
        place(3, "Work"),
        place(4, "Stay"),
    ];
    let kept: Vec<_> = detected
        .into_iter()
        .filter(DetectedPlace::is_named)
        .map(|p| p.label)
        .collect();
    assert_eq!(kept, vec!["Home", "Work"]);
}
