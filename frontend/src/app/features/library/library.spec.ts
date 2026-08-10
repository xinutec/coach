import { TestBed } from "@angular/core/testing";
import { MatBottomSheet } from "@angular/material/bottom-sheet";
import { of } from "rxjs";
import { afterEach, describe, expect, it } from "vitest";

import { CoachApi } from "../../coach-api";
import type { Equipment, Exercise } from "../../models";
import { LibraryPage } from "./library";

/** Browsing the catalog. The search box and the four pattern buttons are the
 *  only way to reach a movement that isn't in today's plan — a filter that drops
 *  something doesn't error, it just means the athlete concludes the coach
 *  doesn't know the exercise. */

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

function equipment(slug: string, name: string): Equipment {
	return { id: 1, slug, name, category: "free_weight", loadable: false, weighted: true };
}

const CATALOG: Exercise[] = [
	exercise(1, { name: "Pull-up", variation: "bar", pattern: "pull" }),
	exercise(2, { name: "Pull-up", variation: "L-sit", pattern: "pull" }),
	exercise(3, { name: "Goblet squat", pattern: "legs", equipment: ["kettlebell"] }),
	exercise(4, { name: "Push-up", pattern: "push" }),
	exercise(5, { name: "Hollow hold", pattern: "core", metric: "hold" }),
];

const KIT: Equipment[] = [equipment("kettlebell", "Kettlebell"), equipment("bench", "Bench")];

interface Opened {
	data: { exerciseId: number };
}

function library(
	catalog: Exercise[] = CATALOG,
	kit: Equipment[] = KIT,
): { page: LibraryPage; opened: Opened[] } {
	const opened: Opened[] = [];
	TestBed.configureTestingModule({
		providers: [
			{
				provide: CoachApi,
				useValue: {
					exercises: () => of(catalog),
					equipment: () => of(kit),
					exerciseImageUrl: (id: number) => `/api/exercises/${id}/image`,
				},
			},
			{
				provide: MatBottomSheet,
				useValue: {
					open: (_component: unknown, config: Opened) => {
						opened.push(config);
					},
				},
			},
		],
	});
	return { page: TestBed.runInInjectionContext(() => new LibraryPage()), opened };
}

/** Names, not ids — an assertion that fails should say which movement. */
function names(page: LibraryPage): string[] {
	return page.filtered().map((e) => page.displayName(e));
}

afterEach(() => {
	TestBed.resetTestingModule();
});

describe("what the list shows before anything is typed", () => {
	it("shows the whole catalog", () => {
		const { page } = library();
		expect(page.filtered().length).toBe(CATALOG.length);
		expect(page.loading()).toBe(false);
	});

	it("has nothing rather than throwing when the catalog is empty", () => {
		const { page } = library([]);
		expect(page.filtered()).toEqual([]);
	});
});

describe("searching", () => {
	it("matches part of a name, anywhere in it", () => {
		const { page } = library();
		page.search.set("squat");
		expect(names(page)).toEqual(["Goblet squat"]);
	});

	it("ignores case, so nobody has to reach for the shift key", () => {
		const { page } = library();
		page.search.set("GOBLET");
		expect(names(page)).toEqual(["Goblet squat"]);
	});

	it("ignores surrounding space, which a phone keyboard adds on its own", () => {
		const { page } = library();
		page.search.set("  squat  ");
		expect(names(page)).toEqual(["Goblet squat"]);
	});

	/** Variations are distinct movements, so the variation has to be searchable
	 *  — otherwise "L-sit" finds nothing and the athlete concludes it isn't in
	 *  the catalog, when it is, under a shared base name. */
	it("matches the variation as well as the base name", () => {
		const { page } = library();
		page.search.set("l-sit");
		expect(names(page)).toEqual(["Pull-up (L-sit)"]);
	});

	it("keeps both variations when the base name is what was typed", () => {
		const { page } = library();
		page.search.set("pull-up");
		expect(names(page)).toEqual(["Pull-up (bar)", "Pull-up (L-sit)"]);
	});

	/** The name on the card was once the one string that found nothing: the
	 *  haystack was `name + " " + variation` while the label is
	 *  `name (variation)`, so reading a movement off the screen and typing it
	 *  back returned an empty library. Both spellings match now. */
	it("matches the name exactly as the card spells it, brackets and all", () => {
		const { page } = library();
		page.search.set("Pull-up (L-sit)");
		expect(names(page)).toEqual(["Pull-up (L-sit)"]);
	});

	it("still matches the same movement typed straight through", () => {
		const { page } = library();
		page.search.set("pull-up l-sit");
		expect(names(page)).toEqual(["Pull-up (L-sit)"]);
	});

	it("matches a partial bracketed name, which is what typing looks like", () => {
		const { page } = library();
		page.search.set("pull-up (l");
		expect(names(page)).toEqual(["Pull-up (L-sit)"]);
	});

	it("finds nothing rather than everything when nothing matches", () => {
		const { page } = library();
		page.search.set("deadlift");
		expect(names(page)).toEqual([]);
	});

	it("shows everything again once the box is cleared", () => {
		const { page } = library();
		page.search.set("squat");
		page.search.set("");
		expect(page.filtered().length).toBe(CATALOG.length);
	});
});

describe("the pattern buttons", () => {
	it("narrows to one pattern", () => {
		const { page } = library();
		page.togglePattern("pull");
		expect(names(page)).toEqual(["Pull-up (bar)", "Pull-up (L-sit)"]);
	});

	it("clears the filter when the button already on is tapped again", () => {
		const { page } = library();
		page.togglePattern("pull");
		page.togglePattern("pull");
		expect(page.pattern()).toBeNull();
		expect(page.filtered().length).toBe(CATALOG.length);
	});

	/** Tapping a second pattern replaces the first rather than adding to it —
	 *  one pattern at a time is what the template's `[class.on]` draws. */
	it("switches to the new pattern rather than combining them", () => {
		const { page } = library();
		page.togglePattern("pull");
		page.togglePattern("legs");
		expect(page.pattern()).toBe("legs");
		expect(names(page)).toEqual(["Goblet squat"]);
	});

	it("combines with the search box rather than overriding it", () => {
		const { page } = library();
		page.search.set("pull-up");
		page.togglePattern("push");
		expect(names(page)).toEqual([]);

		page.togglePattern("push");
		expect(names(page)).toEqual(["Pull-up (bar)", "Pull-up (L-sit)"]);
	});

	it("offers exactly the four the catalog is built around", () => {
		const { page } = library();
		expect([...page.patterns]).toEqual(["push", "pull", "legs", "core"]);
	});
});

describe("what a card says", () => {
	it("names a variation as its own movement", () => {
		const { page } = library();
		expect(page.displayName(exercise(9, { name: "Pull-up", variation: "bar" }))).toBe(
			"Pull-up (bar)",
		);
		expect(page.displayName(exercise(9, { name: "Push-up" }))).toBe("Push-up");
	});

	it("labels kit by its name, not its slug", () => {
		const { page } = library();
		expect(page.equipLabel("kettlebell")).toBe("Kettlebell");
	});

	/** The two catalogs load independently, so a card can render before the kit
	 *  list has arrived. Showing the slug keeps the chip readable instead of
	 *  blank — a blank chip reads as broken, `kettlebell` reads as unpolished. */
	it("falls back to the slug for kit it cannot name yet", () => {
		const { page } = library(CATALOG, []);
		expect(page.equipLabel("kettlebell")).toBe("kettlebell");
	});

	it("capitalises a pattern for display without touching the value", () => {
		const { page } = library();
		expect(page.patternLabel("push")).toBe("Push");
		expect(page.patternLabel("legs")).toBe("Legs");
	});

	it("points the thumbnail at the exercise's own image", () => {
		const { page } = library();
		expect(page.imageUrl(3)).toBe("/api/exercises/3/image");
	});
});

describe("opening a movement", () => {
	it("asks the sheet for that exercise, by id", () => {
		const { page, opened } = library();
		const target = page.filtered()[2];
		if (!target) throw new Error("fixture");
		page.open(target);
		expect(opened.length).toBe(1);
		expect(opened[0]?.data).toEqual({ exerciseId: target.id });
	});
});
