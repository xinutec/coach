import { Component, computed, inject, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { MAT_BOTTOM_SHEET_DATA, MatBottomSheetRef } from '@angular/material/bottom-sheet';
import { MatButtonModule } from '@angular/material/button';
import { MatFormFieldModule } from '@angular/material/form-field';
import { MatInputModule } from '@angular/material/input';
import { MatSelectModule } from '@angular/material/select';

import { CoachApi } from '../../coach-api';
import { Exercise, displayName } from '../../models';

export interface LogPrefill {
  exerciseId: number;
  reps?: number | null;
  loadKg?: number | null;
  holdS?: number | null;
  distanceM?: number | null;
  /** Sets of this plan item still to do; when the last one lands, the sheet moves
   *  on to the next item. Absent off the plan. */
  sets?: number;
}
export interface LogSheetData {
  exercises: Exercise[];
  prefill?: LogPrefill;
  /** Today's unfinished plan items, in plan order: a lift's ramp-in and its work
   *  sets are two. Switching to a planned movement lands on its first item's
   *  numbers; anything else starts blank. `prefill`, when it is one of these
   *  objects, is where the sheet starts in the plan. */
  planPrefills?: LogPrefill[];
  /** Called after each set lands, so the page behind can refresh while the
   *  sheet stays up; resolves once it has. */
  onLogged?: () => void | Promise<void>;
  /** The numbers a movement's card asks now. A calibration's later sets are
   *  back-off work derived from the set just logged, so they exist only after the
   *  page behind has reloaded. */
  prefillFor?: (exerciseId: number) => LogPrefill | undefined;
}

/** The server's `{"error": "..."}` message, read rather than asserted: the ingress
 *  or the network can answer with HTML, other JSON, or nothing. */
function serverMessage(err: unknown): string | null {
  if (typeof err !== 'object' || err === null) return null;
  const body: unknown = (err as { error?: unknown }).error;
  if (typeof body !== 'object' || body === null) return null;
  const msg: unknown = (body as { error?: unknown }).error;
  return typeof msg === 'string' && msg !== '' ? msg : null;
}

/** Likewise the HTTP status: read it, don't assert it. */
function statusOf(err: unknown): number | null {
  if (typeof err !== 'object' || err === null) return null;
  const status: unknown = (err as { status?: unknown }).status;
  return typeof status === 'number' ? status : null;
}

/** Fast "log a set" bottom sheet. Fields shown adapt to the exercise's metric.
 *
 *  The sheet stays open across sets: sets come in runs, and a sheet that
 *  dismisses itself after each one swallows the tap meant for it (landing on
 *  the tab underneath, or losing a typed edit so the *prefilled* value logs).
 *  Only an explicit Done (or the backdrop) closes it. */
@Component({
  selector: 'app-log-sheet',
  templateUrl: './log-sheet.html',
  styleUrl: './log-sheet.scss',
  imports: [FormsModule, MatButtonModule, MatFormFieldModule, MatInputModule, MatSelectModule],
})
export class LogSheet {
  private api = inject(CoachApi);
  private ref = inject<MatBottomSheetRef<LogSheet, number>>(MatBottomSheetRef);
  readonly data = inject<LogSheetData>(MAT_BOTTOM_SHEET_DATA);

  /** Planned movements first, in plan order — mid-workout the next exercise is
   *  almost always one of these — then the rest alphabetically. */
  readonly exercises: Exercise[] = (() => {
    const all = this.data.exercises;
    const planIds = [...new Set((this.data.planPrefills ?? []).map((p) => p.exerciseId))];
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
  readonly distanceM = signal<number | null>(this.data.prefill?.distanceM ?? null);
  readonly note = signal('');
  readonly saving = signal(false);
  /** The server's objection to the last attempt, shown: a swallowed rejection
   *  looks exactly like a logged set. */
  readonly error = signal<string | null>(null);
  /** The server asking "are you sure?" about a load far past anything owned (409):
   *  a typo caught before the ability model takes it as a PR. */
  readonly confirmLoad = signal<string | null>(null);
  /** Sets logged since the sheet opened — the run this sheet represents. */
  readonly logged = signal(0);

  private readonly plan = this.data.planPrefills ?? [];
  /** The plan item being logged and the sets each has left: what moves the sheet
   *  on once an item is done. -1 off the plan. */
  private cursor = this.data.prefill ? this.plan.indexOf(this.data.prefill) : -1;
  private readonly left = this.plan.map((p) => p.sets ?? 0);

  readonly selected = computed(
    () => this.exercises.find((e) => e.id === this.exerciseId()) ?? null,
  );

  displayName(e: Exercise): string {
    return displayName(e);
  }

  /** Switching movements re-derives every field: the plan's prescription for a
   *  planned movement, blank otherwise. Nothing survives the switch: a stale
   *  value behind a *hidden* field would log invisibly (R2-1). */
  onExercise(id: number): void {
    this.cursor = this.plan.findIndex((x, i) => x.exerciseId === id && (this.left[i] ?? 0) > 0);
    this.fill(
      id,
      this.data.prefill?.exerciseId === id
        ? this.data.prefill
        : (this.plan[this.cursor] ?? this.plan.find((x) => x.exerciseId === id)),
    );
  }

  private fill(id: number, p: LogPrefill | undefined): void {
    this.exerciseId.set(id);
    this.error.set(null);
    this.reps.set(p?.reps ?? null);
    this.loadKg.set(p?.loadKg ?? null);
    this.holdS.set(p?.holdS ?? null);
    this.distanceM.set(p?.distanceM ?? null);
  }

  /** A set of the current plan item landed: once it has none left, go to the next
   *  item that does. A run stays put until its last set; after the last item, the
   *  sheet stays where it is. */
  private advance(): boolean {
    const cur = this.cursor;
    if (cur < 0 || this.plan[cur]?.exerciseId !== this.exerciseId()) return false;
    this.left[cur] = (this.left[cur] ?? 0) - 1;
    if ((this.left[cur] ?? 0) > 0) return false;
    const next = this.plan.findIndex((_, i) => i > cur && (this.left[i] ?? 0) > 0);
    const p = this.plan[next];
    if (!p) return false;
    this.cursor = next;
    this.fill(p.exerciseId, p);
    return true;
  }

  /** `confirmed` re-sends a load the server queried, with the athlete's yes. */
  save(confirmed = false): void {
    const ex = this.selected();
    if (ex === null) return;
    const m = ex.metric;
    this.saving.set(true);
    this.error.set(null);
    this.confirmLoad.set(null);
    this.api
      .logSet({
        exerciseId: ex.id,
        // Only the fields the metric owns — the server rejects the rest, and a
        // value the form isn't showing must never ride along.
        reps: m === 'reps' || m === 'weighted_reps' ? this.reps() : null,
        loadKg:
          m === 'weighted_reps' || m === 'weighted_hold' || m === 'weighted_distance'
            ? this.loadKg()
            : null,
        holdS: m === 'hold' || m === 'weighted_hold' ? this.holdS() : null,
        distanceM: m === 'weighted_distance' ? this.distanceM() : null,
        // Never asked for, so never sent. The wire field stays (the ability model
        // reads an RPE when history has one — imported sets do), but the app does
        // not solicit a self-rating of effort. See docs/trainer.md.
        rpe: null,
        note: this.note().trim() || null,
        loggedAt: null,
        confirmLoad: confirmed,
      })
      .subscribe({
        // Keep the sheet up: the next set of a run is the same prescription, and
        // a finished item hands over to the next. The page behind refreshes underneath.
        next: () => {
          this.logged.update((n) => n + 1);
          this.note.set('');
          this.saving.set(false);
          const id = this.exerciseId();
          const moved = this.advance();
          const reloaded = this.data.onLogged?.();
          // Staying on the card: take what it asks now, once the page knows.
          if (!moved && id !== null && reloaded) {
            void reloaded.then(() => {
              const p = this.data.prefillFor?.(id);
              if (p && this.exerciseId() === id) this.fill(id, p);
            });
          }
        },
        error: (err: unknown) => {
          this.saving.set(false);
          const msg = serverMessage(err);
          // 409 is the load-plausibility query, not a refusal: keep the typed
          // numbers exactly as they are and let him answer it.
          if (statusOf(err) === 409 && msg) {
            this.confirmLoad.set(msg);
            return;
          }
          this.error.set(msg ?? "That didn't save — try again");
        },
      });
  }

  done(): void {
    this.ref.dismiss(this.logged());
  }
}
