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
	/** How many of each weight (same order). 0 = plenty, which is what a gym is. */
	weightQty: number[];
	labels: string[];
	/** A loadable bar or adjustable dumbbell handle: its own weight, how many you
	 *  own (a both-arms press needs two), and how many discs a sleeve takes. */
	barKg: number | null;
	barQty: number | null;
	plateSlots: number | null;
}

/** A plate you own: its size, how many, and which kit it fits (a dumbbell
 *  handle's small discs won't go on an Olympic bar). `equipment: null` is the
 *  shared pool every loadable bar here draws from. */
interface PlateForm {
	equipment: string | null;
	loadKg: number;
	qty: number | null;
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
	// Per-equipment specifics being edited: slug → owned weights / band variants /
	// bar weight.
	readonly formOptions = signal<Map<string, EquipmentSpecifics>>(new Map());
	// Every plate here, each pinned to the kit it fits (or the shared pool).
	readonly formPlates = signal<PlateForm[]>([]);
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
		this.formPlates.set([]);
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
						weightQty: [...(o.weightQty ?? [])],
						labels: [...o.labels],
						barKg: o.barKg,
						barQty: o.barQty,
						plateSlots: o.plateSlots,
					},
				]),
			),
		);
		this.formPlates.set(loc.plates.map((p) => ({ ...p })));
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
	// Common bar weights per bar, as one-tap quick-picks. A straight barbell is
	// 15/20 kg (women's/men's Olympic); a trap bar's heavier frame runs 20–30 kg.
	// The "other" field covers anything off this list.
	private readonly BAR_PRESETS: Record<string, number[]> = {
		barbell: [15, 20],
		trap_bar: [20, 25, 30],
		dumbbell: [1.5, 2, 2.5],
	};
	barPresets(slug: string): number[] {
		return this.BAR_PRESETS[slug] ?? [20];
	}

	/** Selected kit that carries a load — each gets a discrete-weights editor. A
	 *  dumbbell can be *both*: an adjustable handle you load, and a plain 5 kg one
	 *  you don't. The two sets union, so kit is no longer either/or.
	 *
	 *  This is the catalog's `weighted` flag, not the free-weight category: a cable
	 *  stack's pin positions are exactly a list of discrete weights, and gating on
	 *  the category meant there was nowhere to enter them. */
	readonly weightedSlugs = computed(() =>
		[...this.formEquip()].filter(
			(s) => this.equipment().find((e) => e.slug === s)?.weighted ?? false,
		),
	);
	/** Selected loadable kit (barbell/trap bar/adjustable dumbbell) — each gets a
	 *  bar-or-handle editor with the plates that fit *it*. */
	readonly loadableSlugs = computed(() =>
		[...this.formEquip()].filter((s) => this.isLoadable(s)),
	);
	/** Selected bands — each gets a named-variant editor. */
	readonly bandSlugs = computed(() =>
		[...this.formEquip()].filter((s) => this.categoryOf(s) === "band"),
	);

	/** A stack's weights are its pin positions, and you can't own "two of" one — so
	 *  the dumbbell-pair language would be nonsense on a pulley. */
	isStack(slug: string): boolean {
		return this.categoryOf(slug) === "machine";
	}
	weightsLabel(slug: string): string {
		const kind = this.isStack(slug) ? "stack weights" : "fixed weights";
		return `${this.equipLabel(slug)} — ${kind} (kg)`;
	}

	weightsOf(slug: string): number[] {
		return this.formOptions().get(slug)?.weights ?? [];
	}
	/** How many of a given weight you own — 0 (or missing) reads as "plenty". */
	weightQtyOf(slug: string, w: number): number {
		const o = this.formOptions().get(slug);
		if (!o) return 0;
		return o.weightQty[o.weights.indexOf(w)] ?? 0;
	}
	barQtyOf(slug: string): number | null {
		return this.formOptions().get(slug)?.barQty ?? null;
	}
	plateSlotsOf(slug: string): number | null {
		return this.formOptions().get(slug)?.plateSlots ?? null;
	}
	/** The plates you own for one piece of kit: those pinned to it, plus the shared
	 *  pool (an Olympic disc goes on the barbell and the trap bar alike). */
	platesOf(slug: string): PlateForm[] {
		return this.formPlates().filter(
			(p) => p.equipment === slug || p.equipment === null,
		);
	}
	labelsOf(slug: string): string[] {
		return this.formOptions().get(slug)?.labels ?? [];
	}
	barKgOf(slug: string): number | null {
		return this.formOptions().get(slug)?.barKg ?? null;
	}

	private mutate(slug: string, fn: (s: EquipmentSpecifics) => void): void {
		const m = new Map(this.formOptions());
		const cur = m.get(slug) ?? {
			weights: [],
			weightQty: [],
			labels: [],
			barKg: null,
			barQty: null,
			plateSlots: null,
		};
		fn(cur);
		m.set(slug, cur);
		this.formOptions.set(m);
	}

	addWeight(slug: string, raw: string, qtyRaw?: string): void {
		const n = Number.parseFloat(raw);
		if (!Number.isFinite(n) || n <= 0) return;
		const q = Number.parseInt(qtyRaw ?? "", 10);
		const qty = Number.isFinite(q) && q > 0 ? q : 0; // 0 = plenty
		this.mutate(slug, (s) => {
			if (s.weights.includes(n)) return;
			const pairs = s.weights
				.map((w, i) => ({ w, q: s.weightQty[i] ?? 0 }))
				.concat({ w: n, q: qty })
				.sort((a, b) => a.w - b.w);
			s.weights = pairs.map((p) => p.w);
			s.weightQty = pairs.map((p) => p.q);
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
			const byW = new Map(s.weights.map((w, i) => [w, s.weightQty[i] ?? 0]));
			for (let w = from; w <= to + 1e-6; w += step) {
				const k = Math.round(w * 100) / 100;
				if (!byW.has(k)) byW.set(k, 0); // a filled rack is "plenty" of each
			}
			const pairs = [...byW.entries()].sort((a, b) => a[0] - b[0]);
			s.weights = pairs.map(([w]) => w);
			s.weightQty = pairs.map(([, q]) => q);
		});
	}
	removeWeight(slug: string, n: number): void {
		this.mutate(slug, (s) => {
			const keep = s.weights
				.map((w, i) => ({ w, q: s.weightQty[i] ?? 0 }))
				.filter((p) => p.w !== n);
			s.weights = keep.map((p) => p.w);
			s.weightQty = keep.map((p) => p.q);
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
	/** Set how many of a bar/handle you own (a both-arms dumbbell press needs two). */
	setBarQty(slug: string, raw: string | number | null): void {
		const n = typeof raw === "number" ? raw : Number.parseInt(raw ?? "", 10);
		this.mutate(slug, (s) => {
			s.barQty = Number.isFinite(n) && n > 0 ? n : null;
		});
	}
	/** Set how many discs fit on one sleeve before it's full. */
	setPlateSlots(slug: string, raw: string | number | null): void {
		const n = typeof raw === "number" ? raw : Number.parseInt(raw ?? "", 10);
		this.mutate(slug, (s) => {
			s.plateSlots = Number.isFinite(n) && n > 0 ? n : null;
		});
	}

	/** Add a plate that fits `slug`. Counts matter: plates load in pairs, so a disc
	 *  you own one of can't be used at all, and a pair of dumbbells shares them. */
	addPlate(slug: string, raw: string, qtyRaw?: string): void {
		const n = Number.parseFloat(raw);
		if (!Number.isFinite(n) || n <= 0) return;
		if (this.platesOf(slug).some((p) => p.loadKg === n)) return;
		const q = Number.parseInt(qtyRaw ?? "", 10);
		const qty = Number.isFinite(q) && q > 0 ? q : null; // null = plenty
		this.formPlates.set(
			[...this.formPlates(), { equipment: slug, loadKg: n, qty }].sort(
				(a, b) => a.loadKg - b.loadKg,
			),
		);
	}
	removePlate(slug: string, p: PlateForm): void {
		this.formPlates.set(
			this.formPlates().filter(
				(x) => !(x.loadKg === p.loadKg && x.equipment === p.equipment),
			),
		);
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
				weightQty: o.weights.map((_, i) => o.weightQty[i] ?? 0),
				labels: o.labels,
				barKg: o.barKg,
				barQty: o.barQty,
				plateSlots: o.plateSlots,
			}))
			.filter(
				(o) => o.weights.length > 0 || o.labels.length > 0 || o.barKg !== null,
			);
		// Plates only matter with a bar, and only for kit that's still selected.
		const plates = this.formPlates().filter(
			(p) => p.equipment === null || this.formEquip().has(p.equipment),
		);
		const body = {
			name: this.formName().trim() || "Location",
			isDefault: this.formDefault(),
			equipment: [...this.formEquip()],
			equipmentOptions,
			plates,
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
