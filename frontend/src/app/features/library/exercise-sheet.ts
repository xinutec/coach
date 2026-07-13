import { Component, computed, inject, signal } from "@angular/core";
import { MAT_BOTTOM_SHEET_DATA } from "@angular/material/bottom-sheet";
import { MatButtonModule } from "@angular/material/button";
import { MatIconModule } from "@angular/material/icon";
import { MatProgressBarModule } from "@angular/material/progress-bar";
import { MatProgressSpinnerModule } from "@angular/material/progress-spinner";
import { DomSanitizer, type SafeResourceUrl } from "@angular/platform-browser";

import { CoachApi } from "../../coach-api";
import { ExerciseDetail } from "../../models";
import { embedUrl, parseYoutube } from "../../shared/youtube";

export interface ExerciseSheetData {
  exerciseId: number;
}

/** Bottom sheet showing an exercise in full: image, muscles, equipment, demo. */
@Component({
  selector: "app-exercise-sheet",
  templateUrl: "./exercise-sheet.html",
  styleUrl: "./exercise-sheet.scss",
  imports: [MatButtonModule, MatIconModule, MatProgressBarModule, MatProgressSpinnerModule],
})
export class ExerciseSheet {
  private api = inject(CoachApi);
  private sanitizer = inject(DomSanitizer);
  private data = inject<ExerciseSheetData>(MAT_BOTTOM_SHEET_DATA);
  readonly detail = signal<ExerciseDetail | null>(null);

  /** The demo, when it's a video we can actually play in here. `null` → the demo
   *  link (if there is one) can only open out to YouTube. */
  readonly video = computed(() => {
    const url = this.detail()?.demoUrl;
    return url ? parseYoutube(url) : null;
  });

  /**
   * Set once the athlete taps play — the frame is built then, never on open. The
   * picture is the offline-safe answer to "what is this movement again?" (the
   * service worker caches it; a YouTube embed can't be, so in a basement gym the
   * video is exactly the thing that fails). So: picture first, video on demand,
   * and no call to Google for a movement you only glanced at.
   */
  readonly playing = signal<SafeResourceUrl | null>(null);

  /** The embed document has arrived. Until it does the frame paints black, which
   *  reads as a broken tap — so the still stays up, with a spinner, until this. */
  readonly frameReady = signal(false);

  constructor() {
    this.api.exercise(this.data.exerciseId).subscribe((d) => this.detail.set(d));
  }

  play(): void {
    const ref = this.video();
    if (!ref) return;
    this.frameReady.set(false);
    this.playing.set(this.sanitizer.bypassSecurityTrustResourceUrl(embedUrl(ref)));
  }

  /** Back to the picture. Dropping the frame is also what stops the playback —
   *  there's no player object to pause, and a video still running behind a closed
   *  sheet is a thing you'd have to go and find. */
  stop(): void {
    this.playing.set(null);
    this.frameReady.set(false);
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
