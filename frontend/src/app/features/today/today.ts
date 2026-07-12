import { Component, computed, effect, inject, signal } from "@angular/core";
import { MatBottomSheet } from "@angular/material/bottom-sheet";
import { MatButtonModule } from "@angular/material/button";
import { MatCardModule } from "@angular/material/card";
import { MatIconModule } from "@angular/material/icon";
import { MatMenuModule } from "@angular/material/menu";
import { MatProgressBarModule } from "@angular/material/progress-bar";
import { RouterLink } from "@angular/router";
import { CoachApi } from "../../coach-api";
import type { Band, Explanation, PacingNow, Suggestion } from "../../models";
import { ExercisesStore, LocationsStore } from "../../stores/catalog";
import { LogSheet, type LogSheetData } from "../log/log-sheet";

@Component({
	selector: "app-today",
	templateUrl: "./today.html",
	styleUrl: "./today.scss",
	imports: [
		MatButtonModule,
		MatCardModule,
		MatIconModule,
		MatMenuModule,
		MatProgressBarModule,
		RouterLink,
	],
})
export class Today {
	private api = inject(CoachApi);
	private sheet = inject(MatBottomSheet);
	private exercisesStore = inject(ExercisesStore);
	private locationsStore = inject(LocationsStore);

	readonly pacing = signal<PacingNow | null>(null);
	// Shared catalogs, retained across tab switches (see CachedResource).
	readonly exercises = computed(() => this.exercisesStore.value() ?? []);
	readonly locations = computed(() => this.locationsStore.value() ?? []);
	readonly loading = signal(true);
	private didInit = false;

	// The location whose kit bounds the session. Initialised to the default, then
	// upgraded to the auto-detected one (best-effort) unless the user has picked.
	// `null` only while locations are loading, or if there are none at all — the
	// engine then declines to plan rather than guessing what's doable.
	readonly selectedLocationId = signal<number | null>(null);
	readonly autoDetected = signal(false);
	private userPickedLocation = false;

	constructor() {
		this.loadAll();
		// The first pacing verdict needs the locations list (to pick the default
		// location). Wait for it, then initialise once. Retained catalogs make this
		// instant on a revisit; a cold load waits for the fetch. (Stores set
		// `loaded` even on failure, so this still fires and clears `loading`.)
		effect(() => {
			if (this.didInit || !this.locationsStore.loaded()) return;
			this.didInit = true;
			const def = this.locations().find((l) => l.isDefault);
			this.selectedLocationId.set(def ? def.id : null);
			this.reloadPacing();
			this.autoSelect();
		});
	}

	loadAll(): void {
		this.loading.set(true);
		this.exercisesStore.refresh();
		this.locationsStore.refresh();
	}

	/** Best-effort: switch to the auto-detected current location once it resolves. */
	private autoSelect(): void {
		this.api.locationCurrent().subscribe({
			next: (cur) => {
				if (cur.locationId == null || this.userPickedLocation) return;
				this.autoDetected.set(true);
				if (cur.locationId !== this.selectedLocationId()) {
					this.selectedLocationId.set(cur.locationId);
					this.reloadPacing();
				}
			},
			error: () => {},
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

	onLocationChange(id: number): void {
		this.userPickedLocation = true;
		this.autoDetected.set(false);
		this.selectedLocationId.set(id);
		this.reloadPacing();
	}

	/** Display name of the selected location for the status line. */
	locationName(): string {
		const id = this.selectedLocationId();
		const name = id == null ? undefined : this.locations().find((l) => l.id === id)?.name;
		return name ?? "No location";
	}

	// Which plan items have their "why this?" reasoning expanded (by exercise id).
	private readonly whyOpen = signal<ReadonlySet<number>>(new Set());
	isWhyOpen(id: number): boolean {
		return this.whyOpen().has(id);
	}
	toggleWhy(id: number): void {
		const next = new Set(this.whyOpen());
		if (next.has(id)) next.delete(id);
		else next.add(id);
		this.whyOpen.set(next);
	}

	/**
	 * Human-readable "why this?" lines from a suggestion's structured trace — the
	 * factors the engine actually weighed (deficit, recovery, ability, readiness).
	 */
	explanationLines(e: Explanation): string[] {
		const lines: string[] = [];
		const conf: Record<string, string> = {
			high: "You've trained this recently — confident estimate",
			medium: "A little recent data — estimate firming up",
			low: "Rusty here — working off older data",
			none: "New to you — calibrating from scratch",
		};
		lines.push(conf[e.confidence]);
		lines.push(`${Math.round(e.deficit * 100)}% below this week's target for this group`);
		lines.push(
			e.recovery >= 0.99 ? "Fully recovered" : `${Math.round(e.recovery * 100)}% recovered`,
		);
		if (e.e1rm !== null) lines.push(`Estimated 1-rep max ≈ ${Math.round(e.e1rm)} kg`);
		if (e.readiness) {
			const r: Record<Band, string> = {
				high: "Biometrics say recovered — a good day to push",
				normal: "Steady readiness",
				low: "Low readiness — easing the volume off",
			};
			lines.push(r[e.readiness]);
		}
		return lines;
	}

	/** One-line description for a warm-up item: a ramp-in set vs a mobility drill. */
	warmupNote(s: Suggestion): string {
		if (s.loadKg !== null) return `Ramp-in set · ${s.loadKg} kg — groove the movement`;
		return "Mobility — loosen up the muscles you're about to train";
	}

	/**
	 * The calibration instruction for an `assess` suggestion — what to actually do
	 * so the logged set measures your ability. Metric comes from the catalog (the
	 * wire suggestion doesn't carry it), so assess-reps and assess-hold differ.
	 */
	assessInstruction(exerciseId: number, repLow: number | null): string {
		const metric = this.exercises().find((e) => e.id === exerciseId)?.metric;
		if (metric === "hold") return "Hold as long as your form stays clean — one honest max.";
		if (metric === "weighted_reps")
			return `Build up to a hard-but-clean set of ${repLow ?? 5}, then log the load, reps and how hard it felt.`;
		return "As many clean reps as you can — stop at form breakdown, then log it.";
	}

	/** Open the log sheet, optionally prefilled from a specific plan item. */
	openLog(from?: Suggestion): void {
		const data: LogSheetData = { exercises: this.exercises() };
		if (from) {
			data.prefill = {
				exerciseId: from.exerciseId,
				reps: from.repLow,
				loadKg: from.loadKg,
				holdS: from.holdS,
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
