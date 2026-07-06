//! The dynamic coaching engine: a pure function from (history, mode, instant) to
//! a verdict. No I/O, no clock — the caller passes `now` (user-local tz). It
//! computes rolling muscle-group volume, gates on recovery, sets per-group
//! targets from the active **mode** blended with the user's own history, picks
//! the biggest recovered deficit, chooses a location-doable exercise, and
//! progresses it off the last performance. No program, no weekly plan.
//!
//! All coefficients below are labelled heuristics, tunable — targets are anchored
//! to the user's own history to avoid false-precision absolute landmarks.

use std::collections::HashMap;

use chrono::{Duration, NaiveDateTime, Timelike};

use crate::exercise::types::Metric;
use crate::muscle::types::{MuscleRole, Region};
use crate::settings::types::Mode;

use super::types::{
    ExerciseInfo, GroupBalance, LastPerf, PacingInput, PacingNow, PacingState, Suggestion,
};

// ---- tunable heuristics ----------------------------------------------------
const ROLLING_DAYS: i64 = 7; // rolling-volume window (a training week)
const HISTORY_WEEKS: i64 = 8; // personal-average window
const RECOVERY_HOURS: i64 = 36; // a group hit hard within this is still recovering
const RECOVERY_SETS: f64 = 3.0; // effective sets within RECOVERY_HOURS → recovering
const DEFAULT_WEEKLY_SETS: f64 = 10.0; // literature maintenance→growth anchor
const SECONDARY_CREDIT: f64 = 0.5; // a secondary muscle counts half a set
const EMPHASIS_MULT: f64 = 1.5;
const DELOAD_RATIO: f64 = 1.6; // last-7d volume this far above avg → auto-deload
const DELOAD_SCALE: f64 = 0.6;

/// Per-region volume weighting for a mode. Balanced = flat; the others tilt
/// toward what the mode is for.
fn region_mult(mode: Mode, r: Region) -> f64 {
    use Region::*;
    match mode {
        Mode::Balanced => 1.0,
        Mode::Strength => match r {
            Legs => 1.3,
            Back => 1.2,
            Chest => 1.2,
            Shoulders => 1.1,
            Core => 0.9,
            Arms => 0.7,
            Forearms => 0.6,
        },
        Mode::Skills => match r {
            Core => 1.4,
            Shoulders => 1.3,
            Forearms => 1.3,
            Arms => 1.1,
            Back => 1.1,
            Chest => 0.9,
            Legs => 0.8,
        },
        Mode::Conditioning => match r {
            Legs => 1.2,
            Core => 1.2,
            Chest | Back | Shoulders => 1.0,
            Arms => 0.9,
            Forearms => 0.8,
        },
    }
}

/// Rep range for a mode + metric (holds are seconds, handled in `progress`).
fn rep_range(mode: Mode, metric: Metric) -> (Option<i32>, Option<i32>) {
    if metric == Metric::Hold {
        return (None, None);
    }
    let weighted = metric == Metric::WeightedReps;
    match mode {
        Mode::Strength => {
            if weighted {
                (Some(3), Some(6))
            } else {
                (Some(5), Some(8))
            }
        }
        Mode::Balanced => {
            if weighted {
                (Some(6), Some(10))
            } else {
                (Some(8), Some(12))
            }
        }
        Mode::Skills => (Some(3), Some(6)),
        Mode::Conditioning => {
            if weighted {
                (Some(12), Some(20))
            } else {
                (Some(15), Some(25))
            }
        }
    }
}

/// How well an exercise fits the mode's style (for ranking within a group).
fn mode_fit(mode: Mode, ex: &ExerciseInfo) -> f64 {
    match mode {
        Mode::Strength => {
            if ex.metric == Metric::WeightedReps {
                1.0
            } else if ex.is_skill {
                0.2
            } else {
                0.6
            }
        }
        Mode::Skills => {
            if ex.is_skill || ex.metric == Metric::Hold {
                1.0
            } else {
                0.4
            }
        }
        Mode::Conditioning => match ex.metric {
            Metric::Reps => 1.0,
            Metric::WeightedReps => 0.7,
            Metric::Hold => 0.3,
        },
        Mode::Balanced => 0.8,
    }
}

/// Progress an exercise off its last performance: (sets, rep_low, rep_high,
/// load_kg, hold_s). Double-progression — top of range last time → add load/rep.
fn progress(
    ex: &ExerciseInfo,
    last: Option<&LastPerf>,
    mode: Mode,
) -> (i32, Option<i32>, Option<i32>, Option<f64>, Option<i32>) {
    let sets = 3;
    match ex.metric {
        Metric::Hold => {
            let base = last.and_then(|l| l.hold_s).unwrap_or(20);
            (2, None, None, None, Some(base + 5)) // +5 s
        }
        Metric::WeightedReps => {
            let (lo, hi) = rep_range(mode, ex.metric);
            let reps = last.and_then(|l| l.reps);
            let load = last.and_then(|l| l.load_kg);
            match (reps, load, hi) {
                (Some(r), Some(w), Some(h)) if r >= h => (sets, lo, hi, Some(w + 2.5), None),
                (Some(r), Some(w), Some(h)) => (sets, Some((r + 1).min(h)), hi, Some(w), None),
                (_, w, _) => (sets, lo, hi, w, None),
            }
        }
        Metric::Reps => {
            let (lo, hi) = rep_range(mode, ex.metric);
            match (last.and_then(|l| l.reps), hi) {
                (Some(r), Some(h)) if r >= h => (sets, hi, hi, None, None),
                (Some(r), Some(h)) => (sets, Some((r + 1).min(h)), hi, None, None),
                _ => (sets, lo, hi, None, None),
            }
        }
    }
}

/// Choose the best exercise for a muscle group given mode + location, or `None`
/// if nothing that trains it as a primary is doable here.
fn pick_for_group(
    input: &PacingInput,
    group_id: i64,
    group_name: &str,
    now: NaiveDateTime,
) -> Option<Suggestion> {
    let mode = input.mode;
    let avail = input.available_equipment.as_ref();
    let doable = |eq: &[i64]| avail.is_none_or(|a| eq.iter().all(|e| a.contains(e)));
    let trains = |e: &ExerciseInfo| {
        e.groups
            .iter()
            .any(|(g, r)| *g == group_id && *r == MuscleRole::Primary)
    };
    // Fresher stimulus scores higher (0..1 over ~3 weeks); never-done = max.
    let recency = |id: i64| -> f64 {
        match input
            .history
            .iter()
            .filter(|s| s.exercise_id == id)
            .map(|s| s.logged_at)
            .max()
        {
            Some(t) => ((now - t).num_hours() as f64 / 24.0).min(21.0) / 21.0,
            None => 1.0,
        }
    };
    let score = |e: &ExerciseInfo| mode_fit(mode, e) * 2.0 + recency(e.id);

    // Deterministic best (score desc, id asc) over a filter.
    let best = |f: &dyn Fn(&ExerciseInfo) -> bool| -> Option<&ExerciseInfo> {
        input.exercises.iter().filter(|e| f(e)).max_by(|a, b| {
            score(a)
                .partial_cmp(&score(b))
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(b.id.cmp(&a.id)) // lower id wins ties (reverse in max)
        })
    };

    let ideal = best(&|e| trains(e));
    let chosen = best(&|e| trains(e) && doable(&e.equipment))?;
    let substituted_for = match (avail, ideal) {
        (Some(_), Some(i)) if i.id != chosen.id => Some(i.name.clone()),
        _ => None,
    };
    let (sets, rep_low, rep_high, load_kg, hold_s) =
        progress(chosen, input.last_perf.get(&chosen.id), mode);
    Some(Suggestion {
        exercise_id: chosen.id,
        exercise_name: chosen.name.clone(),
        pattern: chosen.pattern,
        sets,
        rep_low,
        rep_high,
        load_kg,
        hold_s,
        group: group_name.to_string(),
        substituted_for,
    })
}

/// Evaluate the coach verdict for `now` (local time).
pub fn evaluate(input: &PacingInput, now: NaiveDateTime) -> PacingNow {
    let s = &input.settings;
    let hour = now.hour() as i32;
    let within_window = hour >= s.window_start_hour && hour < s.window_end_hour;
    let after_cutoff = hour >= s.night_cutoff_hour;
    let minutes_since_last_set = input.last_set_at.map(|t| (now - t).num_minutes());
    let spacing_ok = minutes_since_last_set.is_none_or(|m| m >= s.min_rest_min as i64);

    let ex_by_id: HashMap<i64, &ExerciseInfo> = input.exercises.iter().map(|e| (e.id, e)).collect();

    // --- credit volume into rolling / 8-week-avg / recovery windows ---
    let roll_cut = now - Duration::days(ROLLING_DAYS);
    let hist_cut = now - Duration::days(HISTORY_WEEKS * 7);
    let recov_cut = now - Duration::hours(RECOVERY_HOURS);
    let today = now.date();

    let mut current: HashMap<i64, f64> = HashMap::new();
    let mut avg_sum: HashMap<i64, f64> = HashMap::new();
    let mut recent: HashMap<i64, f64> = HashMap::new();
    let mut done_today = 0i32;
    let mut raw_hist = 0i32;
    for set in &input.history {
        let Some(ex) = ex_by_id.get(&set.exercise_id) else {
            continue;
        };
        if set.logged_at.date() == today {
            done_today += 1;
        }
        if set.logged_at >= hist_cut {
            raw_hist += 1;
        }
        for (g, role) in &ex.groups {
            let credit = if *role == MuscleRole::Primary {
                1.0
            } else {
                SECONDARY_CREDIT
            };
            if set.logged_at >= hist_cut {
                *avg_sum.entry(*g).or_default() += credit;
            }
            if set.logged_at >= roll_cut {
                *current.entry(*g).or_default() += credit;
            }
            if set.logged_at >= recov_cut {
                *recent.entry(*g).or_default() += credit;
            }
        }
    }

    // --- auto-deload: recent volume well above the personal weekly average ---
    let avg_weekly_total: f64 = avg_sum.values().sum::<f64>() / HISTORY_WEEKS as f64;
    let last7_total: f64 = current.values().sum();
    let deload = avg_weekly_total > 0.0 && last7_total > DELOAD_RATIO * avg_weekly_total;
    let deload_scale = if deload { DELOAD_SCALE } else { 1.0 };
    let days_scale = (input.days_per_week as f64 / 4.0).clamp(0.5, 2.0);

    // --- per-group balance + focus candidates (non-recovering, in deficit) ---
    let mut balances: Vec<GroupBalance> = Vec::new();
    let mut candidates: Vec<(i64, String, f64)> = Vec::new();
    for gm in &input.groups {
        let cur = *current.get(&gm.id).unwrap_or(&0.0);
        let avg = avg_sum.get(&gm.id).copied().unwrap_or(0.0) / HISTORY_WEEKS as f64;
        let emph = if input.emphasis == Some(gm.region) {
            EMPHASIS_MULT
        } else {
            1.0
        };
        let base = 0.5 * DEFAULT_WEEKLY_SETS + 0.5 * avg;
        let target =
            (base * region_mult(input.mode, gm.region) * emph * days_scale * deload_scale).max(3.0);
        let deficit = ((target - cur) / target).clamp(0.0, 1.0);
        let recovering = *recent.get(&gm.id).unwrap_or(&0.0) >= RECOVERY_SETS;
        if !recovering && deficit > 0.0 {
            candidates.push((gm.id, gm.name.clone(), deficit));
        }
        balances.push(GroupBalance {
            group: gm.name.clone(),
            region: gm.region,
            current: cur,
            target,
            deficit,
            recovering,
        });
    }
    // Balance view: most-in-deficit first.
    balances.sort_by(|a, b| {
        b.deficit
            .partial_cmp(&a.deficit)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    // Focus order: biggest deficit first, tie-break group id.
    candidates.sort_by(|a, b| {
        b.2.partial_cmp(&a.2)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.0.cmp(&b.0))
    });

    // First focus group with a doable exercise wins.
    let mut suggestion = None;
    for (gid, gname, _) in &candidates {
        if let Some(sug) = pick_for_group(input, *gid, gname, now) {
            suggestion = Some(sug);
            break;
        }
    }

    let state = if input.history.is_empty() {
        PacingState::Fresh
    } else if suggestion.is_some() {
        PacingState::Active
    } else {
        PacingState::Rest
    };

    // --- session-size target from personal weekly volume, for the burn-down ---
    let avg_weekly_sets = if raw_hist > 0 {
        raw_hist as f64 / HISTORY_WEEKS as f64
    } else {
        24.0 // cold-start default ≈ 6 sets × 4 days
    };
    let day_target_sets =
        ((avg_weekly_sets / input.days_per_week.max(1) as f64).round() as i32).clamp(3, 15);

    // Burn-down vs window elapsed → nudge when behind (never dump the day at night).
    let now_min = (hour * 60 + now.minute() as i32) as f64;
    let win_start = (s.window_start_hour * 60) as f64;
    let win_end = (s.window_end_hour * 60).max(s.window_start_hour * 60 + 1) as f64;
    let elapsed = ((now_min - win_start) / (win_end - win_start)).clamp(0.0, 1.0);
    let has_work = suggestion.is_some() && done_today < day_target_sets;
    let behind = has_work && (done_today as f64) < elapsed * day_target_sets as f64;
    let nudge = within_window && !after_cutoff && has_work && spacing_ok && behind;

    let reason = if suggestion.is_none() {
        if state == PacingState::Rest {
            "You're balanced and recovered — rest up, or an easy optional set.".to_string()
        } else {
            "Nothing doable here right now.".to_string()
        }
    } else if !has_work {
        "You're on top of it today — nice work.".to_string()
    } else if after_cutoff {
        "It's late — this rolls to tomorrow.".to_string()
    } else if !within_window {
        format!(
            "Outside your training window ({:02}:00–{:02}:00).",
            s.window_start_hour, s.window_end_hour
        )
    } else if !spacing_ok {
        format!(
            "Just trained {}m ago — take a breather.",
            minutes_since_last_set.unwrap_or(0)
        )
    } else if let Some(sug) = &suggestion {
        if behind {
            format!(
                "{} × {} ({}) — you're a bit light there this week.",
                sug.sets, sug.exercise_name, sug.group
            )
        } else {
            format!(
                "Next up: {} × {} ({}).",
                sug.sets, sug.exercise_name, sug.group
            )
        }
    } else {
        String::new()
    };

    PacingNow {
        state,
        mode: input.mode,
        deload,
        nudge,
        reason,
        within_window,
        after_cutoff,
        spacing_ok,
        minutes_since_last_set,
        day_target_sets,
        day_done_sets: done_today,
        groups: balances,
        suggestion,
    }
}
