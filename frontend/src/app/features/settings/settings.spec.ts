import { TestBed } from "@angular/core/testing";
import { type Observable, of, throwError } from "rxjs";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { BUILD_INFO } from "../../build-info";
import { CoachApi } from "../../coach-api";
import type { Settings } from "../../models";
import { SwUpdates } from "../../sw-updates";
import { SettingsPage } from "./settings";

const settings: Settings = {
	mode: "balanced",
	daysPerWeek: 4,
	emphasis: null,
	timezone: "Europe/London",
	windowStartHour: 7,
	windowEndHour: 21,
	minRestMin: 45,
};

/** The message-port bridge the current APK injects. */
class FakeBridge {
	readonly posted: string[] = [];
	private listeners: ((event: { data: string }) => void)[] = [];

	postMessage(message: string): void {
		this.posted.push(message);
	}

	addEventListener(
		_type: "message",
		listener: (event: { data: string }) => void,
	): void {
		this.listeners.push(listener);
	}

	/** The phone volunteering an answer — the same path as a reply. */
	say(data: string): void {
		for (const l of this.listeners) l({ data });
	}
}

/** What an APK older than this page injects: three plain methods from the
 *  `addJavascriptInterface` era, and no `addEventListener` at all. */
const legacyBridge = {
	remindersStatus: () => "{}",
	setupReminders: () => undefined,
	disableReminders: () => undefined,
};

interface Harness {
	page: SettingsPage;
	patched: Partial<Settings>[];
	checkNow: ReturnType<typeof vi.fn>;
}

type UpdateResult = Awaited<ReturnType<SwUpdates["checkNow"]>>;

function settingsPage(
	over: { patch?: () => Observable<Settings>; update?: UpdateResult } = {},
): Harness {
	const patched: Partial<Settings>[] = [];
	const checkNow = vi.fn(() => Promise.resolve(over.update ?? "current"));
	TestBed.configureTestingModule({
		providers: [
			{
				provide: CoachApi,
				useValue: {
					settings: () => of(settings),
					patchSettings: (body: Partial<Settings>) => {
						patched.push(body);
						return over.patch ? over.patch() : of(settings);
					},
				},
			},
			{ provide: SwUpdates, useValue: { checkNow } },
		],
	});
	const page = TestBed.runInInjectionContext(() => new SettingsPage());
	return { page, patched, checkNow };
}

afterEach(() => {
	TestBed.resetTestingModule();
	delete window.CoachAndroid;
	vi.useRealTimers();
});

// --- telling a phone from a browser ----------------------------------------

describe("the reminders card", () => {
	it("is offered when the app injects a message port", () => {
		window.CoachAndroid = new FakeBridge();
		const { page } = settingsPage();
		expect(page.isAndroid()).toBe(true);
	});

	it("is absent in a plain browser", () => {
		const { page } = settingsPage();
		expect(page.isAndroid()).toBe(false);
	});

	/** The app is sideloaded, so the page always updates before the APK. An older
	 *  app has no `addEventListener`, and calling it would throw and take the
	 *  whole Settings page down rather than just this card. Reading as "no
	 *  bridge" is the degradation: install the current APK and it returns. */
	it("is absent against an app too old to speak this protocol", () => {
		window.CoachAndroid = legacyBridge;
		const { page } = settingsPage();
		expect(page.isAndroid()).toBe(false);
	});

	it("asks nothing of an app it cannot talk to", () => {
		window.CoachAndroid = legacyBridge;
		const { page } = settingsPage();
		expect(() => {
			page.enableReminders();
			page.disableReminders();
		}).not.toThrow();
	});

	it("does nothing in a browser rather than throwing", () => {
		const { page } = settingsPage();
		expect(() => {
			page.enableReminders();
			page.disableReminders();
		}).not.toThrow();
	});
});

// --- the conversation with the phone ---------------------------------------

describe("talking to the phone", () => {
	let bridge: FakeBridge;

	beforeEach(() => {
		bridge = new FakeBridge();
		window.CoachAndroid = bridge;
	});

	it("asks for the state as soon as the page opens", () => {
		settingsPage();
		expect(bridge.posted).toEqual(["status"]);
	});

	it("sends the word for each control", () => {
		const { page } = settingsPage();
		page.enableReminders();
		page.disableReminders();
		expect(bridge.posted).toEqual(["status", "setup", "disable"]);
	});

	it("reads the state the phone reports", () => {
		const { page } = settingsPage();
		bridge.say(JSON.stringify({ hasHome: true, armed: true }));
		expect(page.remindersHasHome()).toBe(true);
		expect(page.remindersArmed()).toBe(true);
	});

	/** The whole reason the reply path exists: the flow settles when it settles,
	 *  and the phone volunteers the result rather than the page guessing with a
	 *  timer 1500 ms after asking. */
	it("takes an answer nobody asked for", () => {
		const { page } = settingsPage();
		page.enableReminders();
		bridge.say(JSON.stringify({ hasHome: true, armed: true }));
		expect(page.remindersArmed()).toBe(true);
	});

	it("follows the phone back down when reminders are turned off", () => {
		const { page } = settingsPage();
		bridge.say(JSON.stringify({ hasHome: true, armed: true }));
		bridge.say(JSON.stringify({ hasHome: true, armed: false }));
		expect(page.remindersArmed()).toBe(false);
		expect(page.remindersHasHome()).toBe(true);
	});

	/** A home is set but reminders are off — the card offers "turn on" rather
	 *  than "set home here", so the two booleans are not interchangeable. */
	it("keeps the two facts apart", () => {
		const { page } = settingsPage();
		bridge.say(JSON.stringify({ hasHome: true, armed: false }));
		expect(page.remindersHasHome()).toBe(true);
		expect(page.remindersArmed()).toBe(false);
	});

	it("believes only a real yes", () => {
		const { page } = settingsPage();
		bridge.say(JSON.stringify({ hasHome: "yes", armed: 1 }));
		expect(page.remindersHasHome()).toBe(false);
		expect(page.remindersArmed()).toBe(false);
	});

	for (const [name, data] of [
		["not JSON at all", "<html>nope</html>"],
		["a bare number", "5"],
		["null", "null"],
		["nothing", ""],
	] as const) {
		it(`survives ${name}`, () => {
			const { page } = settingsPage();
			expect(() => bridge.say(data)).not.toThrow();
			expect(page.remindersArmed()).toBe(false);
		});
	}

	it("does not lose a state it already had to a garbled message", () => {
		const { page } = settingsPage();
		bridge.say(JSON.stringify({ hasHome: true, armed: true }));
		bridge.say("}{");
		expect(page.remindersArmed()).toBe(true);
	});
});

// --- the settings themselves -----------------------------------------------

describe("saving", () => {
	it("loads the current settings into the form", () => {
		const { page } = settingsPage();
		expect(page.form()).toEqual(settings);
	});

	it("sends the form and keeps what comes back", () => {
		const { page, patched } = settingsPage();
		page.form.set({ ...settings, daysPerWeek: 6, mode: "strength" });
		page.save();
		expect(patched).toEqual([{ ...settings, daysPerWeek: 6, mode: "strength" }]);
		expect(page.form()).toEqual(settings);
		expect(page.saving()).toBe(false);
	});

	it("says so, briefly", () => {
		vi.useFakeTimers();
		const { page } = settingsPage();
		page.save();
		expect(page.saved()).toBe(true);
		vi.advanceTimersByTime(2000);
		expect(page.saved()).toBe(false);
	});

	it("stops saying it is saving when the save fails", () => {
		const { page } = settingsPage({
			patch: () => throwError((): Error => new Error("nope")),
		});
		page.save();
		expect(page.saving()).toBe(false);
		expect(page.saved()).toBe(false);
	});

	it("sends nothing before the form has loaded", () => {
		const { page, patched } = settingsPage();
		page.form.set(null);
		page.save();
		expect(patched).toEqual([]);
	});
});

// --- which build this tab is ------------------------------------------------

/** Read against the real stamp: `vi.mock` is unavailable for relative imports
 *  under Angular's unit-test system, and `build-info.ts` is generated by the
 *  build. So the unreadable-date fallback is not reachable from here — what is
 *  checkable is that the line names *this* bundle and renders a real date, which
 *  is what makes "Up to date." something you can hold against the server. */
describe("the build stamp", () => {
	it("names this bundle's commit, not the server's", () => {
		const { page } = settingsPage();
		expect(page.buildStamp()).toBe(`Build ${BUILD_INFO.sha} · ${new Date(BUILD_INFO.builtAt).toLocaleString()}`);
	});

	it("never shows the words a failed date parse would leave", () => {
		const { page } = settingsPage();
		expect(page.buildStamp()).not.toContain("Invalid Date");
	});
});

describe("checking for an update", () => {
	for (const [result, message] of [
		["current", "Up to date."],
		["updating", "Updating…"],
		["unsupported", "No service worker (dev build)."],
	] as const) {
		it(`reports ${result}`, async () => {
			const { page } = settingsPage({ update: result });
			await page.checkUpdates();
			expect(page.updateMsg()).toBe(message);
		});
	}

	it("says nothing until asked", () => {
		const { page, checkNow } = settingsPage();
		expect(page.updateMsg()).toBe("");
		expect(checkNow).not.toHaveBeenCalled();
	});
});

describe("labels", () => {
	it("capitalises what the taxonomy spells in lower case", () => {
		const { page } = settingsPage();
		expect(page.modes.map((m) => page.label(m))).toEqual([
			"Balanced",
			"Strength",
			"Skills",
			"Conditioning",
		]);
		expect(page.label("")).toBe("");
	});
});
