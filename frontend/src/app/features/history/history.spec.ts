import { TestBed } from "@angular/core/testing";
import { of } from "rxjs";
import { afterEach, describe, expect, it, vi } from "vitest";

import { CoachApi } from "../../coach-api";
import type { Exercise, WorkoutSet } from "../../models";
import { AllExercisesStore, SetsStore } from "../../stores/catalog";
import { HistoryPage } from "./history";

/** The log read back. `group.spec.ts` covers the summary line; this covers the
 *  day the sets land in, which is the part that has a timezone in it — `loggedAt`
 *  is stored UTC and every screen renders it local, so a set logged at 00:30 BST
 *  belongs to the day the athlete trained, not the day UTC thinks it was.
 *
 *  Fixtures are **newest first**, because `/api/sets` is `ORDER BY logged_at
 *  DESC` (src/workout/repo.rs:52) and the day grouping is insertion-ordered off
 *  that. Handing it ascending sets would test an arrangement the component is
 *  never given, and "open the newest day" would be asserting the wrong end. */

let nextId = 1;
function set(at: string, over: Partial<WorkoutSet> = {}): WorkoutSet {
	return {
		id: nextId++,
		exerciseId: 1,
		loggedAt: at,
		reps: 8,
		loadKg: null,
		holdS: null,
		distanceM: null,
		rpe: null,
		note: null,
		...over,
	};
}

function exercise(id: number, over: Partial<Exercise> = {}): Exercise {
	return {
		id,
		slug: `ex-${id}`,
		name: `Exercise ${id}`,
		variation: null,
		pattern: "push",
		metric: "reps",
		unilateral: false,
		skill: false,
		warmup: false,
		power: false,
		implements: 1,
		difficulty: null,
		isActive: true,
		equipment: [],
		hasImage: false,
		...over,
	};
}

interface Harness {
	page: HistoryPage;
	deleted: number[];
	sets: WorkoutSet[];
}

function history(sets: WorkoutSet[] = [], exercises: Exercise[] = [exercise(1)]): Harness {
	const deleted: number[] = [];
	let held = sets;
	TestBed.configureTestingModule({
		providers: [
			{
				provide: CoachApi,
				useValue: {
					deleteSet: (id: number) => {
						deleted.push(id);
						return of(undefined);
					},
				},
			},
			{
				provide: SetsStore,
				useValue: {
					value: () => held,
					loaded: () => true,
					refresh: vi.fn(),
					patch: (f: (list: WorkoutSet[]) => WorkoutSet[]) => {
						held = f(held);
					},
				},
			},
			{
				provide: AllExercisesStore,
				useValue: { value: () => exercises, loaded: () => true, refresh: vi.fn() },
			},
		],
	});
	const page = TestBed.runInInjectionContext(() => new HistoryPage());
	return { page, deleted, get sets() { return held; } };
}

afterEach(() => {
	TestBed.resetTestingModule();
});

// --- which day a set belongs to --------------------------------------------

describe("grouping by day", () => {
	it("collects a day's sets under one group", () => {
		const { page } = history([
			set("2026-07-15T09:00:00"),
			set("2026-07-14T16:05:00"),
			set("2026-07-14T16:00:00"),
		]);
		expect(page.groups().map((g) => g.setCount)).toEqual([1, 2]);
	});

	/** `loggedAt` has no zone on it; the component appends `Z`. Without that a
	 *  set is read as local time that was already local, and an evening session
	 *  in BST lands an hour out — which is a different day either side of
	 *  midnight. */
	it("puts a late-evening set on the day it was trained", () => {
		// 23:30 UTC on the 14th is 00:30 on the 15th in London (BST).
		const { page } = history([set("2026-07-14T23:30:00")]);
		const [only] = page.groups();
		expect(only?.setCount).toBe(1);
		expect(only?.label).toContain("15");
	});

	it("lists the newest day first, as the API hands them over", () => {
		const { page } = history([set("2026-07-15T09:00:00"), set("2026-07-14T16:00:00")]);
		expect(page.groups().map((g) => g.key)).toEqual(["2026-6-15", "2026-6-14"]);
	});

	it("has nothing to show before anything is logged", () => {
		const { page } = history([]);
		expect(page.groups()).toEqual([]);
	});
});

// --- what a set reads as ---------------------------------------------------

describe("a set's detail line", () => {
	it("says reps and load", () => {
		const { page } = history();
		expect(page.detail(set("2026-07-14T16:00:00", { reps: 6, loadKg: 40 }))).toBe(
			"6 reps · 40 kg",
		);
	});

	it("says a hold in seconds", () => {
		const { page } = history();
		expect(page.detail(set("2026-07-14T16:00:00", { reps: null, holdS: 30 }))).toBe("30s");
	});

	/** The same gap `summarise` had: a carry is measured in metres, and dropping
	 *  them rendered every farmer's walk as its weight alone. */
	it("says how far a carry went", () => {
		const { page } = history();
		expect(
			page.detail(set("2026-07-14T16:00:00", { reps: null, loadKg: 24, distanceM: 10 })),
		).toBe("24 kg · 10 m");
	});

	/** Never solicited, but imported history carries one — so it is shown where
	 *  it exists and absent everywhere else, rather than rendered as a blank. */
	it("shows an imported RPE and omits an absent one", () => {
		const { page } = history();
		expect(page.detail(set("2026-07-14T16:00:00", { reps: 5, rpe: 8 }))).toBe("5 reps · RPE 8");
		expect(page.detail(set("2026-07-14T16:00:00", { reps: 5 }))).toBe("5 reps");
	});

	it("says nothing at all about a set carrying no numbers", () => {
		const { page } = history();
		expect(page.detail(set("2026-07-14T16:00:00", { reps: null }))).toBe("");
	});
});

describe("naming a movement", () => {
	it("uses the full name, variation included", () => {
		const { page } = history([], [exercise(1, { name: "Pull-up", variation: "L-sit" })]);
		expect(page.name(1)).toBe("Pull-up (L-sit)");
	});

	/** A set can outlive its catalog row. A blank where a name should be reads as
	 *  a broken screen; a generic word reads as a movement no longer listed. */
	it("falls back rather than showing a blank", () => {
		const { page } = history();
		expect(page.name(999)).toBe("Exercise");
	});
});

// --- opening and closing ---------------------------------------------------

describe("what starts open", () => {
	it("opens the newest day and leaves the rest collapsed", () => {
		const { page } = history([set("2026-07-15T09:00:00"), set("2026-07-14T16:00:00")]);
		TestBed.tick();
		expect(page.isOpen("2026-6-15")).toBe(true);
		expect(page.isOpen("2026-6-14")).toBe(false);
	});

	/** Terse by default: the grouped line answers "what did I do today", and the
	 *  individual sets are a tap away for when un-logging a mistake needs them. */
	it("keeps the individual sets closed until asked", () => {
		const { page } = history([set("2026-07-15T09:00:00")]);
		TestBed.tick();
		expect(page.areSetsOpen("2026-6-15:1")).toBe(false);
	});

	it("does not re-open a day the athlete closed", () => {
		const { page } = history([set("2026-07-15T09:00:00")]);
		TestBed.tick();
		page.toggle("2026-6-15");
		TestBed.tick();
		expect(page.isOpen("2026-6-15")).toBe(false);
	});

	for (const [name, open, toggle] of [
		["days", "isOpen", "toggle"],
		["set lists", "areSetsOpen", "toggleSets"],
	] as const) {
		it(`toggles ${name} independently of each other key`, () => {
			const { page } = history([set("2026-07-15T09:00:00")]);
			page[toggle]("a");
			page[toggle]("b");
			page[toggle]("a");
			expect(page[open]("a")).toBe(false);
			expect(page[open]("b")).toBe(true);
		});
	}
});

describe("un-logging a set", () => {
	it("asks the server, then drops it from the list without a reload", () => {
		const sets = [set("2026-07-15T09:05:00"), set("2026-07-15T09:00:00")];
		const h = history(sets);
		const [first] = sets;
		if (!first) throw new Error("fixture");
		h.page.del(first);
		expect(h.deleted).toEqual([first.id]);
		expect(h.sets.map((s) => s.id)).not.toContain(first.id);
	});
});
