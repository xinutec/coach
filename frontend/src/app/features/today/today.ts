import { Component, computed, effect, inject, signal } from "@angular/core";
import { FormsModule } from "@angular/forms";
import { MatBottomSheet } from "@angular/material/bottom-sheet";
import { MatButtonModule } from "@angular/material/button";
import { MatCardModule } from "@angular/material/card";
import { MatFormFieldModule } from "@angular/material/form-field";
import { MatIconModule } from "@angular/material/icon";
import { MatProgressBarModule } from "@angular/material/progress-bar";
import { MatSelectModule } from "@angular/material/select";
import { RouterLink } from "@angular/router";
import { CoachApi } from "../../coach-api";
import type { Band, GroupBalance, Mode, PacingNow } from "../../models";
import { EquipmentStore, ExercisesStore, LocationsStore } from "../../stores/catalog";
import { LogSheet, type LogSheetData } from "../log/log-sheet";

const MODES: Mode[] = ["balanced", "strength", "skills", "conditioning"];

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
	private exercisesStore = inject(ExercisesStore);
	private locationsStore = inject(LocationsStore);
	private equipmentStore = inject(EquipmentStore);

	readonly pacing = signal<PacingNow | null>(null);
	// Shared catalogs, retained across tab switches (see CachedResource).
	readonly exercises = computed(() => this.exercisesStore.value() ?? []);
	readonly locations = computed(() => this.locationsStore.value() ?? []);
	readonly loading = signal(true);

	readonly modes = MODES;
	readonly selectedMode = signal<Mode>("balanced");
	private readonly settingsMode = signal<Mode | null>(null);
	private didInit = false;

	// null = "Anywhere". Initialised to the default location, then upgraded to the
	// auto-detected current location (best-effort) unless the user has picked one.
	readonly selectedLocationId = signal<number | null>(null);
	readonly autoDetected = signal(false);
	private userPickedLocation = false;
	private equipmentNames = computed(
		() => new Map((this.equipmentStore.value() ?? []).map((e) => [e.slug, e.name])),
	);

	constructor() {
		this.loadAll();
		// The first pacing verdict needs BOTH the persisted mode (settings) and the
		// locations list (to pick the default location) — both feed one pacingNow
		// call. Wait for both, then initialise once. Retained catalogs make this
		// instant on a revisit; a cold load waits for the fetches. (Stores set
		// `loaded` even on failure, so this still fires and clears `loading`.)
		effect(() => {
			if (this.didInit) return;
			const mode = this.settingsMode();
			if (mode === null || !this.locationsStore.loaded()) return;
			this.didInit = true;
			this.selectedMode.set(mode);
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
		this.equipmentStore.refresh();
		this.api.settings().subscribe({
			next: (s) => this.settingsMode.set(s.mode),
			error: () => this.settingsMode.set("balanced"),
		});
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
		this.api
			.pacingNow(this.selectedLocationId() ?? undefined, this.selectedMode())
			.subscribe({
				next: (p) => {
					this.pacing.set(p);
					this.loading.set(false);
				},
				error: () => this.loading.set(false),
			});
	}

	onLocationChange(id: number | null): void {
		this.userPickedLocation = true;
		this.autoDetected.set(false);
		this.selectedLocationId.set(id);
		this.reloadPacing();
	}

	/** Switch mode: reflect immediately, persist as the new default, re-evaluate. */
	onModeChange(m: Mode): void {
		if (m === this.selectedMode()) return;
		this.selectedMode.set(m);
		this.api.patchSettings({ mode: m }).subscribe({ error: () => {} });
		this.reloadPacing();
	}

	/** The most-relevant groups for the Today balance strip (top of the sorted list). */
	topGroups(p: PacingNow): GroupBalance[] {
		return p.groups.slice(0, 6);
	}

	/** Equipment (display names) the suggested exercise needs, for pills. */
	suggestionEquipment(): string[] {
		const s = this.pacing()?.suggestion;
		if (!s) return [];
		const ex = this.exercises().find((e) => e.id === s.exerciseId);
		if (!ex) return [];
		return ex.equipment.map((slug) => this.equipmentNames().get(slug) ?? slug);
	}

	modeLabel(m: string): string {
		return m.charAt(0).toUpperCase() + m.slice(1);
	}

	readinessLabel(b: Band): string {
		return b === "high"
			? "Recovered"
			: b === "low"
				? "Low readiness"
				: "Steady";
	}
	readinessIcon(b: Band): string {
		return b === "high" ? "bolt" : b === "low" ? "bedtime" : "spa";
	}

	pct(current: number, target: number): number {
		return target > 0 ? Math.min(100, Math.round((current / target) * 100)) : 0;
	}
	round1(n: number): string {
		return (Math.round(n * 10) / 10).toString();
	}
	round0(n: number): string {
		return Math.round(n).toString();
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
