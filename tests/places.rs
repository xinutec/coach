//! The link picker only offers places worth training at: health-named places
//! that aren't clearly non-training venues (see `DetectedPlace::is_trainable`).

use coach::health::DetectedPlace;

fn place(id: i64, named: bool, category: Option<&str>) -> DetectedPlace {
    DetectedPlace {
        id,
        label: "x".to_string(),
        amenity_label: None,
        named,
        category: category.map(str::to_string),
        last_seen_ts: None,
    }
}

#[test]
fn named_places_with_no_category_are_kept() {
    // Home / Work / a bare named place — category unmined.
    assert!(place(1, true, None).is_trainable());
}

#[test]
fn leisure_and_lodging_are_kept() {
    assert!(place(2, true, Some("leisure")).is_trainable()); // park, gym
    assert!(place(3, true, Some("lodging")).is_trainable()); // hotel
}

#[test]
fn non_training_venues_are_dropped() {
    assert!(!place(4, true, Some("food")).is_trainable()); // restaurant/cafe/bar
    assert!(!place(5, true, Some("errand")).is_trainable()); // shop/bank/pharmacy
    assert!(!place(6, true, Some("transport")).is_trainable()); // station/parking
}

#[test]
fn unnamed_places_are_dropped_regardless_of_category() {
    assert!(!place(7, false, None).is_trainable()); // bare "Stay"
    assert!(!place(8, false, Some("leisure")).is_trainable());
}

#[test]
fn filtering_keeps_only_the_trainable() {
    let detected = vec![
        place(1, true, None),              // keep (home/work)
        place(2, false, None),             // drop (unnamed Stay)
        place(3, true, Some("leisure")),   // keep (gym)
        place(4, true, Some("food")),      // drop (restaurant)
        place(5, true, Some("transport")), // drop (station)
    ];
    let kept: Vec<_> = detected
        .into_iter()
        .filter(DetectedPlace::is_trainable)
        .map(|p| p.id)
        .collect();
    assert_eq!(kept, vec![1, 3]);
}
