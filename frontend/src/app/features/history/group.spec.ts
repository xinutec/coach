import { describe, expect, it } from "vitest";

import { WorkoutSet } from "../../models";
import { byMovement, summarise } from "./group";

let nextId = 1;
function set(exerciseId: number, at: string, s: Partial<WorkoutSet> = {}): WorkoutSet {
  return {
    id: nextId++,
    exerciseId,
    loggedAt: at,
    reps: null,
    loadKg: null,
    holdS: null,
    distanceM: null,
    rpe: null,
    note: null,
    ...s,
  };
}
const time = (s: WorkoutSet) => new Date(s.loggedAt + "Z").getTime();

describe("summarise", () => {
  it("says the set count and the numbers once, not once per set", () => {
    const sets = [
      set(1, "2026-07-14T16:00:00", { reps: 6, loadKg: 5 }),
      set(1, "2026-07-14T16:02:00", { reps: 6, loadKg: 5 }),
      set(1, "2026-07-14T16:04:00", { reps: 6, loadKg: 5 }),
    ];
    expect(summarise(sets, false)).toBe("3 sets · 6 reps · 5 kg");
  });

  it("shows a range when the sets differ — never an average", () => {
    // The mean of 6, 5 and 4 is a set of 5 that never happened.
    const sets = [
      set(1, "2026-07-14T16:00:00", { reps: 6, loadKg: 5 }),
      set(1, "2026-07-14T16:02:00", { reps: 5, loadKg: 5 }),
      set(1, "2026-07-14T16:04:00", { reps: 4, loadKg: 7.5 }),
    ];
    const line = summarise(sets, false);
    expect(line).toBe("3 sets · 4–6 reps · 5–7.5 kg");
    expect(line).not.toContain("5 reps");
  });

  it("says a single-arm movement's numbers are per side", () => {
    const sets = [set(1, "2026-07-14T16:00:00", { loadKg: 12, holdS: 30 })];
    expect(summarise(sets, true)).toBe("1 set · 12 kg · 30s · each side");
  });

  it("carries a carry's weight and time together, and the RPE when there is one", () => {
    const sets = [
      set(1, "2026-07-14T16:00:00", { loadKg: 20, holdS: 30, rpe: 7 }),
      set(1, "2026-07-14T16:05:00", { loadKg: 20, holdS: 30, rpe: 8 }),
    ];
    expect(summarise(sets, false)).toBe("2 sets · 20 kg · 30s · RPE 7–8");
  });

  it("omits what the sets never carried", () => {
    // A bodyweight set has no load, and no RPE was given.
    expect(summarise([set(1, "2026-07-14T16:00:00", { reps: 3 })], false)).toBe("1 set · 3 reps");
  });

  /** Migration 0024 gave carries their own column so distance would stop living
   *  as prose in a note — "Distance becomes something coach can say". It was not
   *  said here: every farmer's walk read as its weight alone, and the metres the
   *  migration had just rescued were invisible on the one screen that reads the
   *  log back. */
  it("says how far a carry went", () => {
    const sets = [
      set(1, "2026-07-14T16:00:00", { loadKg: 24, distanceM: 10 }),
      set(1, "2026-07-14T16:03:00", { loadKg: 24, distanceM: 10 }),
    ];
    expect(summarise(sets, false)).toBe("2 sets · 24 kg · 10 m");
  });

  it("ranges the distance like everything else", () => {
    const sets = [
      set(1, "2026-07-14T16:00:00", { loadKg: 24, distanceM: 10 }),
      set(1, "2026-07-14T16:03:00", { loadKg: 24, distanceM: 14 }),
    ];
    expect(summarise(sets, false)).toBe("2 sets · 24 kg · 10–14 m");
  });

  /** The ten carries logged in July 2026 are seconds, deliberately never
   *  converted — nobody recorded a pace, so metres from seconds would be an
   *  invented number. Both units therefore exist in the log at once. */
  it("keeps a timed carry timed and a measured one measured", () => {
    const timed = [set(1, "2026-07-14T16:00:00", { loadKg: 24, holdS: 30 })];
    const walked = [set(1, "2026-07-20T16:00:00", { loadKg: 24, distanceM: 10 })];
    expect(summarise(timed, false)).toBe("1 set · 24 kg · 30s");
    expect(summarise(walked, false)).toBe("1 set · 24 kg · 10 m");
  });

  it("says both when a movement was trained each way in one day", () => {
    const sets = [
      set(1, "2026-07-14T16:00:00", { loadKg: 24, holdS: 30 }),
      set(1, "2026-07-14T16:03:00", { loadKg: 24, distanceM: 10 }),
    ];
    expect(summarise(sets, false)).toBe("2 sets · 24 kg · 30s · 10 m");
  });
});

describe("byMovement", () => {
  it("collapses repeats of one movement into a single group", () => {
    const sets = [
      set(1, "2026-07-14T16:00:00", { reps: 3 }),
      set(1, "2026-07-14T16:03:00", { reps: 3 }),
      set(2, "2026-07-14T16:06:00", { reps: 6 }),
      set(1, "2026-07-14T16:09:00", { reps: 3 }),
    ];
    const groups = byMovement("d", sets, () => false, time);
    const [first] = groups;
    expect(groups.map((g) => g.exerciseId)).toEqual([1, 2]);
    expect(first?.sets).toHaveLength(3);
    expect(first?.summary).toBe("3 sets · 3 reps");
  });

  it("orders movements by when they were first trained, and their sets in order", () => {
    // Fed newest-first (as the API returns them) — the day should still read in the
    // order it was actually done.
    const sets = [
      set(2, "2026-07-14T17:00:00"),
      set(1, "2026-07-14T16:30:00"),
      set(1, "2026-07-14T16:00:00"),
    ];
    const groups = byMovement("d", sets, () => false, time);
    const [first] = groups;
    expect(groups.map((g) => g.exerciseId)).toEqual([1, 2]);
    expect(first?.sets.map((s) => s.loggedAt)).toEqual([
      "2026-07-14T16:00:00",
      "2026-07-14T16:30:00",
    ]);
  });
});
