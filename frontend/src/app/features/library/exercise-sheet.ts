import { Component, inject, signal } from "@angular/core";
import { MAT_BOTTOM_SHEET_DATA } from "@angular/material/bottom-sheet";
import { MatButtonModule } from "@angular/material/button";
import { MatIconModule } from "@angular/material/icon";
import { MatProgressBarModule } from "@angular/material/progress-bar";

import { CoachApi } from "../../coach-api";
import { ExerciseDetail } from "../../models";

export interface ExerciseSheetData {
  exerciseId: number;
}

/** Bottom sheet showing an exercise in full: image, muscles, equipment, demo. */
@Component({
  selector: "app-exercise-sheet",
  templateUrl: "./exercise-sheet.html",
  styleUrl: "./exercise-sheet.scss",
  imports: [MatButtonModule, MatIconModule, MatProgressBarModule],
})
export class ExerciseSheet {
  private api = inject(CoachApi);
  private data = inject<ExerciseSheetData>(MAT_BOTTOM_SHEET_DATA);
  readonly detail = signal<ExerciseDetail | null>(null);

  constructor() {
    this.api.exercise(this.data.exerciseId).subscribe((d) => this.detail.set(d));
  }

  imageUrl(id: number): string {
    return this.api.exerciseImageUrl(id);
  }
  displayName(d: ExerciseDetail): string {
    return d.variation ? `${d.name} (${d.variation})` : d.name;
  }
  primary(d: ExerciseDetail) {
    return d.muscles.filter((m) => m.role === "primary");
  }
  secondary(d: ExerciseDetail) {
    return d.muscles.filter((m) => m.role !== "primary");
  }
  patternLabel(p: string): string {
    return p.charAt(0).toUpperCase() + p.slice(1);
  }
}
