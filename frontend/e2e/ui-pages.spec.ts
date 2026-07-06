import { expect, type Page, test } from "@playwright/test";
// The fleet-shared harness, published as @xinutec/ui-harness (source repo
// ~/Code/ui-harness). Ships compiled JS, so it loads straight from node_modules.
import {
	expectNoHorizontalOverflow,
	expectNoOccludedControls,
	expectNoTextOverlaps,
	expectViewportIsPhone,
} from "@xinutec/ui-harness";

/**
 * Layout-measurement checks: render coach's screens against the built bundle with
 * the backend mocked, and assert the three layout failure classes that read fine
 * in source and only show in a real browser — text collisions, horizontal
 * overflow, and OCCLUDED controls (a tappable control drawn under a fixed bar).
 * The occlusion check runs at a wide viewport too: the log-a-set FAB sinks behind
 * the bottom nav at ≥768px — invisible at phone width.
 *
 * The service worker is blocked: SW-controlled fetches bypass page.route.
 */
test.use({ serviceWorkers: "block" });

const ME = { userId: "test", displayName: "Test User", avatarUrl: "" };

const SETTINGS = {
	timezone: "Europe/London",
	windowStartHour: 8,
	windowEndHour: 21,
	nightCutoffHour: 21,
	minRestMin: 20,
	mode: "balanced",
	daysPerWeek: 4,
	emphasis: null,
};

const EXERCISES = [
	{
		id: 1,
		slug: "pull_up_bar",
		name: "Pull-up",
		variation: "bar",
		pattern: "pull",
		metric: "reps",
		unilateral: false,
		isActive: true,
		equipment: ["pull_up_bar"],
		hasImage: false,
	},
	{
		id: 6,
		slug: "ring_dip",
		name: "Ring dip",
		variation: null,
		pattern: "push",
		metric: "reps",
		unilateral: false,
		isActive: true,
		equipment: ["gymnastic_rings"],
		hasImage: false,
	},
	{
		id: 11,
		slug: "goblet_squat",
		name: "Goblet squat",
		variation: null,
		pattern: "legs",
		metric: "weighted_reps",
		unilateral: false,
		isActive: true,
		equipment: ["dumbbell"],
		hasImage: false,
	},
];

const EQUIPMENT = [
	{ id: 1, slug: "pull_up_bar", name: "Pull-up bar", category: "rig" },
	{ id: 2, slug: "gymnastic_rings", name: "Gymnastic rings", category: "rig" },
	{ id: 3, slug: "dumbbell", name: "Dumbbell", category: "free_weight" },
];

const LOCATIONS = [
	{
		id: 1,
		name: "Home",
		isDefault: true,
		equipment: ["pull_up_bar", "gymnastic_rings"],
	},
];

// Two days of sets (loggedAt is UTC, no 'Z' — the client appends it).
const SETS = [
	{
		id: 1,
		exerciseId: 6,
		programId: null,
		loggedAt: "2024-10-28T09:30:00",
		reps: 8,
		loadKg: null,
		holdS: null,
		rpe: null,
		note: null,
	},
	{
		id: 2,
		exerciseId: 11,
		programId: null,
		loggedAt: "2024-10-28T09:10:00",
		reps: 10,
		loadKg: 20,
		holdS: null,
		rpe: null,
		note: null,
	},
	{
		id: 3,
		exerciseId: 1,
		programId: null,
		loggedAt: "2024-10-20T18:00:00",
		reps: 6,
		loadKg: null,
		holdS: null,
		rpe: null,
		note: null,
	},
];

// A busy "active" verdict so Today renders fully (mode bar, reason, suggestion,
// balance bars, the FAB).
const GROUPS = [
	{
		group: "Lats",
		region: "back",
		current: 2,
		target: 10,
		deficit: 0.8,
		recovering: false,
	},
	{
		group: "Chest",
		region: "chest",
		current: 6,
		target: 10,
		deficit: 0.4,
		recovering: false,
	},
	{
		group: "Quadriceps",
		region: "legs",
		current: 8,
		target: 12,
		deficit: 0.33,
		recovering: true,
	},
];
const PACING = {
	state: "active",
	mode: "balanced",
	deload: false,
	readiness: { score: 0.82, band: "high" },
	nudge: true,
	reason: "2 × Ring dip (Chest) — you're a bit light there this week.",
	withinWindow: true,
	afterCutoff: false,
	spacingOk: true,
	minutesSinceLastSet: 33,
	dayTargetSets: 6,
	dayDoneSets: 1,
	groups: GROUPS,
	suggestion: {
		exerciseId: 6,
		exerciseName: "Ring dip",
		pattern: "push",
		sets: 2,
		repLow: 5,
		repHigh: 8,
		loadKg: null,
		holdS: null,
		group: "Chest",
		substitutedFor: null,
	},
};

/** Mock every backend call. Catch-all FIRST — Playwright runs handlers
 *  last-registered-first, so the specific routes below win. */
async function mockApi(page: Page): Promise<void> {
	await page.route("**/api/**", (r) =>
		r.request().method() === "GET"
			? r.fulfill({ json: [] })
			: r.fulfill({ status: 204, body: "" }),
	);
	await page.route("**/api/me", (r) => r.fulfill({ json: ME }));
	await page.route("**/api/pacing/now*", (r) => r.fulfill({ json: PACING }));
	await page.route("**/api/exercises*", (r) => r.fulfill({ json: EXERCISES }));
	await page.route("**/api/equipment", (r) => r.fulfill({ json: EQUIPMENT }));
	await page.route("**/api/locations", (r) => r.fulfill({ json: LOCATIONS }));
	await page.route("**/api/places/detected", (r) => r.fulfill({ json: [] }));
	await page.route("**/api/location/current", (r) =>
		r.fulfill({ json: { locationId: null } }),
	);
	await page.route("**/api/settings", (r) => r.fulfill({ json: SETTINGS }));
}

test("the suite really runs at phone geometry", async ({ page }) => {
	await mockApi(page);
	await page.goto("/today");
	await expectViewportIsPhone(page);
});

test("today — busy composition: clean + all controls reachable @ phone", async ({
	page,
}, testInfo) => {
	await mockApi(page);
	await page.goto("/today");
	await page.getByText("a bit light", { exact: false }).waitFor();
	await page.locator(".add-fab").waitFor();
	// Health-informed readiness chip renders from the biometric signal.
	await page.getByText("Recovered", { exact: false }).waitFor();
	await expectNoTextOverlaps(page, testInfo);
	await expectNoHorizontalOverflow(page, testInfo);
	await expectNoOccludedControls(page, testInfo);
});

// Regression: an unauthenticated visitor (no session → /api/me 401s) must get a
// visible way in. The app once swallowed the 401 and rendered empty chrome with
// no login affordance and no redirect — "where is the login?". Now it shows a
// sign-in card that links to /login (→ Nextcloud OAuth).
test("signed-out — the sign-in card offers a way in @ phone", async ({
	page,
}, testInfo) => {
	await page.route("**/api/me", (r) => r.fulfill({ status: 401, json: {} }));
	await page.goto("/today");
	const signIn = page.getByRole("link", { name: "Sign in with Nextcloud" });
	await signIn.waitFor();
	await expect(signIn).toHaveAttribute("href", "/login");
	await expectNoTextOverlaps(page, testInfo);
	await expectNoHorizontalOverflow(page, testInfo);
});

test("settings — clean + reachable @ phone", async ({ page }, testInfo) => {
	await mockApi(page);
	await page.goto("/settings");
	await page.getByRole("button", { name: "Check for updates" }).waitFor();
	await expectNoTextOverlaps(page, testInfo);
	await expectNoHorizontalOverflow(page, testInfo);
	await expectNoOccludedControls(page, testInfo);
});

test("library — exercise cards render clean @ phone", async ({
	page,
}, testInfo) => {
	await mockApi(page);
	await page.goto("/library");
	await page.getByRole("heading", { name: "Exercise library" }).waitFor();
	await page.getByText("Ring dip").waitFor();
	await expectNoTextOverlaps(page, testInfo);
	await expectNoHorizontalOverflow(page, testInfo);
	await expectNoOccludedControls(page, testInfo);
});

test("locations — location card + kit chips render clean @ phone", async ({
	page,
}, testInfo) => {
	await mockApi(page);
	await page.goto("/locations");
	await page.getByText("Home").waitFor();
	await expectNoTextOverlaps(page, testInfo);
	await expectNoHorizontalOverflow(page, testInfo);
	await expectNoOccludedControls(page, testInfo);
});

test("history — collapsible days with year on old dates @ phone", async ({
	page,
}, testInfo) => {
	await mockApi(page);
	await page.route(/\/api\/sets(\?|$)/, (r) => r.fulfill({ json: SETS }));
	await page.goto("/history");
	// Old dates carry the year; the newest day is expanded so its sets show.
	await page.getByText("2024", { exact: false }).first().waitFor();
	await page.getByText("Ring dip").waitFor();
	await expectNoTextOverlaps(page, testInfo);
	await expectNoHorizontalOverflow(page, testInfo);
	await expectNoOccludedControls(page, testInfo);
});

test("balance — muscle-group volume bars render clean @ phone", async ({
	page,
}, testInfo) => {
	await mockApi(page);
	await page.goto("/balance");
	await page.getByRole("heading", { name: "Balance" }).waitFor();
	await page.getByText("Lats").waitFor();
	await expectNoTextOverlaps(page, testInfo);
	await expectNoHorizontalOverflow(page, testInfo);
	await expectNoOccludedControls(page, testInfo);
});

// When health reports a current location, Today shows the "here" auto hint.
test("today — auto-detected location shows the 'here' hint @ phone", async ({
	page,
}, testInfo) => {
	await mockApi(page);
	await page.route("**/api/location/current", (r) =>
		r.fulfill({ json: { locationId: 1 } }),
	);
	await page.goto("/today");
	await page.getByText("a bit light", { exact: false }).waitFor();
	await page.locator(".auto-pill").waitFor();
	await expectNoTextOverlaps(page, testInfo);
	await expectNoHorizontalOverflow(page, testInfo);
});

// The FAB-under-nav bug lives at ≥768px (tablet/landscape), where the phone
// suite is blind. Same page, wide viewport, occlusion assertion.
test.describe("wide viewport (tablet/landscape)", () => {
	test.use({ viewport: { width: 1024, height: 800 } });

	test("today — the FAB is not occluded by the bottom nav @ 1024px", async ({
		page,
	}, testInfo) => {
		await mockApi(page);
		await page.goto("/today");
		await page.locator(".add-fab").waitFor();
		await expectNoOccludedControls(page, testInfo);
	});
});
