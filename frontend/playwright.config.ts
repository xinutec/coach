import { defineConfig, devices } from "@playwright/test";
import { phoneConfig } from "@xinutec/ui-harness/config";
import harness from "./e2e/harness.mjs";

/**
 * Layout harness (L2 of dev-lint/docs/layout-quality-architecture.md): render the
 * production build in a real browser at true device geometry and assert about the
 * painted pixels — text overlap, horizontal overflow, and occluded controls (a
 * control drawn under a fixed bar, the coach FAB-under-nav bug). The SW ships only
 * in `ng build`, so it runs against the built bundle.
 *
 * Everything shared — the Pixel geometry, the port, the static server that
 * serves the bundle — comes from @xinutec/ui-harness. What this app says about
 * itself is in e2e/harness.mjs. `npm run ui-check` builds first.
 *
 * Tests live in e2e/ (outside src/), so the vitest unit runner ignores them.
 */
export default defineConfig(phoneConfig(harness, devices));
