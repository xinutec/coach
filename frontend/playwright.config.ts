import { defineConfig, devices } from '@playwright/test';
import { phoneConfig } from '@xinutec/ui-harness/config';
import harness from './e2e/harness.mjs';

/**
 * Layout harness (L2 of dev-lint/docs/layout-quality-architecture.md): the
 * production build in a real browser at device geometry, asserting on painted
 * pixels — text overlap, horizontal overflow, occluded controls. Shared settings
 * come from @xinutec/ui-harness, this app's from e2e/harness.mjs. It reads the
 * built dist, so build first.
 */
export default defineConfig(phoneConfig(harness, devices));
