import { expect, type Page, test } from '@playwright/test';
// The fleet-shared harness, published as @xinutec/ui-harness (source repo
// ~/Code/ui-harness). Ships compiled JS, so it loads straight from node_modules.
import {
  expectBackClosesOverlay,
  expectCleanLayout,
  expectNoOccludedControls,
  expectRecoversFromMissingBundle,
  expectUpInTheBar,
  expectViewportIsPhone,
} from '@xinutec/ui-harness';
import type {
  CurrentLocation,
  DetectedPlace,
  Equipment,
  Exercise,
  ExerciseDetail,
  GroupBalance,
  Location,
  Me,
  PacingNow,
  Settings,
  WorkoutSet,
} from '../src/app/models';

/**
 * Layout-measurement checks: render coach's screens against the built bundle with
 * the backend mocked, and assert the three layout failure classes that read fine
 * in source and only show in a real browser — text collisions, horizontal
 * overflow, and OCCLUDED controls (a tappable control drawn under a fixed bar).
 * The occlusion check runs at a wide viewport too: the log-a-set FAB sinks behind
 * the bottom nav at ≥768px — invisible at phone width.
 */

/**
 * Raw values that reached the screen. `{{ someObject }}` compiles even under
 * `--strictTemplates`, and template lint isn't type-aware, so the rendered DOM is
 * the only place to catch it (the .ts side is covered by typescript-eslint).
 * `NaN` is the arithmetic version: a missing denominator painted as a number.
 */
const LEAKED = /\[object Object\]|\bNaN\b|\bundefined\b/;

/** After EVERY test in this file, not at each assertion point — a check you have
 *  to remember to call is one that stops covering new screens the day it's
 *  written. This costs one page.evaluate per test and covers whatever the test
 *  happened to render. */
test.afterEach(async ({ page }) => {
  if (page.isClosed()) return;
  const leaks = await page.evaluate((src: string) => {
    const re = new RegExp(src);
    return (document.body.innerText || '')
      .split('\n')
      .map((l) => l.trim())
      .filter((l) => re.test(l));
  }, LEAKED.source);
  expect(leaks, 'a raw value was painted on screen').toEqual([]);
});

const ME = { userId: 'test', displayName: 'Test User', avatarUrl: '' } satisfies Me;

const SETTINGS = {
  timezone: 'Europe/London',
  windowStartHour: 8,
  windowEndHour: 21,
  minRestMin: 20,
  mode: 'balanced',
  daysPerWeek: 4,
  emphasis: null,
} satisfies Settings;

const EXERCISES = [
  {
    id: 1,
    slug: 'pull_up_bar',
    name: 'Pull-up',
    variation: 'bar',
    pattern: 'pull',
    metric: 'reps',
    unilateral: false,
    skill: false,
    warmup: false,
    power: false,
    implements: 1,
    difficulty: null,
    isActive: true,
    equipment: ['pull_up_bar'],
    imageVersion: null,
  },
  {
    id: 6,
    slug: 'ring_dip',
    name: 'Ring dip',
    variation: null,
    pattern: 'push',
    metric: 'reps',
    unilateral: false,
    skill: false,
    warmup: false,
    power: false,
    implements: 1,
    difficulty: null,
    isActive: true,
    equipment: ['gymnastic_rings'],
    imageVersion: null,
  },
  {
    id: 11,
    slug: 'goblet_squat',
    name: 'Goblet squat',
    variation: null,
    pattern: 'legs',
    metric: 'weighted_reps',
    unilateral: false,
    skill: false,
    warmup: false,
    power: false,
    implements: 1,
    difficulty: null,
    isActive: true,
    equipment: ['dumbbell'],
    imageVersion: null,
  },
] satisfies Exercise[];

const EQUIPMENT = [
  {
    id: 1,
    slug: 'pull_up_bar',
    name: 'Pull-up bar',
    category: 'rig',
    loadable: false,
    weighted: false,
  },
  {
    id: 2,
    slug: 'gymnastic_rings',
    name: 'Gymnastic rings',
    category: 'rig',
    loadable: false,
    weighted: false,
  },
  {
    id: 3,
    slug: 'dumbbell',
    name: 'Dumbbell',
    category: 'free_weight',
    loadable: false,
    weighted: true,
  },
  {
    id: 4,
    slug: 'barbell',
    name: 'Barbell',
    category: 'free_weight',
    loadable: true,
    weighted: true,
  },
] satisfies Equipment[];

const LOCATIONS = [
  {
    id: 1,
    name: 'Home',
    isDefault: true,
    roomForPower: true,
    equipment: ['pull_up_bar', 'gymnastic_rings', 'dumbbell', 'barbell'],
    equipmentOptions: [
      {
        slug: 'dumbbell',
        weights: [10, 15, 20],
        weightQty: [],
        labels: [],
        barKg: null,
        barQty: null,
        plateSlots: null,
      },
      {
        slug: 'barbell',
        weights: [],
        weightQty: [],
        labels: [],
        barKg: 20,
        barQty: null,
        plateSlots: null,
      },
    ],
    // Pairs only: a plate you own one of is dead weight on a bar.
    plates: [
      { equipment: null, loadKg: 20, qty: 2 },
      { equipment: null, loadKg: 10, qty: 4 },
    ],
    healthPlaceId: null,
  },
] satisfies Location[];

// Two days of sets (loggedAt is UTC, no 'Z' — the client appends it).
const SETS = [
  {
    id: 1,
    exerciseId: 6,
    loggedAt: '2024-10-28T09:30:00',
    reps: 8,
    loadKg: null,
    holdS: null,
    distanceM: null,
    rpe: null,
    note: null,
  },
  {
    id: 2,
    exerciseId: 11,
    loggedAt: '2024-10-28T09:10:00',
    reps: 10,
    loadKg: 20,
    holdS: null,
    distanceM: null,
    rpe: null,
    note: null,
  },
  {
    id: 3,
    exerciseId: 1,
    loggedAt: '2024-10-20T18:00:00',
    reps: 6,
    loadKg: null,
    holdS: null,
    distanceM: null,
    rpe: null,
    note: null,
  },
] satisfies WorkoutSet[];

// A busy "active" verdict so Today renders fully (status line, reason, the
// ordered plan, the FAB). `groups` feeds the Balance tab.
const GROUPS = [
  {
    group: 'Lats',
    region: 'back',
    current: 2,
    target: 10,
    deficit: 0.8,
    recovering: false,
  },
  {
    group: 'Chest',
    region: 'chest',
    current: 6,
    target: 10,
    deficit: 0.4,
    recovering: false,
  },
  {
    group: 'Quadriceps',
    region: 'legs',
    current: 8,
    target: 12,
    deficit: 0.33,
    recovering: true,
  },
] satisfies GroupBalance[];
const PACING = {
  state: 'active',
  deload: false,
  readiness: { score: 0.82, band: 'high' },
  nudge: true,
  // Readiness is woven into the coach's sentence server-side (no chips).
  reason:
    "Recovered — good day to push. 2 × Ring dip (Chest) — you're a bit light there this week.",
  window: 'within',
  spacingOk: true,
  minutesSinceLastSet: 33,
  dayTargetSets: 6,
  dayDoneSets: 1,
  groups: GROUPS,
  suggestion: {
    exerciseId: 6,
    exerciseName: 'Ring dip',
    pattern: 'push',
    kind: 'work',
    sets: 2,
    logged: [],
    ask: { kind: 'bodyweight', repLow: 5, repHigh: 8 },
    group: 'Chest',
    substitutedFor: null,
    explanation: null,
    newBest: null,
  },
  // The ordered session: a warm-up (leads), a work item + a calibration item.
  plan: [
    {
      exerciseId: 20,
      exerciseName: 'Arm circles',
      pattern: 'core',
      kind: 'warmup',
      sets: 1,
      logged: [],
      ask: { kind: 'bodyweight', repLow: 8, repHigh: 8 },
      group: 'Shoulders',
      substitutedFor: null,
      explanation: null,
      newBest: null,
    },
    {
      exerciseId: 6,
      exerciseName: 'Ring dip',
      pattern: 'push',
      kind: 'work',
      sets: 2,
      logged: [],
      ask: { kind: 'bodyweight', repLow: 5, repHigh: 8 },
      group: 'Chest',
      substitutedFor: null,
      explanation: {
        deficit: 0.4,
        recovery: 1,
        pays: 2.4,
        confidence: 'high',
        e1rm: null,
        readiness: 'high',
        confirming: false,
        estimateFrom: null,
        misses: 0,
        offCardKg: null,
        steppedFromKg: null,
      },
      newBest: null,
    },
    {
      exerciseId: 11,
      exerciseName: 'Goblet squat',
      pattern: 'legs',
      kind: 'assess',
      sets: 1,
      logged: [],
      ask: { kind: 'buildUp', startKg: 20, reps: 5, leave: 0 },
      group: 'Quadriceps',
      substitutedFor: null,
      explanation: {
        deficit: 0.33,
        recovery: 0.5,
        pays: 1.2,
        confidence: 'none',
        e1rm: null,
        readiness: 'high',
        confirming: false,
        estimateFrom: null,
        misses: 0,
        offCardKg: null,
        steppedFromKg: null,
      },
      newBest: null,
    },
  ],
  // Kit present but with no registered weights: the coach drops those lifts
  // rather than guessing a load, and says so.
  notices: [
    "No weights registered here for Kettlebell — I've left its exercises out rather than guess a load.",
  ],
  // A rest the athlete asked for, with its button to end it early.
  resting: [{ exerciseId: 12, exerciseName: 'Bulgarian split squat', until: '2026-10-15' }],
} satisfies PacingNow;

// GET /api/exercises/6 — the library sheet's own fetch. The catch-all answers it
// with `[]`, which is not an ExerciseDetail, so the sheet needs its own mock.
const DETAIL = {
  id: 6,
  slug: 'ring_dip',
  name: 'Ring dip',
  variation: null,
  pattern: 'push',
  metric: 'reps',
  position: null,
  unilateral: false,
  isActive: true,
  cue: 'Rings turned out at the top, elbows in.',
  demoUrl: null,
  summary: null,
  difficulty: 3,
  imageVersion: null,
  loopVersion: null,
  imageCredit: null,
  equipment: [
    {
      id: 2,
      slug: 'gymnastic_rings',
      name: 'Gymnastic rings',
      category: 'rig',
      loadable: false,
      weighted: false,
    },
  ],
  muscles: [
    {
      slug: 'pec_major',
      name: 'Pectoralis major',
      group: 'Chest',
      region: 'chest',
      role: 'primary',
    },
    { slug: 'triceps', name: 'Triceps', group: 'Triceps', region: 'arms', role: 'secondary' },
  ],
} satisfies ExerciseDetail;

/** Mock every backend call. Catch-all FIRST — Playwright runs handlers
 *  last-registered-first, so the specific routes below win. */
async function mockApi(page: Page): Promise<void> {
  await page.route('**/api/**', (r) =>
    // dev-lint: allow-untyped-mock the catch-all answers GETs no test reads; every route a test reads has its own typed mock below
    r.request().method() === 'GET' ? r.fulfill({ json: [] }) : r.fulfill({ status: 204, body: '' }),
  );
  await page.route('**/api/me', (r) => r.fulfill({ json: ME }));
  await page.route('**/api/pacing/now*', (r) => r.fulfill({ json: PACING }));
  await page.route('**/api/exercises*', (r) => r.fulfill({ json: EXERCISES }));
  await page.route('**/api/equipment', (r) => r.fulfill({ json: EQUIPMENT }));
  await page.route('**/api/locations', (r) => r.fulfill({ json: LOCATIONS }));
  await page.route('**/api/places/detected', (r) =>
    r.fulfill({ json: [] satisfies DetectedPlace[] }),
  );
  await page.route('**/api/location/current', (r) =>
    r.fulfill({ json: { locationId: null } satisfies CurrentLocation }),
  );
  await page.route('**/api/settings', (r) => r.fulfill({ json: SETTINGS }));
}

test('the suite really runs at phone geometry', async ({ page }) => {
  await mockApi(page);
  await page.goto('/today');
  await expectViewportIsPhone(page);
});

// A service worker can serve an index naming a bundle a later deploy removed, and
// the app's own update handling is inside that bundle. The recovery is inline in
// `src/index.html`; this checks it is there and works.
test('a bundle a deploy removed reloads into the app, not a blank screen', async ({ page }) => {
  await mockApi(page);
  await expectRecoversFromMissingBundle(page, '/today', '.add-fab');
});

test('today — busy composition: clean + all controls reachable @ phone', async ({
  page,
}, testInfo) => {
  await mockApi(page);
  await page.goto('/today');
  await page.getByText('a bit light', { exact: false }).waitFor();
  await page.locator('.add-fab').waitFor();
  // The readiness note arrives inside the coach's one sentence, not a chip.
  await page.getByText('Recovered — good day to push', { exact: false }).waitFor();
  await expectCleanLayout(page, testInfo);
  await expectNoOccludedControls(page, testInfo);
});

// An unauthenticated visitor (no session → /api/me 401s) must get a visible way
// in, not empty chrome: a sign-in card that links to /login (→ Nextcloud OAuth).
test('signed-out — the sign-in card offers a way in @ phone', async ({ page }, testInfo) => {
  // dev-lint: allow-untyped-mock a 401's body is never read: the status alone signs the visitor out
  await page.route('**/api/me', (r) => r.fulfill({ status: 401, json: {} }));
  await page.goto('/today');
  const signIn = page.getByRole('link', { name: 'Sign in with Nextcloud' });
  await signIn.waitFor();
  await expect(signIn).toHaveAttribute('href', '/login');
  await expectCleanLayout(page, testInfo);
});

test('settings — clean + reachable @ phone', async ({ page }, testInfo) => {
  await mockApi(page);
  await page.goto('/settings');
  await page.getByRole('button', { name: 'Check for updates' }).waitFor();
  await expectCleanLayout(page, testInfo);
  await expectNoOccludedControls(page, testInfo);
});

test('library — exercise cards render clean @ phone', async ({ page }, testInfo) => {
  await mockApi(page);
  await page.goto('/library');
  await page.getByRole('heading', { name: 'Exercise library' }).waitFor();
  await page.getByText('Ring dip').waitFor();
  await expectCleanLayout(page, testInfo);
  await expectNoOccludedControls(page, testInfo);
});

const SHEET = '.mat-bottom-sheet-container';

// The two bottom sheets are the app's other half — every number the athlete
// actually types or reads mid-set is in one of them — and no page-level test
// opens either, so nothing (including the leaked-value check above) has ever
// looked at them.
test('log sheet — the fields the athlete types into render clean @ phone', async ({
  page,
}, testInfo) => {
  await mockApi(page);
  await page.goto('/today');
  await page.locator('.add-fab').click();
  await page.getByRole('button', { name: /^Log set$/ }).waitFor();
  // Scoped to the sheet: an open bottom sheet is painted OVER the nav, so the
  // nav's labels sit under an opaque surface. Unscoped, the harness reads that
  // as "Note (optional)" colliding with "Today" — a collision no eye can see.
  await expectCleanLayout(page, testInfo, { root: SHEET });
});

test('exercise sheet — the library detail renders clean @ phone', async ({ page }, testInfo) => {
  await mockApi(page);
  await page.route('**/api/exercises/6', (r) => r.fulfill({ json: DETAIL }));
  await page.goto('/library');
  await page.getByText('Ring dip').click();
  await page.getByText('Pectoralis major').waitFor();
  await expectCleanLayout(page, testInfo, { root: SHEET });
});

test('exercise sheet — a credited picture and loop render clean @ phone', async ({
  page,
}, testInfo) => {
  await mockApi(page);
  await page.route('**/api/exercises/6', (r) =>
    r.fulfill({
      json: {
        ...DETAIL,
        imageVersion: 'img1',
        loopVersion: 'loop1',
        imageCredit: {
          text: 'Anatomy from Z-Anatomy (based on BodyParts3D), CC BY-SA 4.0',
          url: 'https://github.com/Z-Anatomy',
        },
      } satisfies ExerciseDetail,
    }),
  );
  await page.goto('/library');
  await page.getByText('Ring dip').click();
  // One under the picture, one under the loop.
  await expect(page.locator('.credit')).toHaveCount(2);
  await expectCleanLayout(page, testInfo, { root: SHEET });
});

test('locations — location card + kit chips render clean @ phone', async ({ page }, testInfo) => {
  await mockApi(page);
  await page.goto('/locations');
  await page.getByText('Home').waitFor();
  await expectCleanLayout(page, testInfo);
  await expectNoOccludedControls(page, testInfo);
});

test('history — collapsible days with year on old dates @ phone', async ({ page }, testInfo) => {
  await mockApi(page);
  await page.route(/\/api\/sets(\?|$)/, (r) => r.fulfill({ json: SETS }));
  await page.goto('/history');
  // Old dates carry the year; the newest day is expanded so its sets show.
  await page.getByText('2024', { exact: false }).first().waitFor();
  await page.getByText('Ring dip').waitFor();
  await expectCleanLayout(page, testInfo);
  await expectNoOccludedControls(page, testInfo);
});

test('balance — muscle-group volume bars render clean @ phone', async ({ page }, testInfo) => {
  await mockApi(page);
  await page.goto('/balance');
  await page.getByRole('heading', { name: 'Balance' }).waitFor();
  await page.getByText('Lats').waitFor();
  await expectCleanLayout(page, testInfo);
  await expectNoOccludedControls(page, testInfo);
});

// Past the window the coach's sentence says "rolls to tomorrow" — the plan must
// agree: headed as tomorrow's session, no "Next up" pressure, no burn-down.
test("today — after the window the plan reads as tomorrow's preview @ phone", async ({
  page,
}, testInfo) => {
  await mockApi(page);
  await page.route('**/api/pacing/now*', (r) =>
    r.fulfill({
      json: {
        ...PACING,
        nudge: false,
        window: 'after',
        reason: "It's late — this rolls to tomorrow.",
      } satisfies PacingNow,
    }),
  );
  await page.goto('/today');
  await page.getByText('rolls to tomorrow', { exact: false }).waitFor();
  await page.getByRole('heading', { name: "Tomorrow's session" }).waitFor();
  await expect(page.locator('.next-pill')).toHaveCount(0);
  await expectCleanLayout(page, testInfo);
});

// A set that beats every earlier one is named on its row, which at phone width
// already carries a name, a receipt and an action.
test('today — a new best is named on its finished row @ phone', async ({ page }, testInfo) => {
  await mockApi(page);
  await page.route('**/api/pacing/now*', (r) =>
    r.fulfill({
      json: {
        ...PACING,
        plan: [
          ...PACING.plan,
          {
            exerciseId: 7,
            exerciseName: 'Pull-up (bar)',
            pattern: 'pull',
            kind: 'work',
            sets: 2,
            logged: [
              { reps: 6, loadKg: null, holdS: null, distanceM: null },
              { reps: 7, loadKg: null, holdS: null, distanceM: null },
            ],
            ask: { kind: 'bodyweight', repLow: 6, repHigh: 12 },
            group: 'Lats',
            substitutedFor: null,
            explanation: null,
            newBest: { reps: 7, loadKg: null, holdS: null, distanceM: null },
          },
        ],
      } satisfies PacingNow,
    }),
  );
  await page.goto('/today');
  await page.locator('.suggestion.compact.done .best-pill').waitFor();
  await expectCleanLayout(page, testInfo);
});

// Mid-session the screen must show the work, not the receipts: finished items and
// warm-ups render as compact rows, so the first unfinished card is above the fold.
// The question is "what now?", not "what have I done?".
test('today — mid-session, the next thing to do is on screen @ phone', async ({
  page,
}, testInfo) => {
  await mockApi(page);
  await page.route('**/api/pacing/now*', (r) =>
    r.fulfill({
      json: {
        ...PACING,
        // Warm-up done, and the first work item half done — the state the
        // page spends most of a session in.
        plan: PACING.plan.map((s, i) =>
          i === 0
            ? { ...s, logged: [{ reps: 10, loadKg: null, holdS: null, distanceM: null }] }
            : i === 1
              ? { ...s, logged: [{ reps: 7, loadKg: null, holdS: null, distanceM: null }] }
              : s,
        ),
      } satisfies PacingNow,
    }),
  );
  await page.goto('/today');
  await page.getByRole('heading', { name: "Today's session" }).waitFor();

  // The finished warm-up is a row, not a card.
  await expect(page.locator('.suggestion.compact.done')).toHaveCount(1);

  // The first card with sets still to do must be fully visible without
  // scrolling — measured against the scroll container, which is what the
  // athlete actually sees (the shell is 100dvh with `.content` scrolling).
  const gap = await page.evaluate(() => {
    const next = document.querySelector('.suggestion.next');
    const view = document.querySelector('main.content');
    if (!next || !view) return null;
    return view.getBoundingClientRect().bottom - next.getBoundingClientRect().bottom;
  });
  expect(gap, 'the next-up card is cut off by the fold').not.toBeNull();
  expect(gap ?? -1).toBeGreaterThan(0);

  // Warm-ups credit no volume, so they are not the session's measure: two work
  // sets of Ring dip and one calibration, one of them done.
  await expect(page.locator('.plan-count')).toHaveText(/1 \/ 3 sets/);

  // The half-done card says what set one actually was, not just that there was
  // one — otherwise "what did I do last set?" is a trip to History mid-movement.
  await expect(page.locator('.s-logged')).toHaveText(/7 reps/);

  await expectCleanLayout(page, testInfo);
  await expectNoOccludedControls(page, testInfo);
});

// When health reports a current location, the status line shows it was detected.
test("today — auto-detected location shows the 'detected' hint @ phone", async ({
  page,
}, testInfo) => {
  await mockApi(page);
  await page.route('**/api/location/current', (r) =>
    r.fulfill({ json: { locationId: 1 } satisfies CurrentLocation }),
  );
  await page.goto('/today');
  await page.getByText('a bit light', { exact: false }).waitFor();
  await page.locator('.status-line .auto').waitFor();
  await expectCleanLayout(page, testInfo);
});

test('today — kit with no registered weights is named, not silently dropped @ phone', async ({
  page,
}, testInfo) => {
  // The coach won't invent a load for a lift whose weights aren't registered, so
  // it leaves the lift out. A drop the athlete can't see just looks like a hole
  // in the plan — so it says which kit to fix.
  await mockApi(page);
  await page.goto('/today');
  await page.getByText('a bit light', { exact: false }).waitFor();
  await page.locator('.notice').getByText('Kettlebell', { exact: false }).waitFor();
  await expectCleanLayout(page, testInfo);
});

test('locations is reachable from the UI @ phone', async ({ page }) => {
  // /locations had no link anywhere in the app — you could only get there by
  // typing the URL. The kit registered there bounds every prescription (an
  // unregistered weight means the lift is dropped), so an unreachable page is a
  // dead end you can't recover from inside the app.
  await mockApi(page);
  await page.goto('/settings');
  await page.getByRole('link', { name: /Locations/i }).click();
  await page.waitForURL('**/locations');
  await page.getByRole('heading', { name: 'Locations' }).waitFor();
  // Up returns to the screen that opened it.
  await expectUpInTheBar(page);
  await page.locator('ui-scaffold').getByRole('button', { name: 'back' }).click();
  await page.waitForURL('**/settings');
});

// Sheets open through the scaffold's `Sheets`, which gives each one a history
// entry: the phone's back gesture closes the sheet, not the screen under it.
test('back closes a sheet and stays on the screen @ phone', async ({ page }) => {
  await mockApi(page);
  await page.goto('/today');
  await expectBackClosesOverlay(page, () => page.locator('.add-fab').click());
  await page.goto('/library');
  await expectBackClosesOverlay(page, () => page.locator('.card').first().click());
});

// The FAB-under-nav bug lives at ≥768px (tablet/landscape), where the phone
// suite is blind. Same page, wide viewport, occlusion assertion.
test.describe('wide viewport (tablet/landscape)', () => {
  test.use({ viewport: { width: 1024, height: 800 } });

  test('today — the FAB is not occluded by the bottom nav @ 1024px', async ({ page }, testInfo) => {
    await mockApi(page);
    await page.goto('/today');
    await page.locator('.add-fab').waitFor();
    // This case's concern is the fixed FAB sinking behind the nav in wide mode
    // (its origin). Check the FAB specifically — the now-taller session plan
    // legitimately scrolls at this short height, which the all-controls default
    // would flag as a scroll artifact, not a real occlusion.
    await expectNoOccludedControls(page, testInfo, '.add-fab');
  });
});
