import { Injectable, inject } from '@angular/core';

import { CachedResource } from '../shared/cached-resource';
import { CoachApi } from '../coach-api';
import { DetectedPlace, Equipment, Exercise, Location, PacingNow, WorkoutSet } from '../models';

/** Root-scoped caches of the server's read-catalogs, shared by every view that
 *  reads them and kept across tab switches; see {@link CachedResource}. */

/** Active exercises. */
@Injectable({ providedIn: 'root' })
export class ExercisesStore extends CachedResource<Exercise[]> {
  constructor() {
    const api = inject(CoachApi);
    super(() => api.exercises());
  }
}

/** All exercises, retired ones included, for naming old sets. */
@Injectable({ providedIn: 'root' })
export class AllExercisesStore extends CachedResource<Exercise[]> {
  constructor() {
    const api = inject(CoachApi);
    super(() => api.exercises(true));
  }
}

/** The equipment catalog. */
@Injectable({ providedIn: 'root' })
export class EquipmentStore extends CachedResource<Equipment[]> {
  constructor() {
    const api = inject(CoachApi);
    super(() => api.equipment());
  }
}

/** The athlete's training locations. */
@Injectable({ providedIn: 'root' })
export class LocationsStore extends CachedResource<Location[]> {
  constructor() {
    const api = inject(CoachApi);
    super(() => api.locations());
  }
}

/** Places health-sync detected, to link a location to. */
@Injectable({ providedIn: 'root' })
export class PlacesStore extends CachedResource<DetectedPlace[]> {
  constructor() {
    const api = inject(CoachApi);
    super(() => api.placesDetected());
  }
}

/** Recent workout sets. */
@Injectable({ providedIn: 'root' })
export class SetsStore extends CachedResource<WorkoutSet[]> {
  constructor() {
    const api = inject(CoachApi);
    super(() => api.sets(100));
  }
}

/** The verdict at the default location. Today fetches its own, for the
 *  location it has selected. */
@Injectable({ providedIn: 'root' })
export class PacingStore extends CachedResource<PacingNow> {
  constructor() {
    const api = inject(CoachApi);
    super(() => api.pacingNow());
  }
}
