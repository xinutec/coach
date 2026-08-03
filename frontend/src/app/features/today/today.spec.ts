import { TestBed } from "@angular/core/testing";
import { MatBottomSheet } from "@angular/material/bottom-sheet";
import { of } from "rxjs";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { CoachApi } from "../../coach-api";
import type { Ask, Exercise, Explanation, PacingNow, Suggestion } from "../../models";
import { Today } from "./today";

/** The card is where the engine's verdict becomes something a person reads
 *  standing in a gym, and nearly all of it is presentation over the wire types —
 *  pure functions of a `Suggestion`, an `Explanation`, a `PacingNow`. Several of
 *  these lines are field-test findings that reached the athlete before they
 *  reached a test (R2-2's "14 / 13", the untrained day that read "3 / 10 done"),
 *  which is exactly the class of bug a component spec catches and neither the
 *  Rust suite nor the layout harness can: the engine was right both times, and
 *  the page did the arithmetic itself. */

const bodyweight = (repLow: number, repHigh: number): Ask => ({
	kind: "bodyweight",
	repLow,
	repHigh,
});

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

function suggestion(over: Partial<Suggestion> = {}): Suggestion {
	return {
		exerciseId: 1,
		exerciseName: "Exercise 1",
		pattern: "push",
		kind: "work",
		sets: 3,
		logged: [],
		ask: bodyweight(8, 12),
		group: "Chest",
		substitutedFor: null,
		explanation: null,
		...over,
	};
}

function explanation(over: Partial<Explanation> = {}): Explanation {
	return {
		deficit: 0.5,
		recovery: 1,
		pays: 1,
		confirming: false,
		confidence: "high",
		e1rm: null,
		estimateFrom: null,
		misses: 0,
		readiness: null,
		...over,
	};
}

function pacing(over: Partial<PacingNow> = {}): PacingNow {
	return {
		state: "active",
		deload: false,
		readiness: null,
		nudge: false,
		reason: "",
		window: "within",
		spacingOk: true,
		minutesSinceLastSet: null,
		dayTargetSets: 10,
		dayDoneSets: 0,
		groups: [],
		suggestion: null,
		plan: [],
		notices: [],
		...over,
	};
}

/** The card with a catalog behind it and nothing else moving.
 *
 *  The stores are the real ones over a fake `CoachApi`: they are thin caches
 *  over a GET, and a synchronous `of()` fills them inside the constructor's
 *  `loadAll()`, so the component sees its catalog without a tick. The pacing
 *  verdict is set directly rather than fetched — every test below is about what
 *  the card *does* with a verdict, and routing one through the effect would only
 *  add a way for the test to be about the effect instead. */
function card(exercises: Exercise[] = []): Today {
	TestBed.configureTestingModule({
		providers: [
			{
				provide: CoachApi,
				useValue: {
					exercises: () => of(exercises),
					locations: () => of([]),
					locationCurrent: () => of({ locationId: null }),
					pacingNow: () => of(pacing()),
					deleteSet: () => of(undefined),
					exerciseImageUrl: (id: number) => `/api/exercises/${id}/image`,
				},
			},
			{ provide: MatBottomSheet, useValue: { open: vi.fn() } },
		],
	});
	return TestBed.runInInjectionContext(() => new Today());
}

afterEach(() => {
	// The Angular vitest runner does not reset the TestBed between tests; without
	// this the second `configureTestingModule` in the file throws "test module has
	// already been instantiated".
	TestBed.resetTestingModule();
	localStorage.clear();
});

describe("the session counter", () => {
	// R2-2. The header used to read the engine's own day-size estimate while the
	// cards held their own set counts, so finishing every card reported "14 / 13".
	// It sums the cards now — and then the *other* half of the same bug showed up:
	// counting warm-ups meant three checked-off arm circles read as "3 / 10 done"
	// on a day the engine scored as untrained.
	it("counts the work, not the warm-ups", () => {
		const t = card();
		const p = pacing({
			plan: [
				suggestion({ kind: "warmup", sets: 1, logged: [{ reps: 10, loadKg: null, holdS: null }] }),
				suggestion({ exerciseId: 2, sets: 3, logged: [{ reps: 8, loadKg: null, holdS: null }] }),
				suggestion({ exerciseId: 3, sets: 4, logged: [] }),
			],
		});
		expect(t.planSets(p)).toBe(7);
		expect(t.planDone(p)).toBe(1);
	});

	it("reads zero done on a day whose only finished item is a warm-up", () => {
		const t = card();
		const p = pacing({
			plan: [
				suggestion({ kind: "warmup", sets: 1, logged: [{ reps: 10, loadKg: null, holdS: null }] }),
				suggestion({ exerciseId: 2, sets: 3 }),
			],
		});
		expect(t.planDone(p)).toBe(0);
	});
});

describe("what comes next", () => {
	it("points at the first item with sets still to do", () => {
		const t = card();
		t.pacing.set(
			pacing({
				plan: [
					suggestion({ sets: 1, logged: [{ reps: 10, loadKg: null, holdS: null }] }),
					suggestion({ exerciseId: 2, exerciseName: "Second", sets: 3 }),
				],
			}),
		);
		expect(t.nextUp()?.exerciseName).toBe("Second");
		expect(t.isNextUp(1)).toBe(true);
		expect(t.isNextUp(0)).toBe(false);
	});

	// A ramp-in warm-up carries the same exercise id as the work item after it, so
	// "is this the one to do now?" has to be asked by position or both light up.
	it("marks the ramp-in and not the work set that shares its movement", () => {
		const t = card();
		t.pacing.set(
			pacing({
				plan: [
					suggestion({ kind: "warmup", sets: 1 }),
					suggestion({ kind: "work", sets: 3 }),
				],
			}),
		);
		expect(t.isNextUp(0)).toBe(true);
		expect(t.isNextUp(1)).toBe(false);
	});

	it("points at nothing outside the training window", () => {
		const t = card();
		t.pacing.set(pacing({ window: "after", plan: [suggestion()] }));
		expect(t.isNextUp(0)).toBe(false);
		// The item is still *there* — the plan reads as a preview, it isn't gone.
		expect(t.nextUp()).not.toBeNull();
	});
});

describe("a plan item's shape on the page", () => {
	// A card earns its height by holding something still to decide. Rendered full,
	// three finished warm-ups filled 498px of a ~780px viewport and pushed the
	// first work card — the reason the page exists — below the fold.
	it("collapses a finished item and a warm-up, and keeps work standing", () => {
		const t = card();
		const done = [{ reps: 8, loadKg: null, holdS: null }];
		expect(t.isCompact(suggestion({ sets: 1, logged: done }))).toBe(true);
		expect(t.isCompact(suggestion({ kind: "warmup" }))).toBe(true);
		expect(t.isCompact(suggestion({ sets: 3, logged: done }))).toBe(false);
	});
});

describe("the dose on a compact row", () => {
	it("aims at a range for work and states one number for a warm-up", () => {
		const t = card();
		expect(t.compactDose(suggestion({ ask: bodyweight(8, 12) }))).toBe("aim 8");
		expect(t.compactDose(suggestion({ kind: "warmup", ask: bodyweight(10, 12) }))).toBe("10 reps");
		// A range that isn't one is stated, not aimed at.
		expect(t.compactDose(suggestion({ ask: bodyweight(10, 10) }))).toBe("10 reps");
	});

	// A loaded warm-up is a ramp-in on the movement itself rather than a mobility
	// drill, and that changes what you do with it.
	it("names a ramp-in so it isn't read as arm circles", () => {
		const t = card();
		const ask: Ask = { kind: "weighted", loadKg: 40, repLow: 5, repHigh: 5 };
		expect(t.compactDose(suggestion({ kind: "warmup", ask }))).toBe("Ramp-in · 5 reps · 40 kg");
	});

	it("says the metres of a carry", () => {
		const t = card();
		const ask: Ask = { kind: "weightedDistance", loadKg: 24, distanceM: 20 };
		expect(t.compactDose(suggestion({ ask }))).toBe("24 kg · 20 m");
	});

	it("falls back to naming the drill when there is no dose to state", () => {
		const t = card();
		expect(t.compactDose(suggestion({ kind: "warmup", ask: { kind: "amrap" } }))).toBe("Mobility");
	});

	it("reports the sets once the item is finished", () => {
		const t = card();
		const s = suggestion({ sets: 1, logged: [{ reps: 8, loadKg: null, holdS: null }] });
		expect(t.compactDose(s)).toBe("1 set · aim 8");
	});

	// One set of a single-arm movement is both arms, and "10 reps" means ten with
	// each. Half a session or a double one, depending on how you read it.
	it("says 'each side' for a single-arm movement", () => {
		const t = card([exercise(1, { unilateral: true })]);
		expect(t.compactDose(suggestion())).toBe("aim 8 · each side");
	});
});

describe("what you already did", () => {
	// On set two the question is what set one was, and that used to live only in
	// History. Reps alone read better with the unit said once at the end.
	it("says the unit once for reps and per-set for anything carrying a load", () => {
		const t = card();
		expect(
			t.loggedSummary(
				suggestion({
					logged: [
						{ reps: 9, loadKg: null, holdS: null },
						{ reps: 6, loadKg: null, holdS: null },
					],
				}),
			),
		).toBe("9 · 6 reps");
		expect(
			t.loggedSummary(
				suggestion({
					logged: [
						{ reps: 7, loadKg: 22.5, holdS: null },
						{ reps: 6, loadKg: 24, holdS: null },
					],
				}),
			),
		).toBe("22.5 kg × 7 · 24 kg × 6");
	});

	it("says nothing at all before the first set", () => {
		const t = card();
		expect(t.loggedSummary(suggestion())).toBe("");
	});

	it("carries the per-side convention into the receipt", () => {
		const t = card([exercise(1, { unilateral: true })]);
		expect(t.loggedSummary(suggestion({ logged: [{ reps: 9, loadKg: null, holdS: null }] }))).toBe(
			"9 reps each side",
		);
	});
});

describe("why this?", () => {
	it("leads with the baseline when the movement is here to confirm one", () => {
		const t = card();
		const lines = t.explanationLines(explanation({ confirming: true, deficit: 0.01 }));
		expect(lines[0]).toContain("Locking in your baseline");
		// A near-zero deficit line would read as "why is this even here?".
		expect(lines.join(" ")).not.toContain("target still to go");
	});

	it("speaks plainly instead of saying a group is 100% below target", () => {
		const t = card();
		expect(t.explanationLines(explanation({ deficit: 1 }))).toContain(
			"Untrained this week — the whole target is still to come",
		);
		expect(t.explanationLines(explanation({ deficit: 0.4 }))).toContain(
			"40% of this week's target still to go",
		);
	});

	it("names every confidence the engine can report", () => {
		const t = card();
		for (const confidence of ["high", "medium", "low", "none"] as const) {
			const lines = t.explanationLines(explanation({ confidence }));
			expect(lines[0]).toBeTruthy();
			expect(lines[0]).not.toContain("undefined");
		}
	});

	// "Lighter than last week" with no reason reads as the coach forgetting rather
	// than listening.
	it("says why the load eased off after a run of short sessions", () => {
		const t = card();
		expect(t.explanationLines(explanation({ misses: 1 })).join(" ")).toContain(
			"holding here rather than adding",
		);
		expect(t.explanationLines(explanation({ misses: 3 })).join(" ")).toContain(
			"3 sessions under target",
		);
		expect(t.explanationLines(explanation({ misses: 0 })).join(" ")).not.toContain("under target");
	});

	it("states a readiness reading without ever urging intensity", () => {
		const t = card();
		for (const readiness of ["high", "normal", "low"] as const) {
			const line = t.explanationLines(explanation({ readiness })).join(" ");
			expect(line).not.toMatch(/push|go hard|smash/i);
		}
		expect(t.explanationLines(explanation({ readiness: "low" }))).toContain(
			"Low readiness — easing the volume off",
		);
	});

	it("shows the estimated max only when there is one", () => {
		const t = card();
		expect(t.explanationLines(explanation({ e1rm: 82.4 })).join(" ")).toContain(
			"Estimated 1-rep max ≈ 82 kg",
		);
		expect(t.explanationLines(explanation()).join(" ")).not.toContain("1-rep max");
	});
});

describe("a calibration instruction", () => {
	const asks: Ask[] = [
		{ kind: "amrap" },
		{ kind: "maxHold" },
		{ kind: "loadedCarry", startKg: 24 },
		{ kind: "loadedDistance", startKg: 24 },
		{ kind: "buildUp", startKg: 40, reps: 5 },
	];

	it("asks for the load and the reps, never for a rating out of ten", () => {
		const t = card();
		for (const ask of asks) {
			const line = t.assessInstruction(suggestion({ kind: "assess", ask }));
			expect(line).not.toMatch(/rpe|how did (that|it) feel|out of ten|rate/i);
			expect(line).toMatch(/log/i);
		}
	});

	it("names the rep target the build-up asked for rather than inventing one", () => {
		const t = card();
		const ask: Ask = { kind: "buildUp", startKg: 40, reps: 3 };
		expect(t.assessInstruction(suggestion({ kind: "assess", ask }))).toContain("clean set of 3");
	});

	it("distinguishes the carry measured in seconds from the one measured in metres", () => {
		const t = card();
		const secs = t.assessInstruction(
			suggestion({ kind: "assess", ask: { kind: "loadedCarry", startKg: 24 } }),
		);
		const metres = t.assessInstruction(
			suggestion({ kind: "assess", ask: { kind: "loadedDistance", startKg: 24 } }),
		);
		expect(secs).toContain("seconds");
		expect(metres).toContain("distance");
	});

	it("says the numbers are per side on a single-arm movement", () => {
		const t = card([exercise(1, { unilateral: true })]);
		expect(t.assessInstruction(suggestion({ kind: "assess", ask: { kind: "amrap" } }))).toContain(
			"per side",
		);
	});
});

describe("a swap the athlete can act on", () => {
	it("distinguishes kit that isn't here from kit whose weights aren't registered", () => {
		const t = card();
		expect(
			t.substitutionNote({
				ideal: "Cable row",
				blocker: { kind: "absent", kit: ["cable machine"] },
			}),
		).toBe("Swapped in for Cable row — no cable machine here");
		expect(
			t.substitutionNote({
				ideal: "Cable row",
				blocker: { kind: "unweighted", kit: ["cable machine"] },
			}),
		).toBe("Swapped in for Cable row — no weights registered for cable machine");
	});
});

describe("the set behind an estimate", () => {
	// Ability is a max, so one mistyped set becomes a ceiling nothing later can
	// lower — and the offending set is usually weeks old. The card has to name it
	// in the terms it was logged in, or correcting it is an archaeology problem.
	it("names it in the terms it was logged in", () => {
		const t = card();
		const when = new Date("2026-07-14T16:00:00Z").toLocaleDateString(undefined, {
			day: "numeric",
			month: "short",
		});
		expect(
			t.describeSource({
				setId: 7,
				loggedAt: "2026-07-14T16:00:00",
				loadKg: 100,
				reps: 8,
				holdS: null,
			}),
		).toBe(`${when} · 100 kg × 8`);
		expect(
			t.describeSource({
				setId: 8,
				loggedAt: "2026-07-14T16:00:00",
				loadKg: null,
				reps: null,
				holdS: 45,
			}),
		).toBe(`${when} · 45s`);
	});
});

describe("the reasoning toggle", () => {
	it("opens and closes one movement at a time", () => {
		const t = card();
		expect(t.isWhyOpen(1)).toBe(false);
		t.toggleWhy(1);
		expect(t.isWhyOpen(1)).toBe(true);
		expect(t.isWhyOpen(2)).toBe(false);
		t.toggleWhy(1);
		expect(t.isWhyOpen(1)).toBe(false);
	});
});

describe("the location the kit comes from", () => {
	it("says so plainly when there is none, rather than showing a blank", () => {
		const t = card();
		expect(t.locationName()).toBe("No location");
	});

	// A location picked by hand holds for the rest of the day: a reload must not
	// silently revert to the detected one and change the plan's loads under the
	// athlete mid-session.
	it("remembers a hand-picked location for the rest of the day", () => {
		const t = card();
		t.onLocationChange(4);
		const raw = localStorage.getItem("coach.pickedLocation");
		expect(raw).not.toBeNull();
		expect(JSON.parse(raw ?? "{}")).toEqual({ id: 4, day: new Date().toDateString() });
		expect(t.autoDetected()).toBe(false);
	});
});

describe("a pick left over from another day", () => {
	beforeEach(() => {
		localStorage.setItem(
			"coach.pickedLocation",
			JSON.stringify({ id: 9, day: new Date(Date.now() - 86_400_000).toDateString() }),
		);
	});

	it("is not applied", () => {
		const t = card();
		TestBed.tick(); // the constructor's effect waits on the locations store
		expect(t.selectedLocationId()).not.toBe(9);
	});
});

describe("a stored pick written by some other version of the app", () => {
	// The blob is checked, not asserted: it is written by whichever version of
	// this app the device last ran, and an `id` that isn't a number would be
	// handed on as a location id and silently select nothing.
	it("reads as no pick at all rather than selecting nothing", () => {
		localStorage.setItem("coach.pickedLocation", JSON.stringify({ id: "home", day: "whenever" }));
		const t = card();
		TestBed.tick();
		expect(t.selectedLocationId()).toBeNull();
	});

	it("survives a blob that isn't JSON", () => {
		localStorage.setItem("coach.pickedLocation", "{not json");
		const t = card();
		TestBed.tick();
		expect(t.selectedLocationId()).toBeNull();
	});
});
