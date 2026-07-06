//! The pacing engine: a pure function from (fetched state, instant) to a pacing
//! verdict. No I/O, no clock — the caller passes `now` (already in the user's
//! local tz). This is where the "spread it across the day, don't cram at 10pm"
//! behaviour lives; the tests below pin every branch.

use std::collections::HashMap;

use chrono::{NaiveDateTime, Timelike};

use crate::exercise::types::Pattern;

use super::types::{PacingInput, PacingNow, PacingState, PatternProgress, Suggestion, TargetInfo};

fn ceil_div(a: i32, b: i32) -> i32 {
    debug_assert!(b > 0);
    (a + b - 1) / b
}

fn pattern_rank(p: Pattern) -> i32 {
    match p {
        Pattern::Push => 0,
        Pattern::Pull => 1,
        Pattern::Legs => 2,
        Pattern::Core => 3,
    }
}

/// Per-exercise working row (internal to a single evaluation).
struct Row<'a> {
    t: &'a TargetInfo,
    pattern: Pattern,
    exercise_id: i64,
    exercise_name: &'a str,
    equipment: &'a [i64],
    primary_muscles: &'a [i64],
    remaining_week: i32,
    today_target: i32,
    today_done: i32,
    today_remaining: i32,
}

/// Evaluate the pacing state for `now` (local time).
pub fn evaluate(input: &PacingInput, now: NaiveDateTime) -> PacingNow {
    let s = &input.settings;
    let hour = now.hour() as i32;
    let within_window = hour >= s.window_start_hour && hour < s.window_end_hour;
    let after_cutoff = hour >= s.night_cutoff_hour;
    let minutes_since_last_set = input.last_set_at.map(|t| (now - t).num_minutes());
    let spacing_ok = minutes_since_last_set.is_none_or(|m| m >= s.min_rest_min as i64);

    let base = |state, week_index, is_deload, reason: String| PacingNow {
        state,
        week_index,
        is_deload,
        nudge: false,
        reason,
        within_window,
        after_cutoff,
        spacing_ok,
        minutes_since_last_set,
        day_remaining_sets: 0,
        week_remaining_sets: 0,
        patterns: Vec::new(),
        suggestion: None,
    };

    let Some(prog) = &input.program else {
        return base(
            PacingState::NoProgram,
            None,
            false,
            "No active program — start one to get going.".to_string(),
        );
    };

    let days_since = (now.date() - prog.start_date).num_days();
    if days_since < 0 {
        return base(
            PacingState::NotStarted,
            None,
            false,
            format!("Your program starts {}.", prog.start_date),
        );
    }
    let week_index = (days_since / 7) as i32 + 1;
    if week_index > prog.weeks {
        return base(
            PacingState::Complete,
            Some(week_index),
            false,
            "Program complete — time for a new block.".to_string(),
        );
    }
    let is_deload = prog.deload_week == Some(week_index);
    let weekday_index = (days_since % 7) as i32; // 0 = program week's first day
    let days_left = 7 - weekday_index; // includes today; 1..=7

    let ex_by_id: HashMap<i64, &super::types::ExerciseInfo> =
        input.exercises.iter().map(|e| (e.id, e)).collect();

    let today = now.date();
    let mut done_week: HashMap<i64, i32> = HashMap::new();
    let mut done_today: HashMap<i64, i32> = HashMap::new();
    for set in &input.sets_this_week {
        *done_week.entry(set.exercise_id).or_default() += 1;
        if set.logged_at.date() == today {
            *done_today.entry(set.exercise_id).or_default() += 1;
        }
    }

    let mut rows: Vec<Row> = Vec::new();
    for t in input.targets.iter().filter(|t| t.week_index == week_index) {
        let Some(ex) = ex_by_id.get(&t.exercise_id) else {
            continue;
        };
        let dw = *done_week.get(&t.exercise_id).unwrap_or(&0);
        let dt = *done_today.get(&t.exercise_id).unwrap_or(&0);
        let remaining_week = (t.target_sets - dw).max(0);

        let pin_today: i32 = input
            .pins
            .iter()
            .filter(|p| p.exercise_id == t.exercise_id && p.weekday == weekday_index)
            .map(|p| p.sets)
            .sum();
        let future_pin: i32 = input
            .pins
            .iter()
            .filter(|p| p.exercise_id == t.exercise_id && p.weekday > weekday_index)
            .map(|p| p.sets)
            .sum();

        // Today's quota is computed from what remained at the START of today, so
        // it's stable through the day (logging a set doesn't ratchet the quota
        // up) while still catching up for volume missed on earlier days.
        let remaining_start = (t.target_sets - (dw - dt)).max(0);
        let floating_pool = (remaining_start - future_pin).max(0);
        let floating_today = ceil_div(floating_pool, days_left);
        // The bigger of the fair share and today's pin, capped at what's left.
        let quota = remaining_start.min(floating_today.max(pin_today));
        let today_done = dt;
        let today_remaining = (quota - dt).max(0);
        let today_target = quota.max(dt);

        rows.push(Row {
            t,
            pattern: ex.pattern,
            exercise_id: ex.id,
            exercise_name: &ex.name,
            equipment: ex.equipment.as_slice(),
            primary_muscles: ex.primary_muscles.as_slice(),
            remaining_week,
            today_target,
            today_done,
            today_remaining,
        });
    }

    // Per-pattern aggregation, in a stable push/pull/legs/core order.
    let mut patterns: Vec<PatternProgress> = Vec::new();
    for pat in [Pattern::Push, Pattern::Pull, Pattern::Legs, Pattern::Core] {
        let rs: Vec<&Row> = rows.iter().filter(|r| r.pattern == pat).collect();
        if rs.is_empty() {
            continue;
        }
        patterns.push(PatternProgress {
            pattern: pat,
            week_target: rs.iter().map(|r| r.t.target_sets).sum(),
            week_done: rs
                .iter()
                .map(|r| *done_week.get(&r.exercise_id).unwrap_or(&0))
                .sum(),
            today_target: rs.iter().map(|r| r.today_target).sum(),
            today_done: rs.iter().map(|r| r.today_done).sum(),
            today_remaining: rs.iter().map(|r| r.today_remaining).sum(),
        });
    }

    let day_remaining_sets: i32 = rows.iter().map(|r| r.today_remaining).sum();
    let week_remaining_sets: i32 = rows.iter().map(|r| r.remaining_week).sum();
    let day_target_total: i32 = rows.iter().map(|r| r.today_target).sum();
    let day_done_total: i32 = rows.iter().map(|r| r.today_done).sum();

    // Candidates ordered by need: most remaining today first, tie-broken by
    // pattern order then id.
    let mut candidates: Vec<&Row> = rows.iter().filter(|r| r.today_remaining > 0).collect();
    candidates.sort_by(|a, b| {
        b.today_remaining
            .cmp(&a.today_remaining)
            .then(pattern_rank(a.pattern).cmp(&pattern_rank(b.pattern)))
            .then(a.exercise_id.cmp(&b.exercise_id))
    });

    // At a location, a goal is doable iff all its equipment is available; empty
    // (bodyweight) is doable anywhere. Without a location, everything is doable.
    let available = input.available_equipment.as_ref();
    let doable =
        |equipment: &[i64]| available.is_none_or(|a| equipment.iter().all(|e| a.contains(e)));

    // Pick the concrete suggestion: the most-needed doable goal, or — when the
    // most-needed goal's kit is missing here — an equivalent movement (same
    // pattern, doable, ranked by shared primary muscles) swapped in for it.
    let mut suggestion = None;
    for r in candidates {
        let sug = if doable(r.equipment) {
            Some((r.exercise_id, r.exercise_name.to_string(), None))
        } else {
            find_substitute(input, r, available)
                .map(|sub| (sub.id, sub.name.clone(), Some(r.exercise_name.to_string())))
        };
        if let Some((exercise_id, exercise_name, substituted_for)) = sug {
            suggestion = Some(Suggestion {
                exercise_id,
                exercise_name,
                pattern: r.pattern,
                sets: r.today_remaining,
                rep_low: r.t.rep_low,
                rep_high: r.t.rep_high,
                load_kg: r.t.load_kg,
                hold_s: r.t.hold_s,
                substituted_for,
            });
            break;
        }
    }

    // Burn-down: how far through the active window are we, and are we behind the
    // ideal pace? Being behind is what triggers a nudge — so nudges cluster
    // earlier when you're falling behind, never dumping the day at night.
    let now_min = (hour * 60 + now.minute() as i32) as f64;
    let win_start = (s.window_start_hour * 60) as f64;
    let win_end = (s.window_end_hour * 60).max(s.window_start_hour * 60 + 1) as f64;
    let progress = ((now_min - win_start) / (win_end - win_start)).clamp(0.0, 1.0);
    let has_work = day_remaining_sets > 0;
    let behind = has_work && (day_done_total as f64) < progress * (day_target_total as f64);

    let nudge = within_window && !after_cutoff && has_work && spacing_ok && behind;

    let reason = if !has_work {
        "You're done for today — nice work.".to_string()
    } else if after_cutoff {
        format!(
            "{} set{} left, but it's late — they'll roll to tomorrow.",
            day_remaining_sets,
            plural(day_remaining_sets)
        )
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
                "{} set{} of {} — you're a bit behind for today.",
                sug.sets,
                plural(sug.sets),
                sug.exercise_name
            )
        } else {
            format!(
                "On track. Next up: {} × {} when you're ready.",
                sug.sets, sug.exercise_name
            )
        }
    } else {
        "On track.".to_string()
    };

    PacingNow {
        state: PacingState::Active,
        week_index: Some(week_index),
        is_deload,
        nudge,
        reason,
        within_window,
        after_cutoff,
        spacing_ok,
        minutes_since_last_set,
        day_remaining_sets,
        week_remaining_sets,
        patterns,
        suggestion,
    }
}

fn plural(n: i32) -> &'static str {
    if n == 1 { "" } else { "s" }
}

/// An equivalent movement for a goal exercise that can't be done at the current
/// location: same pattern, all its equipment available here, ranked by how many
/// primary muscles it shares with the blocked goal (then by lowest id). `None`
/// if no location filter is active or nothing suitable is available.
fn find_substitute<'a>(
    input: &'a PacingInput,
    blocked: &Row,
    available: Option<&std::collections::HashSet<i64>>,
) -> Option<&'a super::types::ExerciseInfo> {
    let a = available?;
    input
        .exercises
        .iter()
        .filter(|e| {
            e.id != blocked.exercise_id
                && e.pattern == blocked.pattern
                && e.equipment.iter().all(|x| a.contains(x))
        })
        .max_by_key(|e| {
            let overlap = e
                .primary_muscles
                .iter()
                .filter(|&&m| blocked.primary_muscles.contains(&m))
                .count();
            (overlap, std::cmp::Reverse(e.id))
        })
}
