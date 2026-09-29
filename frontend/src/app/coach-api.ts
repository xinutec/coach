import { HttpClient } from '@angular/common/http';
import { Injectable, inject } from '@angular/core';
import { Observable } from 'rxjs';
import {
  CurrentLocation,
  DetectedPlace,
  Equipment,
  Exercise,
  ExerciseDetail,
  Location,
  LocationPatch,
  Me,
  Muscle,
  NewLocation,
  NewSet,
  PacingNow,
  Settings,
  SettingsPatch,
  WorkoutSet,
} from './models';

/** Thin client over the coach backend. Same-origin in prod; via the dev proxy
 *  (proxy.conf.json) in `ng serve`. The session cookie rides along automatically. */
@Injectable({ providedIn: 'root' })
export class CoachApi {
  private http = inject(HttpClient);

  me(): Observable<Me> {
    return this.http.get<Me>('/api/me');
  }
  logout(): Observable<unknown> {
    return this.http.post('/logout', {});
  }

  exercises(includeInactive = false): Observable<Exercise[]> {
    const q = includeInactive ? '?includeInactive=true' : '';
    return this.http.get<Exercise[]>(`/api/exercises${q}`);
  }
  exercise(id: number): Observable<ExerciseDetail> {
    return this.http.get<ExerciseDetail>(`/api/exercises/${id}`);
  }
  /** An exercise's picture. Served as immutable, so the version is in the URL: a
   *  new render is a new URL rather than a year of the old one. */
  exerciseImageUrl(id: number, version: string): string {
    return `/api/exercises/${id}/image?v=${version}`;
  }
  /** The generated 3D demo loop, versioned the same way. Ask only when there is
   *  a `loopVersion`: most exercises have a photograph and no loop. */
  exerciseLoopUrl(id: number, version: string): string {
    return `/api/exercises/${id}/loop?v=${version}`;
  }

  equipment(): Observable<Equipment[]> {
    return this.http.get<Equipment[]>('/api/equipment');
  }
  muscles(): Observable<Muscle[]> {
    return this.http.get<Muscle[]>('/api/muscles');
  }

  locations(): Observable<Location[]> {
    return this.http.get<Location[]>('/api/locations');
  }
  createLocation(body: NewLocation): Observable<Location> {
    return this.http.post<Location>('/api/locations', body);
  }
  patchLocation(id: number, body: LocationPatch): Observable<Location> {
    return this.http.patch<Location>(`/api/locations/${id}`, body);
  }
  deleteLocation(id: number): Observable<void> {
    return this.http.delete<void>(`/api/locations/${id}`);
  }
  /** Places health-sync has detected for the user, to link a location to. */
  placesDetected(): Observable<DetectedPlace[]> {
    return this.http.get<DetectedPlace[]>('/api/places/detected');
  }
  /** Which location the user is currently at (auto-detected), if any. */
  locationCurrent(): Observable<CurrentLocation> {
    return this.http.get<CurrentLocation>('/api/location/current');
  }

  sets(limit = 50): Observable<WorkoutSet[]> {
    return this.http.get<WorkoutSet[]>(`/api/sets?limit=${limit}`);
  }
  logSet(body: NewSet): Observable<WorkoutSet> {
    return this.http.post<WorkoutSet>('/api/sets', body);
  }
  deleteSet(id: number): Observable<void> {
    return this.http.delete<void>(`/api/sets/${id}`);
  }

  settings(): Observable<Settings> {
    return this.http.get<Settings>('/api/settings');
  }
  patchSettings(body: Partial<SettingsPatch>): Observable<Settings> {
    return this.http.patch<Settings>('/api/settings', body);
  }
  /** The coach verdict; pass a location (for doability). Mode comes from settings. */
  pacingNow(locationId?: number): Observable<PacingNow> {
    const q = locationId != null ? `?locationId=${locationId}` : '';
    return this.http.get<PacingNow>(`/api/pacing/now${q}`);
  }
}
