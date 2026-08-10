import { TestBed } from "@angular/core/testing";
import { of } from "rxjs";
import { afterEach, describe, expect, it } from "vitest";

import { CoachApi } from "../../coach-api";
import type { GroupBalance, PacingNow, Region } from "../../models";
import { BalancePage } from "./balance";

/** Rolling volume against target, per muscle group. This is the coach showing
 *  its working — the same numbers the engine picked today's session from — so
 *  the ordering is the message: what is most behind comes first. */

function group(name: string, region: Region, over: Partial<GroupBalance> = {}): GroupBalance {
	return {
		group: name,
		region,
		current: 0,
		target: 10,
		deficit: 1,
		recovering: false,
		...over,
	};
}

/** The whole verdict, because the page reads it from the shared PacingStore and
 *  a narrowed stand-in would only prove the narrowing. Everything outside
 *  `groups` is Today's business; it is here at its resting value. */
function verdict(groups: GroupBalance[]): PacingNow {
	return {
		state: "rest",
		deload: false,
		readiness: null,
		nudge: false,
		reason: "",
		window: "within",
		spacingOk: true,
		minutesSinceLastSet: null,
		dayTargetSets: 0,
		dayDoneSets: 0,
		groups,
		suggestion: null,
		plan: [],
		notices: [],
	};
}

function balance(groups: GroupBalance[], loaded = true): BalancePage {
	TestBed.configureTestingModule({
		providers: [
			{
				provide: CoachApi,
				useValue: {
					pacingNow: () => (loaded ? of(verdict(groups)) : of()),
				},
			},
		],
	});
	return TestBed.runInInjectionContext(() => new BalancePage());
}

afterEach(() => {
	TestBed.resetTestingModule();
});

describe("how the regions are laid out", () => {
	/** Head to toe, the order a person thinks about their own body in — not the
	 *  order the query happened to return, which is why the component holds the
	 *  list rather than trusting the payload. */
	it("runs top-down regardless of the order the groups arrive in", () => {
		const page = balance([
			group("Quads", "legs"),
			group("Chest", "chest"),
			group("Abdominals", "core"),
			group("Lats", "back"),
		]);
		expect(page.byRegion().map((r) => r.region)).toEqual(["chest", "back", "core", "legs"]);
	});

	it("leaves out a region nothing was logged for, rather than showing it empty", () => {
		const page = balance([group("Chest", "chest")]);
		expect(page.byRegion().map((r) => r.region)).toEqual(["chest"]);
	});

	it("has nothing to draw before the verdict arrives", () => {
		const page = balance([], false);
		expect(page.groups()).toEqual([]);
		expect(page.byRegion()).toEqual([]);
		expect(page.loading()).toBe(true);
	});

	it("groups every muscle group under its own region", () => {
		const page = balance([
			group("Chest", "chest"),
			group("Serratus", "chest"),
			group("Quads", "legs"),
		]);
		const [chest, legs] = page.byRegion();
		expect(chest?.groups.map((g) => g.group)).toEqual(["Chest", "Serratus"]);
		expect(legs?.groups.map((g) => g.group)).toEqual(["Quads"]);
	});
});

describe("the order within a region", () => {
	/** Most-in-deficit first: the top of each region is what the coach would
	 *  reach for next, so a reader can stop after the first row. */
	it("puts the group furthest behind at the top", () => {
		const page = balance([
			group("Chest", "chest", { deficit: 0.1 }),
			group("Serratus", "chest", { deficit: 0.9 }),
			group("Pecs", "chest", { deficit: 0.5 }),
		]);
		expect(page.byRegion()[0]?.groups.map((g) => g.group)).toEqual([
			"Serratus",
			"Pecs",
			"Chest",
		]);
	});

	it("sorts each region independently of the others", () => {
		const page = balance([
			group("Chest", "chest", { deficit: 0.2 }),
			group("Quads", "legs", { deficit: 0.9 }),
			group("Serratus", "chest", { deficit: 0.8 }),
			group("Calves", "legs", { deficit: 0.1 }),
		]);
		const [chest, legs] = page.byRegion();
		expect(chest?.groups.map((g) => g.group)).toEqual(["Serratus", "Chest"]);
		expect(legs?.groups.map((g) => g.group)).toEqual(["Quads", "Calves"]);
	});
});

describe("the numbers on a row", () => {
	it("fills the bar in proportion to the target", () => {
		const page = balance([]);
		expect(page.pct(5, 10)).toBe(50);
		expect(page.pct(0, 10)).toBe(0);
		expect(page.pct(10, 10)).toBe(100);
	});

	/** The bar stops at full; the figures beside it do not. Over-target is a
	 *  real state — an emphasis week, or a session that ran long — and the row
	 *  still has to say by how much, which is what `current / target` is for. */
	it("caps the bar at full without hiding that the target was passed", () => {
		const page = balance([]);
		expect(page.pct(15, 10)).toBe(100);
		expect(page.round1(15)).toBe("15");
		expect(page.round0(10)).toBe("10");
	});

	/** A group with no target would divide by zero and render `NaN%`, which is a
	 *  broken-looking bar on a screen whose whole job is legibility. */
	it("draws an empty bar rather than NaN when there is no target", () => {
		const page = balance([]);
		expect(page.pct(0, 0)).toBe(0);
		expect(page.pct(3, 0)).toBe(0);
	});

	it("shows a part-set to one decimal, because half a set is a real number here", () => {
		const page = balance([]);
		expect(page.round1(2.5)).toBe("2.5");
		expect(page.round1(2.04)).toBe("2");
		expect(page.round1(0)).toBe("0");
	});

	it("shows the target whole, because it is a number of sets", () => {
		const page = balance([]);
		expect(page.round0(9.6)).toBe("10");
		expect(page.round0(9.4)).toBe("9");
	});

	it("capitalises a region for its heading", () => {
		const page = balance([]);
		expect(page.regionLabel("shoulders")).toBe("Shoulders");
		expect(page.regionLabel("core")).toBe("Core");
	});
});

describe("still recovering", () => {
	/** Dimmed rather than hidden. A group mid-recovery is why the coach is not
	 *  asking for it, and hiding the row would make that look like an omission. */
	it("carries the flag through to the row that draws it", () => {
		const page = balance([
			group("Chest", "chest", { deficit: 0.2, recovering: true }),
			group("Serratus", "chest", { deficit: 0.9, recovering: false }),
		]);
		const rows = page.byRegion()[0]?.groups ?? [];
		expect(rows.map((g) => [g.group, g.recovering])).toEqual([
			["Serratus", false],
			["Chest", true],
		]);
	});
});
