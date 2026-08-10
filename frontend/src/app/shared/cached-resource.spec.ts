import { Observable } from "rxjs";
import { describe, expect, it } from "vitest";

import { CachedResource } from "./cached-resource";

/** The read model every routed tab sits on. Its four promises are written into
 *  its doc comment and were asserted nowhere: a failed background refresh is not
 *  an error, `loaded` never goes back to false, a second refresh cancels the
 *  first, and `patch` shows immediately. Each is invisible until a refactor
 *  breaks it, and then it breaks every screen at once — a placeholder on every
 *  revisit, or a retry button over data that is fine. */

interface Call {
  /** Answer this fetch, as the server would. */
  emit(value: string): void;
  /** Fail it, as an offline phone would. */
  fail(): void;
  /** Torn down before it produced anything — i.e. `switchMap` dropped it. */
  cancelled(): boolean;
}

/** A loader driven by hand: nothing resolves until the test says so, and each
 *  call is addressable, so "the stale one answered late" is expressible. */
function controllable(): { loader: () => Observable<string>; calls: Call[] } {
  const calls: Call[] = [];
  const loader = () =>
    new Observable<string>((sub) => {
      let settled = false;
      let torn = false;
      calls.push({
        emit: (value: string) => {
          settled = true;
          sub.next(value);
          sub.complete();
        },
        fail: () => {
          settled = true;
          sub.error(new Error("offline"));
        },
        // Teardown runs on complete and error too, so "cancelled" has to mean
        // torn down *without* having settled — otherwise every call looks
        // cancelled and the assertion proves nothing.
        cancelled: () => torn && !settled,
      });
      return () => {
        torn = true;
      };
    });
  return { loader, calls };
}

function nth(calls: Call[], n: number): Call {
  const call = calls[n];
  if (!call) throw new Error(`expected a fetch #${n}; the loader ran ${calls.length} time(s)`);
  return call;
}

describe("before anything is asked for", () => {
  it("does not fetch until told to", () => {
    const { loader, calls } = controllable();
    const res = new CachedResource(loader);
    expect(calls.length).toBe(0);
    expect(res.value()).toBeNull();
    expect(res.loaded()).toBe(false);
    expect(res.error()).toBe(false);
    expect(res.refreshing()).toBe(false);
  });
});

describe("the first load", () => {
  it("is in flight, then holds the value", () => {
    const { loader, calls } = controllable();
    const res = new CachedResource(loader);
    res.refresh();
    expect(res.refreshing()).toBe(true);
    expect(res.loaded()).toBe(false);
    expect(res.value()).toBeNull();

    nth(calls, 0).emit("catalog");
    expect(res.value()).toBe("catalog");
    expect(res.loaded()).toBe(true);
    expect(res.refreshing()).toBe(false);
    expect(res.error()).toBe(false);
  });

  /** `loaded` gates the loading placeholder, so a failed first load has to
   *  settle it too — otherwise the screen spins forever instead of offering the
   *  retry that `error` is there to trigger. */
  it("settles even when it fails, and says so", () => {
    const { loader, calls } = controllable();
    const res = new CachedResource(loader);
    res.refresh();
    nth(calls, 0).fail();
    expect(res.loaded()).toBe(true);
    expect(res.refreshing()).toBe(false);
    expect(res.error()).toBe(true);
    expect(res.value()).toBeNull();
  });
});

describe("a revisit never shows the placeholder again", () => {
  /** The whole reason the store outlives the component. If `loaded` dipped back
   *  to false while a background refresh was in flight, every tab switch would
   *  blank to a spinner over data already on screen. */
  it("keeps loaded true across a refresh, in flight and after", () => {
    const { loader, calls } = controllable();
    const res = new CachedResource(loader);
    res.refresh();
    nth(calls, 0).emit("first");

    res.refresh();
    expect(res.loaded()).toBe(true);
    nth(calls, 1).emit("second");
    expect(res.loaded()).toBe(true);
  });

  it("keeps the old value visible while the new one is being fetched", () => {
    const { loader, calls } = controllable();
    const res = new CachedResource(loader);
    res.refresh();
    nth(calls, 0).emit("first");

    res.refresh();
    expect(res.value()).toBe("first");
    expect(res.refreshing()).toBe(true);
    nth(calls, 1).emit("second");
    expect(res.value()).toBe("second");
  });
});

describe("what counts as an error", () => {
  /** The invariant with the most consequence: a phone that drops its connection
   *  mid-session must not replace a rendered plan with a retry button. Only a
   *  failure with nothing to fall back on is an error. */
  it("is not an error when a background refresh fails over cached data", () => {
    const { loader, calls } = controllable();
    const res = new CachedResource(loader);
    res.refresh();
    nth(calls, 0).emit("plan");

    res.refresh();
    nth(calls, 1).fail();
    expect(res.error()).toBe(false);
    expect(res.value()).toBe("plan");
    expect(res.loaded()).toBe(true);
    expect(res.refreshing()).toBe(false);
  });

  it("clears the error while a retry is in flight, rather than showing both", () => {
    const { loader, calls } = controllable();
    const res = new CachedResource(loader);
    res.refresh();
    nth(calls, 0).fail();
    expect(res.error()).toBe(true);

    res.refresh();
    expect(res.error()).toBe(false);
    expect(res.refreshing()).toBe(true);
  });

  it("goes away when the retry succeeds", () => {
    const { loader, calls } = controllable();
    const res = new CachedResource(loader);
    res.refresh();
    nth(calls, 0).fail();

    res.refresh();
    nth(calls, 1).emit("arrived");
    expect(res.error()).toBe(false);
    expect(res.value()).toBe("arrived");
  });

  /** `catchError` sits inside `switchMap`, on the inner observable. Hoisted out
   *  to the outer pipe it would still pass this suite's happy path, and the
   *  first failure would silently end the subscription for the app's lifetime —
   *  every later refresh a no-op, on a screen with no way to say so. */
  it("still works after a failure — the pipeline is not dead", () => {
    const { loader, calls } = controllable();
    const res = new CachedResource(loader);
    res.refresh();
    nth(calls, 0).fail();

    res.refresh();
    expect(calls.length).toBe(2);
    nth(calls, 1).emit("recovered");
    expect(res.value()).toBe("recovered");
    expect(res.error()).toBe(false);
  });
});

describe("racing refreshes", () => {
  /** Callers are told to refresh freely — on view entry and after every
   *  mutation — so overlapping fetches are the normal case, not the edge one. */
  it("drops the in-flight fetch when another starts", () => {
    const { loader, calls } = controllable();
    const res = new CachedResource(loader);
    res.refresh();
    res.refresh();
    expect(calls.length).toBe(2);
    expect(nth(calls, 0).cancelled()).toBe(true);
    expect(nth(calls, 1).cancelled()).toBe(false);
  });

  /** The failure cancellation exists to prevent: a slow first response landing
   *  after a fresher second one and overwriting it. Here the stale fetch answers
   *  last, and must be ignored precisely because it was already dropped. */
  it("ignores a stale answer that arrives after a newer one", () => {
    const { loader, calls } = controllable();
    const res = new CachedResource(loader);
    res.refresh();
    res.refresh();
    nth(calls, 1).emit("fresh");
    nth(calls, 0).emit("stale");
    expect(res.value()).toBe("fresh");
  });

  it("is no longer refreshing once the live fetch settles", () => {
    const { loader, calls } = controllable();
    const res = new CachedResource(loader);
    res.refresh();
    res.refresh();
    expect(res.refreshing()).toBe(true);
    nth(calls, 1).emit("fresh");
    expect(res.refreshing()).toBe(false);
  });
});

describe("patching after a local mutation", () => {
  it("shows the change immediately, without a fetch", () => {
    const { loader, calls } = controllable();
    const res = new CachedResource<string>(loader);
    res.refresh();
    nth(calls, 0).emit("one");

    res.patch((current) => `${current ?? ""} + two`);
    expect(res.value()).toBe("one + two");
    expect(calls.length).toBe(1);
  });

  it("hands the updater what is cached now, which may be nothing yet", () => {
    const { loader } = controllable();
    const res = new CachedResource<string>(loader);
    const seen: (string | null)[] = [];
    res.patch((current) => {
      seen.push(current);
      return "optimistic";
    });
    expect(seen).toEqual([null]);
    expect(res.value()).toBe("optimistic");
    expect(res.loaded()).toBe(false);
  });

  /** The optimistic half of the contract: the patch is a guess, and the
   *  background refresh is what makes it true or corrects it. */
  it("is overwritten by what the server actually says", () => {
    const { loader, calls } = controllable();
    const res = new CachedResource<string>(loader);
    res.refresh();
    nth(calls, 0).emit("one");
    res.patch(() => "guessed");

    res.refresh();
    nth(calls, 1).emit("authoritative");
    expect(res.value()).toBe("authoritative");
  });
});
