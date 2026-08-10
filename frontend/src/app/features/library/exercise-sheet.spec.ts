import { TestBed } from "@angular/core/testing";
import { MAT_BOTTOM_SHEET_DATA } from "@angular/material/bottom-sheet";
import { of } from "rxjs";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { CoachApi } from "../../coach-api";
import type { ExerciseDetail, ExerciseMuscle, MuscleRole } from "../../models";
import { ExerciseSheet } from "./exercise-sheet";

/** The library's detail sheet, and the only screen in the app that is *rendered*
 *  to be tested rather than driven through an injection context. It has to be:
 *  the component injects `ViewContainerRef`, which exists only in a node
 *  injector, so constructing it by hand throws before any assertion runs.
 *
 *  What is worth the harness is the rotation behaviour. The player is one
 *  template rendered in one of two places — inline in the sheet, or portalled
 *  into a full-viewport overlay when the phone turns sideways — because the
 *  sheet's own container is transformed, so a `position: fixed` child would
 *  resolve against the sheet instead of the screen. Nothing about that is
 *  visible from the outside except by looking for the overlay. */

const FS_PANE = ".demo-fs-pane";

function muscle(slug: string, role: MuscleRole): ExerciseMuscle {
	return { slug, name: slug, group: "chest", region: "chest", role };
}

function detail(over: Partial<ExerciseDetail> = {}): ExerciseDetail {
	return {
		id: 7,
		slug: "pull-up",
		name: "Pull-up",
		variation: null,
		pattern: "pull",
		metric: "reps",
		position: null,
		unilateral: false,
		isActive: true,
		cue: null,
		demoUrl: "https://youtu.be/3S5rnnI7VSs?t=11",
		summary: null,
		difficulty: 3,
		hasImage: true,
		equipment: [],
		muscles: [],
		...over,
	};
}

/** jsdom has no `matchMedia`, and the constructor calls it — so the stub is part
 *  of standing the component up, not a convenience. It also has to be
 *  controllable: rotating the phone is the behaviour under test. */
interface Screen {
	rotate(toLandscape: boolean): void;
	listeners(): number;
}

/** All the component reads off the event is `matches`, so that is all the stub
 *  promises. Manufacturing a whole `MediaQueryListEvent` would mean asserting a
 *  literal into a type jsdom does not construct, which says less and lies more. */
interface Rotation {
	matches: boolean;
}

let screen: Screen;

beforeEach(() => {
	const handlers = new Set<(e: Rotation) => void>();
	let landscape = false;
	Object.defineProperty(window, "matchMedia", {
		configurable: true,
		writable: true,
		value: (query: string) => ({
			matches: query.includes("landscape") ? landscape : false,
			media: query,
			addEventListener: (_: string, fn: (e: Rotation) => void) => {
				handlers.add(fn);
			},
			removeEventListener: (_: string, fn: (e: Rotation) => void) => {
				handlers.delete(fn);
			},
		}),
	});
	screen = {
		rotate: (toLandscape: boolean) => {
			landscape = toLandscape;
			for (const fn of handlers) fn({ matches: toLandscape });
		},
		listeners: () => handlers.size,
	};
});

function sheet(d: ExerciseDetail | null = detail()) {
	TestBed.configureTestingModule({
		providers: [
			{ provide: MAT_BOTTOM_SHEET_DATA, useValue: { exerciseId: 7 } },
			{
				provide: CoachApi,
				useValue: {
					exercise: () => of(d),
					exerciseImageUrl: (id: number) => `/api/exercises/${id}/image`,
				},
			},
		],
	});
	const fixture = TestBed.createComponent(ExerciseSheet);
	fixture.detectChanges();
	return fixture;
}

/** The overlay lives outside the fixture's own element — that is the point of
 *  portalling it — so it is found in the document, not in the component. */
function fullscreenPanes(): number {
	return document.querySelectorAll(FS_PANE).length;
}

afterEach(() => {
	TestBed.resetTestingModule();
});

describe("standing the sheet up", () => {
	it("asks for the exercise it was opened with and shows it", () => {
		const fixture = sheet();
		expect(fixture.componentInstance.detail()?.name).toBe("Pull-up");
	});

	it("starts on the picture, with nothing playing and no overlay", () => {
		const fixture = sheet();
		expect(fixture.componentInstance.playing()).toBeNull();
		expect(fixture.componentInstance.frameReady()).toBe(false);
		expect(fullscreenPanes()).toBe(0);
	});

	it("reads the orientation it opened at, rather than assuming portrait", () => {
		screen.rotate(true);
		const fixture = sheet();
		expect(fixture.componentInstance.landscape()).toBe(true);
	});
});

describe("the demo", () => {
	it("offers to play a link it can embed", () => {
		const fixture = sheet();
		expect(fixture.componentInstance.video()).toEqual({ id: "3S5rnnI7VSs", startS: 11 });
	});

	/** A movement with no demo, and one whose link isn't a YouTube video, are the
	 *  same case here: there is nothing to put in a frame. The template falls back
	 *  to linking out rather than building an embed from a guessed id.
	 *
	 *  Two tests rather than two assertions: the TestBed can only be configured
	 *  once per instantiation, so a second `sheet()` in the same `it` throws
	 *  before it can be compared. */
	it("offers nothing to embed for a movement with no demo at all", () => {
		expect(sheet(detail({ demoUrl: null })).componentInstance.video()).toBeNull();
	});

	it("offers nothing to embed for a demo that isn't a YouTube video", () => {
		expect(
			sheet(detail({ demoUrl: "https://vimeo.com/12345" })).componentInstance.video(),
		).toBeNull();
	});

	/** The frame is built on the tap, never on open: the picture is cached by the
	 *  service worker and a YouTube embed cannot be, so in a basement gym the
	 *  video is exactly the thing that fails. Building it eagerly would also call
	 *  Google for every movement merely glanced at. */
	it("builds the embed only when the athlete asks for it", () => {
		const fixture = sheet();
		expect(fixture.componentInstance.playing()).toBeNull();
		fixture.componentInstance.play();
		expect(fixture.componentInstance.playing()).not.toBeNull();
	});

	it("does nothing when asked to play something it cannot embed", () => {
		const fixture = sheet(detail({ demoUrl: null }));
		fixture.componentInstance.play();
		expect(fixture.componentInstance.playing()).toBeNull();
	});

	/** Dropping the frame is also what stops the playback — there is no player
	 *  object to pause, and a video still running behind a closed sheet is a thing
	 *  you would have to go and find. */
	it("goes back to the picture, which is what stops the sound", () => {
		const fixture = sheet();
		fixture.componentInstance.play();
		fixture.componentInstance.frameReady.set(true);
		fixture.componentInstance.stop();
		expect(fixture.componentInstance.playing()).toBeNull();
		expect(fixture.componentInstance.frameReady()).toBe(false);
	});
});

describe("turning the phone sideways", () => {
	/** The whole reason the player is a portal. Landscape gives the demo the
	 *  viewport — not true fullscreen, because `requestFullscreen` needs a user
	 *  gesture and a rotation is not one, so the browser would refuse. */
	it("lifts a playing video out to a full-viewport overlay", () => {
		const fixture = sheet();
		fixture.componentInstance.play();
		fixture.detectChanges();
		expect(fullscreenPanes()).toBe(0);

		screen.rotate(true);
		fixture.detectChanges();
		expect(fullscreenPanes()).toBe(1);
	});

	it("puts it back in the sheet when the phone comes upright", () => {
		const fixture = sheet();
		fixture.componentInstance.play();
		screen.rotate(true);
		fixture.detectChanges();
		expect(fullscreenPanes()).toBe(1);

		screen.rotate(false);
		fixture.detectChanges();
		expect(fullscreenPanes()).toBe(0);
	});

	/** Rotation alone is not the trigger — there has to be something to lift.
	 *  Otherwise merely holding the phone sideways in the library would throw an
	 *  empty black overlay over the page. */
	it("does nothing on its own when no video is playing", () => {
		const fixture = sheet();
		screen.rotate(true);
		fixture.detectChanges();
		expect(fullscreenPanes()).toBe(0);
	});

	it("takes the overlay away when the video is stopped from inside it", () => {
		const fixture = sheet();
		fixture.componentInstance.play();
		screen.rotate(true);
		fixture.detectChanges();
		expect(fullscreenPanes()).toBe(1);

		fixture.componentInstance.stop();
		fixture.detectChanges();
		expect(fullscreenPanes()).toBe(0);
	});

	/** Moving the frame reloads it — the iframe is destroyed and recreated in the
	 *  new place — so the still has to come back up while YouTube comes down the
	 *  wire again. Leaving `frameReady` true paints the sheet black for a second
	 *  and reads as a broken rotation. */
	it("puts the still back up while the moved frame reloads", () => {
		const fixture = sheet();
		fixture.componentInstance.play();
		fixture.componentInstance.frameReady.set(true);

		screen.rotate(true);
		fixture.detectChanges();
		expect(fixture.componentInstance.frameReady()).toBe(false);
	});

	it("only opens one overlay, however many times it is asked", () => {
		const fixture = sheet();
		fixture.componentInstance.play();
		screen.rotate(true);
		fixture.detectChanges();
		screen.rotate(true);
		fixture.detectChanges();
		expect(fullscreenPanes()).toBe(1);
	});
});

describe("closing the sheet", () => {
	/** A full-screen player outliving the sheet that opened it would be a video
	 *  covering the app with nothing left to close it. */
	it("takes the overlay with it", () => {
		const fixture = sheet();
		fixture.componentInstance.play();
		screen.rotate(true);
		fixture.detectChanges();
		expect(fullscreenPanes()).toBe(1);

		fixture.destroy();
		expect(fullscreenPanes()).toBe(0);
	});

	it("stops listening to the orientation", () => {
		const fixture = sheet();
		expect(screen.listeners()).toBe(1);
		fixture.destroy();
		expect(screen.listeners()).toBe(0);
	});
});

describe("what the sheet says about a movement", () => {
	it("names a variation as its own movement", () => {
		const fixture = sheet();
		const page = fixture.componentInstance;
		expect(page.displayName(detail({ variation: "L-sit" }))).toBe("Pull-up (L-sit)");
		expect(page.displayName(detail())).toBe("Pull-up");
	});

	/** Primary is what the movement is *for*; everything else — secondary and
	 *  stabiliser alike — is what it also asks of you. The split is two-way on
	 *  screen even though the data has three roles, so a stabiliser must land on
	 *  the second list rather than vanish. */
	it("splits the muscles two ways, with stabilisers on the secondary side", () => {
		const page = sheet(
			detail({
				muscles: [
					muscle("lats", "primary"),
					muscle("biceps", "secondary"),
					muscle("core", "stabilizer"),
				],
			}),
		).componentInstance;
		const d = page.detail();
		if (!d) throw new Error("fixture");
		expect(page.primary(d).map((m) => m.slug)).toEqual(["lats"]);
		expect(page.secondary(d).map((m) => m.slug)).toEqual(["biceps", "core"]);
	});

	it("capitalises the pattern for display", () => {
		const page = sheet().componentInstance;
		expect(page.patternLabel("pull")).toBe("Pull");
	});

	it("points the picture at the exercise's own image", () => {
		const page = sheet().componentInstance;
		expect(page.imageUrl(7)).toBe("/api/exercises/7/image");
	});
});
