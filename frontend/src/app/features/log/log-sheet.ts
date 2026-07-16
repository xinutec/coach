import { Component, computed, inject, signal } from "@angular/core";
import { FormsModule } from "@angular/forms";
import {
  MAT_BOTTOM_SHEET_DATA,
  MatBottomSheetRef,
} from "@angular/material/bottom-sheet";
import { MatButtonModule } from "@angular/material/button";
import { MatFormFieldModule } from "@angular/material/form-field";
import { MatInputModule } from "@angular/material/input";
import { MatSelectModule } from "@angular/material/select";

import { CoachApi } from "../../coach-api";
import { Exercise, displayName } from "../../models";

export interface LogPrefill {
  exerciseId: number;
  reps?: number | null;
  loadKg?: number | null;
  holdS?: number | null;
}
export interface LogSheetData {
  exercises: Exercise[];
  prefill?: LogPrefill;
  /** Today's prescriptions, one per planned exercise — switching the sheet to
   *  a planned movement lands on its numbers; anything else starts blank. */
  planPrefills?: LogPrefill[];
  /** Called after each set lands, so the page behind can refresh while the
   *  sheet stays up. */
  onLogged?: () => void;
}

/** Fast "log a set" bottom sheet. Fields shown adapt to the exercise's metric.
 *
 *  The sheet stays open across sets: sets come in runs, and a sheet that
 *  dismisses itself after each one swallows the tap meant for it — that's how
 *  a "Log set" tap once landed on the History tab underneath, and a typed
 *  edit was once lost so the *prefilled* value logged silently. Only an
 *  explicit Done (or the backdrop) closes it. */
@Component({
  selector: "app-log-sheet",
  templateUrl: "./log-sheet.html",
  styleUrl: "./log-sheet.scss",
  imports: [
    FormsModule,
    MatButtonModule,
    MatFormFieldModule,
    MatInputModule,
    MatSelectModule,
  ],
})
export class LogSheet {
  private api = inject(CoachApi);
  private ref = inject<MatBottomSheetRef<LogSheet, number>>(MatBottomSheetRef);
  readonly data = inject<LogSheetData>(MAT_BOTTOM_SHEET_DATA);

  /** Planned movements first, in plan order — mid-workout the next exercise is
   *  almost always one of these — then the rest alphabetically. The raw catalog
   *  order made the picker a 120-item scroll hunt. */
  readonly exercises: Exercise[] = (() => {
    const all = this.data.exercises;
    const planIds = (this.data.planPrefills ?? []).map((p) => p.exerciseId);
    const planned = planIds
      .map((id) => all.find((e) => e.id === id))
      .filter((e): e is Exercise => e !== undefined);
    const rest = all
      .filter((e) => !planIds.includes(e.id))
      .sort((a, b) => displayName(a).localeCompare(displayName(b)));
    return [...planned, ...rest];
  })();

  readonly exerciseId = signal<number | null>(
    this.data.prefill?.exerciseId ?? this.exercises[0]?.id ?? null,
  );
  readonly reps = signal<number | null>(this.data.prefill?.reps ?? null);
  readonly loadKg = signal<number | null>(this.data.prefill?.loadKg ?? null);
  readonly holdS = signal<number | null>(this.data.prefill?.holdS ?? null);
  readonly note = signal("");
  readonly saving = signal(false);
  /** Sets logged since the sheet opened — the run this sheet represents. */
  readonly logged = signal(0);

  readonly selected = computed(
    () => this.exercises.find((e) => e.id === this.exerciseId()) ?? null,
  );

  displayName(e: Exercise): string {
    return displayName(e);
  }

  /** Switching movements re-derives every field: the plan's prescription for a
   *  planned movement, blank otherwise. Nothing survives the switch — a stale
   *  value behind a *hidden* field once logged "10 reps · 4 kg" against a
   *  bodyweight drill, invisible at log time (field-test R2-1). */
  onExercise(id: number): void {
    this.exerciseId.set(id);
    const p =
      this.data.prefill?.exerciseId === id
        ? this.data.prefill
        : this.data.planPrefills?.find((x) => x.exerciseId === id);
    this.reps.set(p?.reps ?? null);
    this.loadKg.set(p?.loadKg ?? null);
    this.holdS.set(p?.holdS ?? null);
  }

  save(): void {
    const ex = this.selected();
    if (ex === null) return;
    const m = ex.metric;
    this.saving.set(true);
    this.api
      .logSet({
        exerciseId: ex.id,
        // Only the fields the metric owns — the server rejects the rest, and a
        // value the form isn't showing must never ride along.
        reps: m === "reps" || m === "weighted_reps" ? this.reps() : null,
        loadKg: m === "weighted_reps" || m === "weighted_hold" ? this.loadKg() : null,
        holdS: m === "hold" || m === "weighted_hold" ? this.holdS() : null,
        // Never asked for, so never sent. The wire field stays (the ability model
        // reads an RPE when history has one — imported sets do), but the app does
        // not solicit a self-rating of effort. See docs/trainer.md.
        rpe: null,
        note: this.note().trim() || null,
        loggedAt: null,
      })
      .subscribe({
        // Keep the sheet up with the same numbers — the next set of a run is
        // usually the same prescription. The page behind refreshes underneath.
        next: () => {
          this.logged.update((n) => n + 1);
          this.note.set("");
          this.saving.set(false);
          this.data.onLogged?.();
        },
        error: () => this.saving.set(false),
      });
  }

  done(): void {
    this.ref.dismiss(this.logged());
  }
}
