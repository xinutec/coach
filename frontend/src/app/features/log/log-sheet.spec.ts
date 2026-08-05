import { TestBed } from "@angular/core/testing";
import {
	MAT_BOTTOM_SHEET_DATA,
	MatBottomSheetRef,
} from "@angular/material/bottom-sheet";
import { of, throwError } from "rxjs";
import { afterEach, describe, expect, it, vi } from "vitest";

import { CoachApi } from "../../coach-api";
import type { Exercise, Metric, NewSet, WorkoutSet } from "../../models";
import { LogSheet, type LogSheetData } from "./log-sheet";

/** Every number the engine reasons from enters through this sheet. Ability is a
 *  max over decayed sets, so a wrong value here is not a wrong screen — it is a
 *  PR the model cannot unlearn, and it goes on shaping prescriptions for weeks.
 *
 *  Two of the cases below are field-test findings that reached the athlete
 *  before they reached a test: R2-1 logged "10 reps · 4 kg" against a bodyweight
 *  drill because a stale value sat behind a hidden field, and a run of sets was
 *  lost when the sheet dismissed itself under the next tap. */

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

/** What the server echoes back. Nothing here reads it — the sheet only counts
 *  that a set landed — but the API's type is the contract, and a stub that
 *  doesn't satisfy it would let a real shape change through unnoticed. */
const saved: WorkoutSet = {
	id: 1,
	exerciseId: 1,
	loggedAt: "2026-08-05T09:00:00Z",
	reps: null,
	loadKg: null,
	holdS: null,
	distanceM: null,
	rpe: null,
	note: null,
};

interface Harness {
	sheet: LogSheet;
	sent: NewSet[];
	dismissed: number[];
	logSet: ReturnType<typeof vi.fn>;
}

/** The nth set sent, or a failure naming which one was missing — `sent[n]!`
 *  would assert a fact the test is there to establish. */
function nth(sent: NewSet[], n: number): NewSet {
	const body = sent[n];
	if (body === undefined) {
		throw new Error(`expected at least ${n + 1} set(s), got ${sent.length}`);
	}
	return body;
}

/** The sheet with a recording API. `logSet` defaults to succeeding; a test that
 *  cares about failure replaces it before calling `save()`. */
function open(data: Partial<LogSheetData> = {}): Harness {
	const sent: NewSet[] = [];
	const dismissed: number[] = [];
	const logSet = vi.fn((body: NewSet) => {
		sent.push(body);
		return of(saved);
	});
	TestBed.configureTestingModule({
		providers: [
			{ provide: CoachApi, useValue: { logSet } },
			{
				provide: MatBottomSheetRef,
				useValue: {
					dismiss: (n: number) => {
						dismissed.push(n);
					},
				},
			},
			{
				provide: MAT_BOTTOM_SHEET_DATA,
				useValue: { exercises: [exercise(1)], ...data } satisfies LogSheetData,
			},
		],
	});
	const sheet = TestBed.runInInjectionContext(() => new LogSheet());
	return { sheet, sent, dismissed, logSet };
}

afterEach(() => {
	TestBed.resetTestingModule();
});

// --- the picker's order ----------------------------------------------------

describe("the exercise list", () => {
	it("puts today's planned movements first, in plan order", () => {
		const { sheet } = open({
			exercises: [exercise(1, { name: "Alpha" }), exercise(2, { name: "Beta" }), exercise(3, { name: "Gamma" })],
			planPrefills: [{ exerciseId: 3 }, { exerciseId: 1 }],
		});
		expect(sheet.exercises.map((e) => e.id)).toEqual([3, 1, 2]);
	});

	it("sorts the rest by what the athlete reads, not by catalog order", () => {
		const { sheet } = open({
			exercises: [
				exercise(1, { name: "Zercher squat" }),
				exercise(2, { name: "Ab wheel" }),
				exercise(3, { name: "Pull-up", variation: "L-sit" }),
			],
		});
		expect(sheet.exercises.map((e) => e.name)).toEqual([
			"Ab wheel",
			"Pull-up",
			"Zercher squat",
		]);
	});

	/** A plan can name a movement the catalog no longer lists. Dropping it is
	 *  right; letting an `undefined` into the picker is not. */
	it("ignores a planned id that isn't in the catalog", () => {
		const { sheet } = open({
			exercises: [exercise(1)],
			planPrefills: [{ exerciseId: 99 }, { exerciseId: 1 }],
		});
		expect(sheet.exercises.map((e) => e.id)).toEqual([1]);
	});

	it("never lists one movement twice", () => {
		const { sheet } = open({
			exercises: [exercise(1), exercise(2)],
			planPrefills: [{ exerciseId: 1 }],
		});
		expect(sheet.exercises.map((e) => e.id)).toEqual([1, 2]);
	});
});

// --- opening on a prescription ---------------------------------------------

describe("opening", () => {
	it("starts on the prefilled movement with its numbers", () => {
		const { sheet } = open({
			exercises: [exercise(1), exercise(2, { metric: "weighted_reps" })],
			prefill: { exerciseId: 2, reps: 5, loadKg: 40 },
		});
		expect(sheet.exerciseId()).toBe(2);
		expect(sheet.reps()).toBe(5);
		expect(sheet.loadKg()).toBe(40);
	});

	it("falls back to the first movement when nothing is prefilled", () => {
		const { sheet } = open({ exercises: [exercise(7), exercise(8)] });
		expect(sheet.exerciseId()).toBe(7);
		expect(sheet.reps()).toBeNull();
	});

	it("has no selection at all when the catalog is empty", () => {
		const { sheet } = open({ exercises: [] });
		expect(sheet.exerciseId()).toBeNull();
		expect(sheet.selected()).toBeNull();
	});
});

// --- switching movements (field-test R2-1) ---------------------------------

describe("switching movement", () => {
	it("lands on the plan's numbers for a planned movement", () => {
		const { sheet } = open({
			exercises: [exercise(1), exercise(2, { metric: "weighted_reps" })],
			planPrefills: [{ exerciseId: 2, reps: 8, loadKg: 32.5 }],
		});
		sheet.onExercise(2);
		expect(sheet.reps()).toBe(8);
		expect(sheet.loadKg()).toBe(32.5);
	});

	/** The R2-1 shape exactly: a load typed for a barbell lift must not ride
	 *  along into a bodyweight drill, where no field shows it. */
	it("leaves nothing behind when the new movement has no prescription", () => {
		const { sheet } = open({
			exercises: [exercise(1, { metric: "weighted_reps" }), exercise(2)],
			prefill: { exerciseId: 1, reps: 10, loadKg: 4 },
		});
		sheet.onExercise(2);
		expect(sheet.reps()).toBeNull();
		expect(sheet.loadKg()).toBeNull();
		expect(sheet.holdS()).toBeNull();
		expect(sheet.distanceM()).toBeNull();
	});

	it("comes back to the opening prescription when you switch back", () => {
		const { sheet } = open({
			exercises: [exercise(1, { metric: "weighted_reps" }), exercise(2)],
			prefill: { exerciseId: 1, reps: 10, loadKg: 4 },
		});
		sheet.onExercise(2);
		sheet.onExercise(1);
		expect(sheet.reps()).toBe(10);
		expect(sheet.loadKg()).toBe(4);
	});

	it("clears a stale complaint about the movement you just left", () => {
		const { sheet } = open({ exercises: [exercise(1), exercise(2)] });
		sheet.error.set("That didn't save — try again");
		sheet.onExercise(2);
		expect(sheet.error()).toBeNull();
	});
});

// --- what actually goes on the wire ----------------------------------------

describe("the set it sends", () => {
	/** The server rejects fields the metric does not own, and a value the form
	 *  is not showing must never ride along. Each metric is checked separately
	 *  because each one is a different set of owned fields. */
	const cases: { metric: Metric; owns: (keyof NewSet)[] }[] = [
		{ metric: "reps", owns: ["reps"] },
		{ metric: "weighted_reps", owns: ["reps", "loadKg"] },
		{ metric: "hold", owns: ["holdS"] },
		{ metric: "weighted_hold", owns: ["holdS", "loadKg"] },
		{ metric: "weighted_distance", owns: ["distanceM", "loadKg"] },
	];

	for (const { metric, owns } of cases) {
		it(`sends only ${owns.join(" + ")} for ${metric}`, () => {
			const { sheet, sent } = open({ exercises: [exercise(1, { metric })] });
			// Every field filled in, so anything sent is sent on purpose.
			sheet.reps.set(9);
			sheet.loadKg.set(20);
			sheet.holdS.set(30);
			sheet.distanceM.set(40);
			sheet.save();
			const body = nth(sent, 0);
			for (const field of ["reps", "loadKg", "holdS", "distanceM"] as const) {
				expect(body[field], `${field} on a ${metric} set`).toBe(
					owns.includes(field) ? { reps: 9, loadKg: 20, holdS: 30, distanceM: 40 }[field] : null,
				);
			}
		});
	}

	/** The athlete is never asked to rate his own exertion — the loop is report
	 *  what happened, not how it felt (docs/trainer.md). The wire field stays
	 *  because imported history carries one, but the app must not solicit it. */
	it("never sends an RPE", () => {
		const { sheet, sent } = open({ exercises: [exercise(1)] });
		sheet.save();
		expect(nth(sent, 0).rpe).toBeNull();
	});

	it("trims a note, and sends nothing rather than an empty one", () => {
		const { sheet, sent } = open({ exercises: [exercise(1)] });
		sheet.note.set("  felt easy  ");
		sheet.save();
		sheet.note.set("   ");
		sheet.save();
		expect(nth(sent, 0).note).toBe("felt easy");
		expect(nth(sent, 1).note).toBeNull();
	});

	it("asks the server to decide the time", () => {
		const { sheet, sent } = open({ exercises: [exercise(1)] });
		sheet.save();
		expect(nth(sent, 0).loggedAt).toBeNull();
	});

	it("does not send anything when no movement is selected", () => {
		const { sheet, logSet } = open({ exercises: [] });
		sheet.save();
		expect(logSet).not.toHaveBeenCalled();
	});

	it("sends the confirmation only when it is given", () => {
		const { sheet, sent } = open({ exercises: [exercise(1)] });
		sheet.save();
		sheet.save(true);
		expect(nth(sent, 0).confirmLoad).toBe(false);
		expect(nth(sent, 1).confirmLoad).toBe(true);
	});
});

// --- after a set lands -----------------------------------------------------

describe("after a set lands", () => {
	it("counts it, clears the note and keeps the numbers for the next set", () => {
		const onLogged = vi.fn();
		const { sheet } = open({
			exercises: [exercise(1, { metric: "weighted_reps" })],
			prefill: { exerciseId: 1, reps: 8, loadKg: 40 },
			onLogged,
		});
		sheet.note.set("first");
		sheet.save();
		expect(sheet.logged()).toBe(1);
		expect(sheet.note()).toBe("");
		expect(sheet.reps()).toBe(8);
		expect(sheet.loadKg()).toBe(40);
		expect(sheet.saving()).toBe(false);
		expect(onLogged).toHaveBeenCalledOnce();
	});

	/** Sets come in runs. A sheet that dismisses itself swallows the tap meant
	 *  for it — that is how a "Log set" tap once landed on the History tab. */
	it("stays open", () => {
		const { sheet, dismissed } = open({ exercises: [exercise(1)] });
		sheet.save();
		sheet.save();
		expect(dismissed).toEqual([]);
		expect(sheet.logged()).toBe(2);
	});

	it("reports the run's length when it is closed", () => {
		const { sheet, dismissed } = open({ exercises: [exercise(1)] });
		sheet.save();
		sheet.save();
		sheet.done();
		expect(dismissed).toEqual([2]);
	});
});

// --- when it doesn't ------------------------------------------------------

describe("when the server objects", () => {
	function failing(err: unknown, data: Partial<LogSheetData> = {}): LogSheet {
		const { sheet } = open({ exercises: [exercise(1)], ...data });
		vi.spyOn(TestBed.inject(CoachApi), "logSet").mockReturnValue(
			throwError(() => err),
		);
		return sheet;
	}

	it("shows what the server said", () => {
		const sheet = failing({ status: 422, error: { error: "reps must be 1–100" } });
		sheet.save();
		expect(sheet.error()).toBe("reps must be 1–100");
		expect(sheet.saving()).toBe(false);
	});

	/** A swallowed rejection looks exactly like a logged set, which is worse
	 *  than the bad value it refused. */
	it("does not count a set that was refused", () => {
		const sheet = failing({ status: 422, error: { error: "no" } });
		sheet.save();
		expect(sheet.logged()).toBe(0);
	});

	/** The ingress and the network answer with HTML, a differently-shaped JSON,
	 *  or nothing. None of those may reach the screen as "[object Object]". */
	for (const [name, err] of [
		["an HTML error page", { status: 502, error: "<html>502</html>" }],
		["a differently-shaped body", { status: 500, error: { message: "boom" } }],
		["an empty body", { status: 0, error: null }],
		["nothing at all", undefined],
	] as const) {
		it(`falls back to plain words for ${name}`, () => {
			const sheet = failing(err);
			sheet.save();
			expect(sheet.error()).toBe("That didn't save — try again");
		});
	}

	it("treats an empty server message as no message", () => {
		const sheet = failing({ status: 422, error: { error: "" } });
		sheet.save();
		expect(sheet.error()).toBe("That didn't save — try again");
	});
});

describe("when the server queries a surprising load", () => {
	function queried(): LogSheet {
		const { sheet } = open({
			exercises: [exercise(1, { metric: "weighted_reps" })],
			prefill: { exerciseId: 1, reps: 5, loadKg: 400 },
		});
		vi.spyOn(TestBed.inject(CoachApi), "logSet").mockReturnValue(
			throwError(() => ({ status: 409, error: { error: "400 kg — really?" } })),
		);
		return sheet;
	}

	it("asks rather than refuses, and keeps the typed numbers", () => {
		const sheet = queried();
		sheet.save();
		expect(sheet.confirmLoad()).toBe("400 kg — really?");
		expect(sheet.error()).toBeNull();
		expect(sheet.reps()).toBe(5);
		expect(sheet.loadKg()).toBe(400);
	});

	/** A 409 with no message we can read is not a question anyone can answer. */
	it("is an ordinary failure when the query has no words", () => {
		const { sheet } = open({ exercises: [exercise(1)] });
		vi.spyOn(TestBed.inject(CoachApi), "logSet").mockReturnValue(
			throwError(() => ({ status: 409, error: {} })),
		);
		sheet.save();
		expect(sheet.confirmLoad()).toBeNull();
		expect(sheet.error()).toBe("That didn't save — try again");
	});

	it("drops the question when the next attempt is made", () => {
		const sheet = queried();
		sheet.save();
		vi.spyOn(TestBed.inject(CoachApi), "logSet").mockReturnValue(of(saved));
		sheet.save(true);
		expect(sheet.confirmLoad()).toBeNull();
		expect(sheet.logged()).toBe(1);
	});
});
