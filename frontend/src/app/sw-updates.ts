import { Injectable, inject } from '@angular/core';
import { SwUpdate, VersionReadyEvent } from '@angular/service-worker';
import {
  type PagePort,
  type ServiceWorkerPort,
  SwUpdates as SwUpdatePolicy,
  type UpdateOutcome,
} from '@xinutec/ui-harness/sw-updates';
import { filter } from 'rxjs';

export type { UpdateOutcome };

/** Marks that we have already auto-reloaded out of an unrecoverable service worker
 *  state. Session-scoped so it survives that very reload. Unchanged from when this
 *  logic lived here, so a tab mid-recovery across the upgrade still sees its mark. */
const RECOVERY_KEY = 'coach.sw-recovery-attempted';

/**
 * Self-update — the Angular wiring. The rules live in
 * `@xinutec/ui-harness/sw-updates`; this is the adapter.
 *
 * ⚠ **This repo carried its own 78-line copy until 2026-09-13**, written before the
 * policy was shared and never migrated — dev-lint#1384 recorded the fleet as being
 * back to one implementation while this was quietly a second. `DL-NGSW-NO-UPDATE`
 * could not see it either: coach HAD an update path, just not the shared one. It was
 * found by reading the rule's output, not by the rule.
 *
 * ⚠ **The shared policy is not merely equivalent — it fixes a bug this copy had.**
 * The old `checkNow()` asked ngsw for a newer build and reported `'current'` when it
 * said no. But a build already downloaded and STAGED mid-session answers no, so
 * pressing "Check for updates" with an update waiting said "Up to date." The policy
 * applies the staged build instead.
 *
 * ⚠ **The policy is deliberately not an `@Injectable`.** ui-harness compiles with
 * plain `tsc`, so a decorated service would ship without the metadata `ngtsc`
 * generates and fail to inject in an AOT build. It is therefore free of Angular and
 * rxjs entirely, and unit-tested against a fake — which it never was while welded to
 * a real service worker.
 *
 * What stays here is what a fake cannot reach: that `SwUpdate.versionUpdates` really
 * feeds it, filtered to VERSION_READY, and that a reload really happens.
 */
@Injectable({ providedIn: 'root' })
export class SwUpdates {
  private readonly sw = inject(SwUpdate);

  private readonly serviceWorker: ServiceWorkerPort = ((sw: SwUpdate) => ({
    // Bound to a local, not `this`: an object-literal getter does not capture the
    // enclosing `this` lexically, and a copied boolean would freeze `isEnabled` at
    // construction when start() must read the live value.
    get isEnabled(): boolean {
      return sw.isEnabled;
    },
    onVersionReady: (handler: () => void): void => {
      sw.versionUpdates
        .pipe(filter((event): event is VersionReadyEvent => event.type === 'VERSION_READY'))
        .subscribe(() => handler());
    },
    onUnrecoverable: (handler: () => void): void => {
      // The cached build is broken and the server no longer holds the files to repair
      // it — what a roll-forward deploy of :latest leaves a client whose cache was
      // evicted meanwhile. Nothing recovers from here except a fresh load.
      sw.unrecoverable.subscribe(() => handler());
    },
    checkForUpdate: () => sw.checkForUpdate(),
    activateUpdate: () => sw.activateUpdate(),
  }))(this.sw);

  private readonly page: PagePort = {
    get hidden(): boolean {
      return document.visibilityState === 'hidden';
    },
    onVisibilityChange: (handler: () => void): void => {
      document.addEventListener('visibilitychange', handler);
    },
    recoveryAttempted: () => sessionStorage.getItem(RECOVERY_KEY) !== null,
    markRecoveryAttempted: () => sessionStorage.setItem(RECOVERY_KEY, '1'),
    // Routed through the method below rather than called directly, so a test can
    // assert "this would have reloaded" without navigating the test runner.
    reload: () => this.reload(),
    now: () => Date.now(),
  };

  private readonly policy = new SwUpdatePolicy(this.serviceWorker, this.page);

  start(): void {
    this.policy.start();
  }

  /** Manual "Check for updates" (Settings). Never rejects — every failure comes back
   *  as `'failed'` so the caller can say so. */
  checkNow(): Promise<UpdateOutcome> {
    return this.policy.checkNow();
  }

  /** The one place the page is thrown away. Its own method so tests can assert
   *  "this would have reloaded" without navigating the test runner. */
  reload(): void {
    document.location.reload();
  }
}
