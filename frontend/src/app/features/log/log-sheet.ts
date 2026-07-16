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

  readonly exercises = this.data.exercises;
  readonly exerciseId = signal<number | null>(
    this.data.prefill?.exerciseId ?? this.exercises[0]?.id ?? null,
  );
  reps: number | null = this.data.prefill?.reps ?? null;
  loadKg: number | null = this.data.prefill?.loadKg ?? null;
  holdS: number | null = this.data.prefill?.holdS ?? null;
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

  save(): void {
    const id = this.exerciseId();
    if (id == null) return;
    this.saving.set(true);
    this.api
      .logSet({
        exerciseId: id,
        reps: this.reps,
        loadKg: this.loadKg,
        holdS: this.holdS,
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
