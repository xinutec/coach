import { Component, computed, inject, signal } from "@angular/core";
import { FormsModule } from "@angular/forms";
import { MatButtonModule } from "@angular/material/button";
import { MatFormFieldModule } from "@angular/material/form-field";
import { MatIconModule } from "@angular/material/icon";
import { MatInputModule } from "@angular/material/input";
import { MatSelectModule } from "@angular/material/select";
import { MatSlideToggleModule } from "@angular/material/slide-toggle";
import { CoachApi } from "../../coach-api";
import type { Category, Equipment, Location } from "../../models";
import { EquipmentStore, LocationsStore, PlacesStore } from "../../stores/catalog";

const CATEGORY_LABEL: Record<Category, string> = {
	free_weight: "Free weights",
	band: "Bands",
	machine: "Machines",
	ball: "Balls",
	rig: "Bars & rings",
	bench: "Bench",
};
const CATEGORY_ORDER: Category[] = [
	"free_weight",
	"rig",
	"bench",
	"machine",
	"band",
	"ball",
];

interface EquipmentSpecifics {
	weights: number[];
	labels: string[];
	// Loadable bars (barbell / trap bar): the bar's own weight + owned plate sizes.
	barKg: number | null;
	plates: number[];
}

/** Manage training locations: each is a named place with an equipment inventory.
 *  The Today view uses the selected location to decide what to do there. */
@Component({
	selector: "app-locations",
	templateUrl: "./locations.html",
	styleUrl: "./locations.scss",
	imports: [
		FormsModule,
		MatButtonModule,
		MatFormFieldModule,
		MatIconModule,
		MatInputModule,
		MatSelectModule,
		MatSlideToggleModule,
	],
})
export class LocationsPage {
	private api = inject(CoachApi);
	private locationsStore = inject(LocationsStore);
	private equipmentStore = inject(EquipmentStore);
	private placesStore = inject(PlacesStore);

	// Shared catalogs, retained across tab switches (see CachedResource).
	readonly locations = computed(() => this.locationsStore.value() ?? []);
	readonly equipment = computed(() => this.equipmentStore.value() ?? []);
	// health-detected places for the link picker (empty when the integration is off).
	readonly detectedPlaces = computed(() => this.placesStore.value() ?? []);
	readonly loading = computed(
		() =>
			!this.locationsStore.loaded() ||
			!this.equipmentStore.loaded() ||
			!this.placesStore.loaded(),
	);

	// Form state: editingId is null (hidden), 0 (new), or an id (editing).
	readonly editingId = signal<number | null>(null);
	readonly formName = signal("");
	readonly formDefault = signal(false);
	readonly formEquip = signal<Set<string>>(new Set());
	// Per-equipment specifics being edited: slug → owned weights / band variants.
	readonly formOptions = signal<Map<string, EquipmentSpecifics>>(new Map());
	readonly formHealthPlaceId = signal<number | null>(null);
	readonly saving = signal(false);

	/** Equipment grouped by category, in a stable order, for the picker. */
	readonly grouped = computed(() => {
		const byCat = new Map<Category, Equipment[]>();
		for (const e of this.equipment()) {
			const arr = byCat.get(e.category) ?? [];
			arr.push(e);
			byCat.set(e.category, arr);
		}
		return CATEGORY_ORDER.filter((c) => byCat.has(c)).map((c) => ({
			category: c,
			label: CATEGORY_LABEL[c],
			items: byCat.get(c)!,
		}));
	});

	constructor() {
		this.reload();
	}

	reload(): void {
		this.locationsStore.refresh();
		this.equipmentStore.refresh();
		this.placesStore.refresh();
	}

	placeLabel(id: number | null): string {
		if (id === null) return "";
		return this.detectedPlaces().find((p) => p.id === id)?.label ?? "";
	}

	private equipmentNames = computed(
		() => new Map(this.equipment().map((e) => [e.slug, e.name])),
	);
	equipLabel(slug: string): string {
		return this.equipmentNames().get(slug) ?? slug;
	}

	startNew(): void {
		this.editingId.set(0);
		this.formName.set("");
		this.formDefault.set(this.locations().length === 0);
		this.formEquip.set(new Set());
		this.formOptions.set(new Map());
		this.formHealthPlaceId.set(null);
	}

	startEdit(loc: Location): void {
		this.editingId.set(loc.id);
		this.formName.set(loc.name);
		this.formDefault.set(loc.isDefault);
		this.formEquip.set(new Set(loc.equipment));
		this.formOptions.set(
			new Map(
				loc.equipmentOptions.map((o) => [
					o.slug,
					{
						weights: [...o.weights],
						labels: [...o.labels],
						barKg: o.barKg,
						plates: [...o.plates],
					},
				]),
			),
		);
		this.formHealthPlaceId.set(loc.healthPlaceId);
	}

	cancel(): void {
		this.editingId.set(null);
	}

	toggleEquip(slug: string): void {
		const s = new Set(this.formEquip());
		if (s.has(slug)) {
			s.delete(slug);
			// Drop any specifics for kit that's no longer here.
			const m = new Map(this.formOptions());
			if (m.delete(slug)) this.formOptions.set(m);
		} else {
			s.add(slug);
		}
		this.formEquip.set(s);
	}

	categoryOf(slug: string): Category | null {
		return this.equipment().find((e) => e.slug === slug)?.category ?? null;
	}
	private isLoadable(slug: string): boolean {
		return this.equipment().find((e) => e.slug === slug)?.loadable ?? false;
	}

	/** Selected fixed free weights (dumbbell/kettlebell) — each gets a weights
	 *  editor. Loadable bars are handled separately (bar + plates). */
	readonly weightedSlugs = computed(() =>
		[...this.formEquip()].filter(
			(s) => this.categoryOf(s) === "free_weight" && !this.isLoadable(s),
		),
	);
	/** Selected loadable bars (barbell/trap bar) — each gets a bar + plates editor. */
	readonly loadableSlugs = computed(() =>
		[...this.formEquip()].filter((s) => this.isLoadable(s)),
	);
	/** Selected bands — each gets a named-variant editor. */
	readonly bandSlugs = computed(() =>
		[...this.formEquip()].filter((s) => this.categoryOf(s) === "band"),
	);

	weightsOf(slug: string): number[] {
		return this.formOptions().get(slug)?.weights ?? [];
	}
	labelsOf(slug: string): string[] {
		return this.formOptions().get(slug)?.labels ?? [];
	}
	barKgOf(slug: string): number | null {
		return this.formOptions().get(slug)?.barKg ?? null;
	}
	platesOf(slug: string): number[] {
		return this.formOptions().get(slug)?.plates ?? [];
	}

	private mutate(slug: string, fn: (s: EquipmentSpecifics) => void): void {
		const m = new Map(this.formOptions());
		const cur = m.get(slug) ?? {
			weights: [],
			labels: [],
			barKg: null,
			plates: [],
		};
		fn(cur);
		m.set(slug, cur);
		this.formOptions.set(m);
	}

	addWeight(slug: string, raw: string): void {
		const n = Number.parseFloat(raw);
		if (!Number.isFinite(n) || n <= 0) return;
		this.mutate(slug, (s) => {
			if (!s.weights.includes(n))
				s.weights = [...s.weights, n].sort((a, b) => a - b);
		});
	}
	/** Add a whole rack at once: from..to inclusive, in `step` increments. Lets a
	 *  full dumbbell set (e.g. 2.5–50 by 2.5) go in without tapping each weight. */
	addRange(slug: string, fromRaw: string, toRaw: string, stepRaw: string): void {
		const from = Number.parseFloat(fromRaw);
		const to = Number.parseFloat(toRaw);
		const step = Number.parseFloat(stepRaw);
		if (![from, to, step].every(Number.isFinite)) return;
		if (from <= 0 || step <= 0 || to < from) return;
		if ((to - from) / step > 200) return; // guard a runaway range
		this.mutate(slug, (s) => {
			const set = new Set(s.weights);
			for (let w = from; w <= to + 1e-6; w += step) {
				set.add(Math.round(w * 100) / 100);
			}
			s.weights = [...set].sort((a, b) => a - b);
		});
	}
	removeWeight(slug: string, n: number): void {
		this.mutate(slug, (s) => {
			s.weights = s.weights.filter((w) => w !== n);
		});
	}
	addLabel(slug: string, raw: string): void {
		const l = raw.trim();
		if (!l) return;
		this.mutate(slug, (s) => {
			if (!s.labels.includes(l)) s.labels = [...s.labels, l];
		});
	}
	removeLabel(slug: string, l: string): void {
		this.mutate(slug, (s) => {
			s.labels = s.labels.filter((x) => x !== l);
		});
	}
	/** Set (or clear) a loadable bar's own weight. */
	setBar(slug: string, raw: string | number | null): void {
		const n = typeof raw === "number" ? raw : Number.parseFloat(raw ?? "");
		this.mutate(slug, (s) => {
			s.barKg = Number.isFinite(n) && n > 0 ? n : null;
		});
	}
	addPlate(slug: string, raw: string): void {
		const n = Number.parseFloat(raw);
		if (!Number.isFinite(n) || n <= 0) return;
		this.mutate(slug, (s) => {
			if (!s.plates.includes(n)) s.plates = [...s.plates, n].sort((a, b) => a - b);
		});
	}
	removePlate(slug: string, n: number): void {
		this.mutate(slug, (s) => {
			s.plates = s.plates.filter((p) => p !== n);
		});
	}

	save(): void {
		const id = this.editingId();
		if (id === null) return;
		// Only keep specifics for kit that's still selected and actually has any.
		const equipmentOptions = [...this.formOptions().entries()]
			.filter(([slug]) => this.formEquip().has(slug))
			.map(([slug, o]) => ({
				slug,
				weights: o.weights,
				labels: o.labels,
				barKg: o.barKg,
				plates: o.plates,
			}))
			.filter(
				(o) =>
					o.weights.length > 0 ||
					o.labels.length > 0 ||
					o.barKg !== null ||
					o.plates.length > 0,
			);
		const body = {
			name: this.formName().trim() || "Location",
			isDefault: this.formDefault(),
			equipment: [...this.formEquip()],
			equipmentOptions,
			healthPlaceId: this.formHealthPlaceId(),
		};
		this.saving.set(true);
		const done = {
			next: () => {
				this.saving.set(false);
				this.editingId.set(null);
				this.reload();
			},
			error: () => this.saving.set(false),
		};
		if (id === 0) this.api.createLocation(body).subscribe(done);
		else this.api.patchLocation(id, body).subscribe(done);
	}

	remove(loc: Location): void {
		this.api.deleteLocation(loc.id).subscribe(() => this.reload());
	}
}
