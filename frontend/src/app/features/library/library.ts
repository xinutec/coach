import { Component, computed, inject, signal } from "@angular/core";
import { FormsModule } from "@angular/forms";
import { MatBottomSheet } from "@angular/material/bottom-sheet";
import { MatButtonModule } from "@angular/material/button";
import { MatFormFieldModule } from "@angular/material/form-field";
import { MatIconModule } from "@angular/material/icon";
import { MatInputModule } from "@angular/material/input";
import { forkJoin } from "rxjs";

import { CoachApi } from "../../coach-api";
import { Exercise } from "../../models";
import { ExerciseSheet } from "./exercise-sheet";

const PATTERNS = ["push", "pull", "legs", "core"] as const;

/** Browse the exercise library: search + pattern filter, tap for full detail. */
@Component({
  selector: "app-library",
  templateUrl: "./library.html",
  styleUrl: "./library.scss",
  imports: [FormsModule, MatButtonModule, MatFormFieldModule, MatIconModule, MatInputModule],
})
export class LibraryPage {
  private api = inject(CoachApi);
  private sheet = inject(MatBottomSheet);

  readonly exercises = signal<Exercise[]>([]);
  readonly loading = signal(true);
  // Signals (not plain fields) because the filtered view is a computed over them.
  readonly search = signal("");
  readonly pattern = signal<string | null>(null);
  readonly patterns = PATTERNS;
  private equipmentNames = signal<Map<string, string>>(new Map());

  readonly filtered = computed(() => {
    const q = this.search().trim().toLowerCase();
    const pat = this.pattern();
    return this.exercises().filter((e) => {
      if (pat && e.pattern !== pat) return false;
      if (!q) return true;
      const name = (e.variation ? `${e.name} ${e.variation}` : e.name).toLowerCase();
      return name.includes(q);
    });
  });

  constructor() {
    forkJoin({ exercises: this.api.exercises(), equipment: this.api.equipment() }).subscribe({
      next: ({ exercises, equipment }) => {
        this.exercises.set(exercises);
        this.equipmentNames.set(new Map(equipment.map((e) => [e.slug, e.name])));
        this.loading.set(false);
      },
      error: () => this.loading.set(false),
    });
  }

  displayName(e: Exercise): string {
    return e.variation ? `${e.name} (${e.variation})` : e.name;
  }
  equipLabel(slug: string): string {
    return this.equipmentNames().get(slug) ?? slug;
  }
  patternLabel(p: string): string {
    return p.charAt(0).toUpperCase() + p.slice(1);
  }
  imageUrl(id: number): string {
    return this.api.exerciseImageUrl(id);
  }
  togglePattern(p: string): void {
    this.pattern.set(this.pattern() === p ? null : p);
  }
  open(e: Exercise): void {
    this.sheet.open(ExerciseSheet, { data: { exerciseId: e.id } });
  }
}
