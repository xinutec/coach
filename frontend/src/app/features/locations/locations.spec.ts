import { TestBed } from "@angular/core/testing";
import { of } from "rxjs";
import { afterEach, describe, expect, it, vi } from "vitest";

import { CoachApi } from "../../coach-api";
import type { Equipment, Location, NewLocation, Plate } from "../../models";
import { LocationsPage } from "./locations";

/** The page where the athlete tells the coach what is in the room.
 *
 *  Everything the engine prescribes is bounded by what it reads here: a weight
 *  that didn't make it into the list is a weight that will never be asked for,
 *  and a weight that got in wrong is a load the athlete is told to lift. None of
 *  it errors — it just quietly changes the session. That is what makes this the
 *  page most worth testing after the card itself.
 *
 *  The invariant that runs through all of it: `weights[i]` and `weightQty[i]` are
 *  one fact stored as two arrays, and they must stay in step through every
 *  add/remove/sort or the athlete owns two of the wrong dumbbell. */

function equipment(slug: string, over: Partial<Equipment> = {}): Equipment {
	return {
		id: 1,
		slug,
		name: slug,
		category: "free_weight",
		loadable: false,
		weighted: false,
		...over,
	};
}

const KIT: Equipment[] = [
	equipment("dumbbell", { id: 1, name: "Dumbbell", weighted: true, loadable: true }),
	equipment("kettlebell", { id: 2, name: "Kettlebell", weighted: true }),
	equipment("barbell", { id: 3, name: "Barbell", weighted: true, loadable: true }),
	equipment("cable_machine", {
		id: 4,
		name: "Cable machine",
		category: "machine",
		weighted: true,
	}),
	equipment("resistance_band", { id: 5, name: "Band", category: "band" }),
	equipment("pull_up_bar", { id: 6, name: "Pull-up bar", category: "rig" }),
	equipment("bench", { id: 7, name: "Bench", category: "bench" }),
];

function location(over: Partial<Location> = {}): Location {
	return {
		id: 1,
		name: "Home",
		isDefault: true,
		equipment: [],
		equipmentOptions: [],
		plates: [],
		healthPlaceId: null,
		...over,
	};
}

const api = {
	created: [] as NewLocation[],
	patched: [] as { id: number; body: NewLocation }[],
	deleted: [] as number[],
};

function page(locations: Location[] = []): LocationsPage {
	api.created = [];
	api.patched = [];
	api.deleted = [];
	TestBed.configureTestingModule({
		providers: [
			{
				provide: CoachApi,
				useValue: {
					locations: () => of(locations),
					equipment: () => of(KIT),
					placesDetected: () => of([]),
					createLocation: (body: NewLocation) => {
						api.created.push(body);
						return of(location());
					},
					patchLocation: (id: number, body: NewLocation) => {
						api.patched.push({ id, body });
						return of(location());
					},
					deleteLocation: (id: number) => {
						api.deleted.push(id);
						return of(undefined);
					},
				},
			},
		],
	});
	return TestBed.runInInjectionContext(() => new LocationsPage());
}

afterEach(() => {
	TestBed.resetTestingModule();
	vi.restoreAllMocks();
});

/** Open the editor with `slug` selected — the state every weight test starts from. */
function editing(p: LocationsPage, slug = "dumbbell"): LocationsPage {
	p.startNew();
	p.toggleEquip(slug);
	return p;
}

describe("adding a weight", () => {
	it("keeps the rack in order however it was entered", () => {
		const p = editing(page());
		p.addWeight("dumbbell", "20");
		p.addWeight("dumbbell", "6");
		p.addWeight("dumbbell", "12.5");
		expect(p.weightsOf("dumbbell")).toEqual([6, 12.5, 20]);
	});

	// The count is stored in a second array addressed by the same index. Inserting
	// out of order re-sorts the weights, and the counts have to move with them —
	// otherwise "two of the 6 kg" silently becomes "two of the 20 kg", and the
	// engine builds a load out of a dumbbell that doesn't exist in that quantity.
	it("moves each count with its weight when the sort reshuffles them", () => {
		const p = editing(page());
		p.addWeight("dumbbell", "20", "2");
		p.addWeight("dumbbell", "6", "4");
		p.addWeight("dumbbell", "12.5", "1");
		expect(p.weightsOf("dumbbell")).toEqual([6, 12.5, 20]);
		expect(p.weightQtyOf("dumbbell", 6)).toBe(4);
		expect(p.weightQtyOf("dumbbell", 12.5)).toBe(1);
		expect(p.weightQtyOf("dumbbell", 20)).toBe(2);
	});

	// 0 means "plenty", which is what a gym rack is.
	it("reads a missing or nonsense count as plenty", () => {
		const p = editing(page());
		p.addWeight("dumbbell", "10");
		p.addWeight("dumbbell", "12", "");
		p.addWeight("dumbbell", "14", "not a number");
		p.addWeight("dumbbell", "16", "0");
		p.addWeight("dumbbell", "18", "-3");
		for (const w of [10, 12, 14, 16, 18]) expect(p.weightQtyOf("dumbbell", w)).toBe(0);
	});

	it("refuses anything that isn't a weight", () => {
		const p = editing(page());
		p.addWeight("dumbbell", "");
		p.addWeight("dumbbell", "heavy");
		p.addWeight("dumbbell", "0");
		p.addWeight("dumbbell", "-5");
		expect(p.weightsOf("dumbbell")).toEqual([]);
	});

	it("does not add the same weight twice", () => {
		const p = editing(page());
		p.addWeight("dumbbell", "10", "2");
		p.addWeight("dumbbell", "10", "5");
		expect(p.weightsOf("dumbbell")).toEqual([10]);
		expect(p.weightQtyOf("dumbbell", 10)).toBe(2);
	});
});

describe("adding a whole rack at once", () => {
	it("fills the range inclusive of both ends", () => {
		const p = editing(page());
		p.addRange("dumbbell", "2.5", "10", "2.5");
		expect(p.weightsOf("dumbbell")).toEqual([2.5, 5, 7.5, 10]);
	});

	// Repeated += on a fractional step drifts: 0.1 added thirty times is not 3.
	// Left alone that lands 7.499999999999999 in the list, which matches no real
	// dumbbell and never dedupes against the 7.5 someone types by hand.
	it("lands on the numbers written on the dumbbells, not on floating-point dust", () => {
		const p = editing(page());
		p.addRange("dumbbell", "2.5", "50", "2.5");
		const weights = p.weightsOf("dumbbell");
		expect(weights).toHaveLength(20);
		expect(weights).toContain(7.5);
		expect(weights).toContain(47.5);
		for (const w of weights) expect(w).toBe(Math.round(w * 100) / 100);
	});

	it("treats a filled rack as plenty of each", () => {
		const p = editing(page());
		p.addRange("dumbbell", "5", "15", "5");
		for (const w of [5, 10, 15]) expect(p.weightQtyOf("dumbbell", w)).toBe(0);
	});

	// A range is a convenience over addWeight, not a replacement — a weight already
	// entered with a count must keep it.
	it("leaves a count already entered by hand alone", () => {
		const p = editing(page());
		p.addWeight("dumbbell", "10", "2");
		p.addRange("dumbbell", "5", "15", "5");
		expect(p.weightsOf("dumbbell")).toEqual([5, 10, 15]);
		expect(p.weightQtyOf("dumbbell", 10)).toBe(2);
	});

	it("refuses a range that isn't one", () => {
		const p = editing(page());
		p.addRange("dumbbell", "10", "5", "1"); // backwards
		p.addRange("dumbbell", "0", "10", "1"); // from zero
		p.addRange("dumbbell", "5", "10", "0"); // no step
		p.addRange("dumbbell", "a", "b", "c"); // not numbers
		expect(p.weightsOf("dumbbell")).toEqual([]);
	});

	// A fat-fingered step (1 instead of 1000) would otherwise spin out a list
	// nobody wants and the engine would search every one of them.
	it("refuses a runaway range rather than generating it", () => {
		const p = editing(page());
		p.addRange("dumbbell", "1", "5000", "1");
		expect(p.weightsOf("dumbbell")).toEqual([]);
	});
});

describe("removing a weight", () => {
	it("takes its count with it and leaves the rest aligned", () => {
		const p = editing(page());
		p.addWeight("dumbbell", "6", "4");
		p.addWeight("dumbbell", "10", "2");
		p.addWeight("dumbbell", "20", "1");
		p.removeWeight("dumbbell", 10);
		expect(p.weightsOf("dumbbell")).toEqual([6, 20]);
		expect(p.weightQtyOf("dumbbell", 6)).toBe(4);
		expect(p.weightQtyOf("dumbbell", 20)).toBe(1);
	});
});

describe("plates", () => {
	// Plates load in pairs, so a disc you own one of can't be used at all — the
	// count is not decoration.
	it("keeps them sorted and records how many you own", () => {
		const p = editing(page(), "barbell");
		p.addPlate("barbell", "20", "4");
		p.addPlate("barbell", "5", "2");
		expect(p.platesOf("barbell").map((x) => x.loadKg)).toEqual([5, 20]);
		expect(p.platesOf("barbell")[0]?.qty).toBe(2);
	});

	it("reads a missing count as plenty", () => {
		const p = editing(page(), "barbell");
		p.addPlate("barbell", "10");
		expect(p.platesOf("barbell")[0]?.qty).toBeNull();
	});

	it("refuses a plate that isn't a weight", () => {
		const p = editing(page(), "barbell");
		p.addPlate("barbell", "0");
		p.addPlate("barbell", "-2");
		p.addPlate("barbell", "");
		expect(p.platesOf("barbell")).toEqual([]);
	});

	// An Olympic disc goes on the barbell and the trap bar alike, so the shared
	// pool (equipment: null) shows under every bar.
	it("shows the shared pool alongside the ones pinned to this bar", () => {
		const p = editing(page(), "barbell");
		p.formPlates.set([
			{ equipment: null, loadKg: 20, qty: 4 },
			{ equipment: "barbell", loadKg: 2.5, qty: 2 },
			{ equipment: "dumbbell", loadKg: 1.25, qty: 4 },
		] satisfies Plate[]);
		expect(p.platesOf("barbell").map((x) => x.loadKg).sort((a, b) => a - b)).toEqual([
			2.5, 20,
		]);
	});

	// Double-counting the same disc would let the engine build a load twice over.
	it("will not pin a plate to a bar when the shared pool already has that size", () => {
		const p = editing(page(), "barbell");
		p.formPlates.set([{ equipment: null, loadKg: 20, qty: 4 }]);
		p.addPlate("barbell", "20", "2");
		expect(p.formPlates()).toHaveLength(1);
	});

	// Removing one must not take its twin from the other pile.
	it("removes the one you pointed at, not every plate of that size", () => {
		const p = editing(page(), "barbell");
		const shared: Plate = { equipment: null, loadKg: 20, qty: 4 };
		const pinned: Plate = { equipment: "barbell", loadKg: 20, qty: 2 };
		p.formPlates.set([shared, pinned]);
		p.removePlate(pinned);
		expect(p.formPlates()).toEqual([shared]);
	});
});

describe("un-selecting a piece of kit", () => {
	// The weights described kit that is no longer in the room. Left behind they
	// would be saved back and the engine would prescribe against them.
	it("forgets the weights that described it", () => {
		const p = editing(page());
		p.addWeight("dumbbell", "10", "2");
		p.toggleEquip("dumbbell");
		expect(p.weightsOf("dumbbell")).toEqual([]);
	});

	it("puts it back empty rather than remembering the old numbers", () => {
		const p = editing(page());
		p.addWeight("dumbbell", "10");
		p.toggleEquip("dumbbell");
		p.toggleEquip("dumbbell");
		expect(p.weightsOf("dumbbell")).toEqual([]);
	});
});

describe("which kit gets which editor", () => {
	it("offers discrete weights to anything weighted, machines included", () => {
		const p = page();
		p.startNew();
		for (const s of ["dumbbell", "cable_machine", "pull_up_bar", "resistance_band"]) {
			p.toggleEquip(s);
		}
		// The cable stack's pin positions are exactly a list of discrete weights;
		// gating this on the free-weight category left nowhere to enter them, and
		// the coach dropped every cable movement.
		expect(p.weightedSlugs().sort()).toEqual(["cable_machine", "dumbbell"]);
		expect(p.loadableSlugs()).toEqual(["dumbbell"]);
		expect(p.bandSlugs()).toEqual(["resistance_band"]);
	});

	// You cannot own "two of" a pin position, so the dumbbell-pair language would
	// be nonsense on a pulley.
	it("calls a stack's numbers stack weights and a rack's fixed weights", () => {
		const p = page();
		expect(p.isStack("cable_machine")).toBe(true);
		expect(p.isStack("dumbbell")).toBe(false);
		expect(p.weightsLabel("cable_machine")).toBe("Cable machine — stack weights (kg)");
		expect(p.weightsLabel("dumbbell")).toBe("Dumbbell — fixed weights (kg)");
	});

	it("suggests the bar weights that bar actually comes in", () => {
		const p = page();
		expect(p.barPresets("barbell")).toEqual([15, 20]);
		expect(p.barPresets("trap_bar")).toEqual([20, 25, 30]);
		expect(p.barPresets("anything_else")).toEqual([20]);
	});
});

describe("a bar's own numbers", () => {
	it("records the bar weight, how many you own, and how many discs fit", () => {
		const p = editing(page(), "barbell");
		p.setBar("barbell", "20");
		p.setBarQty("barbell", "2");
		p.setPlateSlots("barbell", "5");
		expect(p.barKgOf("barbell")).toBe(20);
		expect(p.barQtyOf("barbell")).toBe(2);
		expect(p.plateSlotsOf("barbell")).toBe(5);
	});

	// Clearing the field means "I don't know", which is not the same as zero — a
	// zero-weight bar would make every load computed from it wrong by 20 kg.
	it("clears back to unknown rather than to zero", () => {
		const p = editing(page(), "barbell");
		p.setBar("barbell", "20");
		p.setBar("barbell", "");
		expect(p.barKgOf("barbell")).toBeNull();
		p.setBar("barbell", "0");
		expect(p.barKgOf("barbell")).toBeNull();
		p.setBar("barbell", null);
		expect(p.barKgOf("barbell")).toBeNull();
	});
});

describe("band variants", () => {
	it("keeps each name once and drops blank ones", () => {
		const p = editing(page(), "resistance_band");
		p.addLabel("resistance_band", "red");
		p.addLabel("resistance_band", "  black  ");
		p.addLabel("resistance_band", "red");
		p.addLabel("resistance_band", "   ");
		expect(p.labelsOf("resistance_band")).toEqual(["red", "black"]);
		p.removeLabel("resistance_band", "red");
		expect(p.labelsOf("resistance_band")).toEqual(["black"]);
	});
});

describe("opening an existing location for editing", () => {
	const saved = location({
		id: 7,
		name: "Office",
		isDefault: false,
		equipment: ["dumbbell"],
		equipmentOptions: [
			{
				slug: "dumbbell",
				weights: [10, 20],
				weightQty: [2, 2],
				labels: [],
				barKg: null,
				barQty: null,
				plateSlots: null,
			},
		],
		plates: [{ equipment: null, loadKg: 20, qty: 4 }],
	});

	it("fills the form from what was saved", () => {
		const p = page([saved]);
		p.startEdit(saved);
		expect(p.editingId()).toBe(7);
		expect(p.formName()).toBe("Office");
		expect(p.weightsOf("dumbbell")).toEqual([10, 20]);
		expect(p.platesOf("dumbbell")).toHaveLength(1);
	});

	// The store's objects are shared with every other view of them. Editing the
	// form must not reach back into the cached location, or abandoning an edit
	// would still have changed what the app thinks is in the room.
	it("copies the numbers instead of pointing at the store's own", () => {
		const p = page([saved]);
		p.startEdit(saved);
		p.addWeight("dumbbell", "30");
		p.removePlate({ equipment: null, loadKg: 20, qty: 4 });
		expect(saved.equipmentOptions[0]?.weights).toEqual([10, 20]);
		expect(saved.plates).toHaveLength(1);
	});

	it("leaves the editor without writing anything when cancelled", () => {
		const p = page([saved]);
		p.startEdit(saved);
		p.addWeight("dumbbell", "30");
		p.cancel();
		expect(p.editingId()).toBeNull();
		expect(api.patched).toEqual([]);
		expect(saved.equipmentOptions[0]?.weights).toEqual([10, 20]);
	});
});

describe("the first location", () => {
	it("is the default one, because there is nothing else to be", () => {
		const p = page();
		p.startNew();
		expect(p.formDefault()).toBe(true);
	});

	it("is not, once there is one already", () => {
		const p = page([location()]);
		p.startNew();
		expect(p.formDefault()).toBe(false);
	});
});

describe("saving", () => {
	it("creates when new and patches when editing", () => {
		const p = page();
		p.startNew();
		p.formName.set("Garage");
		p.save();
		expect(api.created).toHaveLength(1);
		expect(api.patched).toEqual([]);

		const existing = location({ id: 4 });
		p.startEdit(existing);
		p.save();
		expect(api.patched[0]?.id).toBe(4);
	});

	it("names an unnamed place rather than saving a blank", () => {
		const p = page();
		p.startNew();
		p.formName.set("   ");
		p.save();
		expect(api.created[0]?.name).toBe("Location");
	});

	// Specifics for kit that was de-selected after being filled in would otherwise
	// describe a room that no longer exists.
	it("drops the numbers for kit that is no longer selected", () => {
		const p = editing(page());
		p.addWeight("dumbbell", "10");
		p.toggleEquip("kettlebell");
		p.addWeight("kettlebell", "16");
		p.toggleEquip("kettlebell"); // changed my mind
		p.save();
		const body = api.created[0];
		expect(body?.equipmentOptions.map((o) => o.slug)).toEqual(["dumbbell"]);
	});

	it("does not send an entry for kit nobody described", () => {
		const p = editing(page());
		p.toggleEquip("bench"); // selected, but has no numbers to give
		p.addWeight("dumbbell", "10");
		p.save();
		const body = api.created[0];
		expect(body?.equipment.sort()).toEqual(["bench", "dumbbell"]);
		expect(body?.equipmentOptions.map((o) => o.slug)).toEqual(["dumbbell"]);
	});

	// The two arrays leave as one fact per index, whatever order they were built in.
	it("sends a count for every weight, in step", () => {
		const p = editing(page());
		p.addWeight("dumbbell", "20", "2");
		p.addWeight("dumbbell", "6");
		p.addWeight("dumbbell", "12.5", "1");
		p.save();
		const body = api.created[0];
		const o = body?.equipmentOptions[0];
		expect(o?.weights).toEqual([6, 12.5, 20]);
		expect(o?.weightQty).toEqual([0, 1, 2]);
		expect(o?.weightQty).toHaveLength(o?.weights.length ?? -1);
	});

	it("keeps the shared pool and drops plates pinned to kit that left", () => {
		const p = editing(page(), "barbell");
		p.formPlates.set([
			{ equipment: null, loadKg: 20, qty: 4 },
			{ equipment: "barbell", loadKg: 2.5, qty: 2 },
			{ equipment: "kettlebell", loadKg: 1.25, qty: 2 },
		]);
		p.save();
		const body = api.created[0];
		expect(body?.plates.map((x) => x.equipment)).toEqual([null, "barbell"]);
	});

	it("does nothing at all when no editor is open", () => {
		const p = page();
		p.save();
		expect(api.created).toEqual([]);
		expect(api.patched).toEqual([]);
	});
});

describe("the kit picker", () => {
	it("groups by category in a stable order and skips empty ones", () => {
		const p = page();
		expect(p.grouped().map((g) => g.category)).toEqual([
			"free_weight",
			"rig",
			"bench",
			"machine",
			"band",
		]);
		expect(p.grouped()[0]?.label).toBe("Free weights");
		expect(p.grouped().map((g) => g.category)).not.toContain("ball");
	});

	it("names a piece of kit, falling back to its slug", () => {
		const p = page();
		expect(p.equipLabel("dumbbell")).toBe("Dumbbell");
		expect(p.equipLabel("unknown_thing")).toBe("unknown_thing");
	});
});

describe("removing a location", () => {
	it("asks the server to delete exactly that one", () => {
		const loc = location({ id: 9 });
		const p = page([loc]);
		p.remove(loc);
		expect(api.deleted).toEqual([9]);
	});
});
