import { Injectable, inject } from "@angular/core";

import { CachedResource } from "../shared/cached-resource";
import { CoachApi } from "../coach-api";
import { DetectedPlace, Equipment, Exercise, Location, PacingNow, WorkoutSet } from "../models";

/** Root-scoped caches for the server read-catalogs the routed tabs show. Being
 *  singletons, they retain their data across a tab switch (the component is
 *  destroyed, the store isn't) and let every view of the same data share one
 *  fetch. Each is just a loader — all the retain/refresh/error logic lives once
 *  in {@link CachedResource}. Call `.refresh()` on entering a view and after a
 *  mutation; read `.value()` / `.loaded()` in the template. */

/** Active exercises — Today (suggestions) and Library (the list). */
@Injectable({ providedIn: "root" })
export class ExercisesStore extends CachedResource<Exercise[]> {
  constructor() {
    const api = inject(CoachApi);
    super(() => api.exercises());
  }
}

/** All exercises incl. retired — History's id→exercise lookup needs inactive ones. */
@Injectable({ providedIn: "root" })
export class AllExercisesStore extends CachedResource<Exercise[]> {
  constructor() {
    const api = inject(CoachApi);
    super(() => api.exercises(true));
  }
}

/** Equipment reference — Today, Library and Locations. */
@Injectable({ providedIn: "root" })
export class EquipmentStore extends CachedResource<Equipment[]> {
  constructor() {
    const api = inject(CoachApi);
    super(() => api.equipment());
  }
}

/** Training locations — Today and Locations. */
@Injectable({ providedIn: "root" })
export class LocationsStore extends CachedResource<Location[]> {
  constructor() {
    const api = inject(CoachApi);
    super(() => api.locations());
  }
}

/** Health-sync detected places — Locations (link a location to one). */
@Injectable({ providedIn: "root" })
export class PlacesStore extends CachedResource<DetectedPlace[]> {
  constructor() {
    const api = inject(CoachApi);
    super(() => api.placesDetected());
  }
}

/** Recent workout sets — History. */
@Injectable({ providedIn: "root" })
export class SetsStore extends CachedResource<WorkoutSet[]> {
  constructor() {
    const api = inject(CoachApi);
    super(() => api.sets(100));
  }
}

/** The default (no location/mode) pacing verdict — Balance. Today fetches its
 *  own, parameterised by the selected location + mode, so it stays local. */
@Injectable({ providedIn: "root" })
export class PacingStore extends CachedResource<PacingNow> {
  constructor() {
    const api = inject(CoachApi);
    super(() => api.pacingNow());
  }
}
