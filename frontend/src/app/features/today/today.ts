import { Component, inject, signal } from "@angular/core";
import { FormsModule } from "@angular/forms";
import { RouterLink } from "@angular/router";
import { MatBottomSheet } from "@angular/material/bottom-sheet";
import { MatButtonModule } from "@angular/material/button";
import { MatCardModule } from "@angular/material/card";
import { MatFormFieldModule } from "@angular/material/form-field";
import { MatIconModule } from "@angular/material/icon";
import { MatProgressBarModule } from "@angular/material/progress-bar";
import { MatSelectModule } from "@angular/material/select";
import { forkJoin } from "rxjs";

import { CoachApi } from "../../coach-api";
import { Exercise, Location, PacingNow } from "../../models";
import { LogSheet, LogSheetData } from "../log/log-sheet";

@Component({
  selector: "app-today",
  templateUrl: "./today.html",
  styleUrl: "./today.scss",
  imports: [
    FormsModule,
    RouterLink,
    MatButtonModule,
    MatCardModule,
    MatFormFieldModule,
    MatIconModule,
    MatProgressBarModule,
    MatSelectModule,
  ],
})
export class Today {
  private api = inject(CoachApi);
  private sheet = inject(MatBottomSheet);

  readonly pacing = signal<PacingNow | null>(null);
  readonly exercises = signal<Exercise[]>([]);
  readonly locations = signal<Location[]>([]);
  readonly loading = signal(true);
  readonly starting = signal(false);
  // null = "Anywhere" (no location filter). Initialised to the default location.
  readonly selectedLocationId = signal<number | null>(null);
  private equipmentNames = signal<Map<string, string>>(new Map());

  constructor() {
    this.loadAll();
  }

  /** Load the static context (exercises, locations, kit names) then the pacing
   *  verdict for the default location. */
  loadAll(): void {
    this.loading.set(true);
    forkJoin({
      exercises: this.api.exercises(),
      locations: this.api.locations(),
      equipment: this.api.equipment(),
    }).subscribe({
      next: ({ exercises, locations, equipment }) => {
        this.exercises.set(exercises);
        this.locations.set(locations);
        this.equipmentNames.set(new Map(equipment.map((e) => [e.slug, e.name])));
        const def = locations.find((l) => l.isDefault);
        this.selectedLocationId.set(def ? def.id : null);
        this.reloadPacing();
      },
      error: () => this.loading.set(false),
    });
  }

  reloadPacing(): void {
    this.api.pacingNow(this.selectedLocationId() ?? undefined).subscribe({
      next: (p) => {
        this.pacing.set(p);
        this.loading.set(false);
      },
      error: () => this.loading.set(false),
    });
  }

  onLocationChange(id: number | null): void {
    this.selectedLocationId.set(id);
    this.reloadPacing();
  }

  /** Equipment (display names) the suggested exercise needs, for pills. */
  suggestionEquipment(): string[] {
    const s = this.pacing()?.suggestion;
    if (!s) return [];
    const ex = this.exercises().find((e) => e.id === s.exerciseId);
    if (!ex) return [];
    return ex.equipment.map((slug) => this.equipmentNames().get(slug) ?? slug);
  }

  patternLabel(p: string): string {
    return p.charAt(0).toUpperCase() + p.slice(1);
  }

  pct(done: number, target: number): number {
    return target > 0 ? Math.min(100, Math.round((done / target) * 100)) : 0;
  }

  startStarter(): void {
    this.starting.set(true);
    this.api.createStarter().subscribe({
      next: () => {
        this.starting.set(false);
        this.reloadPacing();
      },
      error: () => this.starting.set(false),
    });
  }

  openLog(fromSuggestion = false): void {
    const p = this.pacing();
    const data: LogSheetData = { exercises: this.exercises() };
    if (fromSuggestion && p?.suggestion) {
      data.prefill = {
        exerciseId: p.suggestion.exerciseId,
        reps: p.suggestion.repLow,
        loadKg: p.suggestion.loadKg,
        holdS: p.suggestion.holdS,
      };
    }
    this.sheet
      .open(LogSheet, { data })
      .afterDismissed()
      .subscribe((res) => {
        if (res) this.reloadPacing();
      });
  }
}
