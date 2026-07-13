import { Overlay, type OverlayRef } from "@angular/cdk/overlay";
import { TemplatePortal } from "@angular/cdk/portal";
import { NgTemplateOutlet } from "@angular/common";
import {
  Component,
  DestroyRef,
  type TemplateRef,
  ViewContainerRef,
  computed,
  effect,
  inject,
  signal,
  viewChild,
} from "@angular/core";
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
  imports: [
    MatButtonModule,
    MatIconModule,
    MatProgressBarModule,
    MatProgressSpinnerModule,
    NgTemplateOutlet,
  ],
})
export class ExerciseSheet {
  private api = inject(CoachApi);
  private sanitizer = inject(DomSanitizer);
  private overlay = inject(Overlay);
  private vcr = inject(ViewContainerRef);
  private data = inject<ExerciseSheetData>(MAT_BOTTOM_SHEET_DATA);
  readonly detail = signal<ExerciseDetail | null>(null);

  private playerTpl = viewChild.required<TemplateRef<unknown>>("playerTpl");

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

  /**
   * Turning the phone sideways to watch something is the gesture everyone already
   * has, so landscape gives the demo the whole screen.
   *
   * It has to be the *viewport* it fills, not true fullscreen: `requestFullscreen`
   * demands transient user activation, and a rotation isn't one — the browser
   * would refuse. In the installed app there's no browser chrome, so filling the
   * viewport is the same picture.
   */
  readonly landscape = signal(false);

  constructor() {
    this.api.exercise(this.data.exerciseId).subscribe((d) => this.detail.set(d));

    const mq = window.matchMedia("(orientation: landscape)");
    this.landscape.set(mq.matches);
    const onChange = (e: MediaQueryListEvent) => this.landscape.set(e.matches);
    mq.addEventListener("change", onChange);
    inject(DestroyRef).onDestroy(() => {
      mq.removeEventListener("change", onChange);
      this.closeFullscreen();
    });

    // Sideways with a video running → lift the player out to full screen; upright
    // again → put it back in the sheet. Moving the frame reloads it, so playback
    // restarts — at the timestamp the link points to, which is the rep itself.
    effect(() => {
      if (this.playing() && this.landscape()) this.openFullscreen();
      else this.closeFullscreen();
    });
  }

  private fsRef: OverlayRef | null = null;

  private openFullscreen(): void {
    if (this.fsRef) return;
    this.frameReady.set(false);
    this.fsRef = this.overlay.create({
      panelClass: "demo-fs-pane",
      hasBackdrop: false,
      positionStrategy: this.overlay.position().global().top("0").left("0"),
      scrollStrategy: this.overlay.scrollStrategies.block(),
    });
    this.fsRef.attach(new TemplatePortal(this.playerTpl(), this.vcr));
  }

  private closeFullscreen(): void {
    this.fsRef?.dispose();
    this.fsRef = null;
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
