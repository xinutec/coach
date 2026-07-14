import { WorkoutSet } from "../../models";

/** One movement within a day: every set of it, and the one line that says what it
 *  was. Three identical rows reading "Triceps extension" is the log describing
 *  itself instead of the training. */
export interface MovementGroup {
  key: string;
  exerciseId: number;
  summary: string;
  sets: WorkoutSet[];
}

/**
 * A day's sets, collected per movement, in the order they were trained.
 *
 * Pure, so the thing worth getting right — the summary line — is testable without
 * a component, a store or a browser.
 */
export function byMovement(
  dayKey: string,
  sets: readonly WorkoutSet[],
  unilateral: (exerciseId: number) => boolean,
  at: (s: WorkoutSet) => number,
): MovementGroup[] {
  const byEx = new Map<number, WorkoutSet[]>();
  for (const s of sets) {
    const list = byEx.get(s.exerciseId);
    if (list) list.push(s);
    else byEx.set(s.exerciseId, [s]);
  }
  return [...byEx.entries()]
    .map(([exerciseId, list]) => {
      const chronological = [...list].sort((a, b) => at(a) - at(b));
      return {
        key: `${dayKey}:${exerciseId}`,
        exerciseId,
        summary: summarise(chronological, unilateral(exerciseId)),
        sets: chronological,
      };
    })
    .sort((a, b) => at(a.sets[0]) - at(b.sets[0]));
}

/**
 * One line for every set of a movement.
 *
 * Where the sets differ, this shows the **range** — `3 sets · 4–6 reps` — never an
 * average. The mean of 6, 5 and 4 reps is a set of 5 that never happened, and a log
 * that invents sets is not a log.
 */
export function summarise(sets: readonly WorkoutSet[], unilateral: boolean): string {
  const range = (pick: (s: WorkoutSet) => number | null, fmt: (v: string) => string): string => {
    const vals = sets.map(pick).filter((v): v is number => v != null);
    if (vals.length === 0) return "";
    const [lo, hi] = [Math.min(...vals), Math.max(...vals)];
    return fmt(lo === hi ? `${lo}` : `${lo}–${hi}`);
  };
  return [
    `${sets.length} set${sets.length === 1 ? "" : "s"}`,
    range(
      (s) => s.reps,
      (v) => `${v} reps`,
    ),
    range(
      (s) => s.loadKg,
      (v) => `${v} kg`,
    ),
    range(
      (s) => s.holdS,
      (v) => `${v}s`,
    ),
    // A single-arm movement's numbers are per side — one set is both arms.
    unilateral ? "each side" : "",
    range(
      (s) => s.rpe,
      (v) => `RPE ${v}`,
    ),
  ]
    .filter(Boolean)
    .join(" · ");
}
