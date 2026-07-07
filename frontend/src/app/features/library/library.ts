import { Component, computed, inject, signal } from "@angular/core";
import { FormsModule } from "@angular/forms";
import { MatBottomSheet } from "@angular/material/bottom-sheet";
import { MatButtonModule } from "@angular/material/button";
import { MatFormFieldModule } from "@angular/material/form-field";
import { MatIconModule } from "@angular/material/icon";
import { MatInputModule } from "@angular/material/input";

import { CoachApi } from "../../coach-api";
import { Exercise } from "../../models";
import { EquipmentStore, ExercisesStore } from "../../stores/catalog";
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
  private exercisesStore = inject(ExercisesStore);
  private equipmentStore = inject(EquipmentStore);

  // Shared catalogs, retained across tab switches (see CachedResource).
  readonly exercises = computed(() => this.exercisesStore.value() ?? []);
  readonly loading = computed(() => !this.exercisesStore.loaded());
  // Signals (not plain fields) because the filtered view is a computed over them.
  readonly search = signal("");
  readonly pattern = signal<string | null>(null);
  readonly patterns = PATTERNS;
  private equipmentNames = computed(
    () => new Map((this.equipmentStore.value() ?? []).map((e) => [e.slug, e.name])),
  );

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
    this.exercisesStore.refresh();
    this.equipmentStore.refresh();
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
