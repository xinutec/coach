import { Component, computed, inject, signal } from "@angular/core";
import { MatButtonModule } from "@angular/material/button";
import { MatIconModule } from "@angular/material/icon";
import { forkJoin } from "rxjs";

import { CoachApi } from "../../coach-api";
import { Exercise, WorkoutSet } from "../../models";

interface DayGroup {
  key: string;
  label: string;
  sets: WorkoutSet[];
}

@Component({
  selector: "app-history",
  templateUrl: "./history.html",
  styleUrl: "./history.scss",
  imports: [MatButtonModule, MatIconModule],
})
export class HistoryPage {
  private api = inject(CoachApi);

  readonly sets = signal<WorkoutSet[]>([]);
  readonly exMap = signal<Map<number, Exercise>>(new Map());
  readonly loading = signal(true);
  // Which day groups are expanded (terse by default; newest opens on load).
  readonly expanded = signal<Set<string>>(new Set());

  // logged_at is stored UTC; append 'Z' so the browser renders local time.
  private local(loggedAt: string): Date {
    return new Date(loggedAt + "Z");
  }

  /** Weekday + day + month, plus the year only when it isn't the current one. */
  private dayLabel(d: Date): string {
    const opts: Intl.DateTimeFormatOptions = { weekday: "short", day: "numeric", month: "short" };
    if (d.getFullYear() !== new Date().getFullYear()) opts.year = "numeric";
    return d.toLocaleDateString([], opts);
  }

  readonly groups = computed<DayGroup[]>(() => {
    const byDay = new Map<string, WorkoutSet[]>();
    for (const s of this.sets()) {
      const d = this.local(s.loggedAt);
      const key = `${d.getFullYear()}-${d.getMonth()}-${d.getDate()}`;
      const list = byDay.get(key);
      if (list) list.push(s);
      else byDay.set(key, [s]);
    }
    return [...byDay.entries()].map(([key, sets]) => ({
      key,
      label: this.dayLabel(this.local(sets[0].loggedAt)),
      sets,
    }));
  });

  constructor() {
    this.reload();
  }

  reload(): void {
    this.loading.set(true);
    forkJoin({
      sets: this.api.sets(100),
      exercises: this.api.exercises(true),
    }).subscribe({
      next: ({ sets, exercises }) => {
        this.sets.set(sets);
        this.exMap.set(new Map(exercises.map((e) => [e.id, e])));
        // Open the most recent day by default; the rest stay collapsed.
        const first = this.groups()[0];
        this.expanded.set(new Set(first ? [first.key] : []));
        this.loading.set(false);
      },
      error: () => this.loading.set(false),
    });
  }

  isOpen(key: string): boolean {
    return this.expanded().has(key);
  }

  toggle(key: string): void {
    const next = new Set(this.expanded());
    if (next.has(key)) next.delete(key);
    else next.add(key);
    this.expanded.set(next);
  }

  name(id: number): string {
    return this.exMap().get(id)?.name ?? "Exercise";
  }

  detail(s: WorkoutSet): string {
    const parts: string[] = [];
    if (s.reps != null) parts.push(`${s.reps} reps`);
    if (s.loadKg != null) parts.push(`${s.loadKg} kg`);
    if (s.holdS != null) parts.push(`${s.holdS}s`);
    if (s.rpe != null) parts.push(`RPE ${s.rpe}`);
    return parts.join(" · ");
  }

  time(s: WorkoutSet): string {
    return this.local(s.loggedAt).toLocaleTimeString([], {
      hour: "2-digit",
      minute: "2-digit",
    });
  }

  del(s: WorkoutSet): void {
    this.api
      .deleteSet(s.id)
      .subscribe(() => this.sets.set(this.sets().filter((x) => x.id !== s.id)));
  }
}
