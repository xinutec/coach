import { HttpClient } from "@angular/common/http";
import { Injectable, inject } from "@angular/core";
import { Observable } from "rxjs";
import {
  CurrentLocation,
  DetectedPlace,
  Equipment,
  Exercise,
  ExerciseDetail,
  ExercisePatch,
  Location,
  LocationPatch,
  Me,
  Muscle,
  NewExercise,
  NewLocation,
  NewPin,
  NewSet,
  PacingNow,
  Program,
  ProgramDetail,
  ProgramPin,
  ProgramTarget,
  Settings,
  SettingsPatch,
  StarterRequest,
  TargetPatch,
  WorkoutSet,
} from "./models";

/** Thin client over the coach backend. Same-origin in prod; via the dev proxy
 *  (proxy.conf.json) in `ng serve`. The session cookie rides along automatically. */
@Injectable({ providedIn: "root" })
export class CoachApi {
  private http = inject(HttpClient);

  me(): Observable<Me> {
    return this.http.get<Me>("/api/me");
  }
  logout(): Observable<unknown> {
    return this.http.post("/logout", {});
  }

  // Exercise catalog
  exercises(includeInactive = false): Observable<Exercise[]> {
    const q = includeInactive ? "?includeInactive=true" : "";
    return this.http.get<Exercise[]>(`/api/exercises${q}`);
  }
  exercise(id: number): Observable<ExerciseDetail> {
    return this.http.get<ExerciseDetail>(`/api/exercises/${id}`);
  }
  createExercise(body: NewExercise): Observable<ExerciseDetail> {
    return this.http.post<ExerciseDetail>("/api/exercises", body);
  }
  patchExercise(id: number, body: ExercisePatch): Observable<ExerciseDetail> {
    return this.http.patch<ExerciseDetail>(`/api/exercises/${id}`, body);
  }
  /** URL of an exercise's demo image blob (immutable; ETag-cached by the browser). */
  exerciseImageUrl(id: number): string {
    return `/api/exercises/${id}/image`;
  }

  // Reference catalogs
  equipment(): Observable<Equipment[]> {
    return this.http.get<Equipment[]>("/api/equipment");
  }
  muscles(): Observable<Muscle[]> {
    return this.http.get<Muscle[]>("/api/muscles");
  }

  // Training locations
  locations(): Observable<Location[]> {
    return this.http.get<Location[]>("/api/locations");
  }
  createLocation(body: NewLocation): Observable<Location> {
    return this.http.post<Location>("/api/locations", body);
  }
  patchLocation(id: number, body: LocationPatch): Observable<Location> {
    return this.http.patch<Location>(`/api/locations/${id}`, body);
  }
  deleteLocation(id: number): Observable<void> {
    return this.http.delete<void>(`/api/locations/${id}`);
  }
  /** Places health-sync has detected for the user, to link a location to. */
  placesDetected(): Observable<DetectedPlace[]> {
    return this.http.get<DetectedPlace[]>("/api/places/detected");
  }
  /** Which location the user is currently at (auto-detected), if any. */
  locationCurrent(): Observable<CurrentLocation> {
    return this.http.get<CurrentLocation>("/api/location/current");
  }

  // Programs
  programs(): Observable<Program[]> {
    return this.http.get<Program[]>("/api/programs");
  }
  activeProgram(): Observable<ProgramDetail | null> {
    return this.http.get<ProgramDetail | null>("/api/programs/active");
  }
  program(id: number): Observable<ProgramDetail> {
    return this.http.get<ProgramDetail>(`/api/programs/${id}`);
  }
  createStarter(body: StarterRequest = { startDate: null }): Observable<ProgramDetail> {
    return this.http.post<ProgramDetail>("/api/programs/starter", body);
  }
  activateProgram(id: number): Observable<void> {
    return this.http.post<void>(`/api/programs/${id}/activate`, {});
  }
  patchTarget(id: number, body: TargetPatch): Observable<ProgramTarget> {
    return this.http.patch<ProgramTarget>(`/api/program-targets/${id}`, body);
  }
  upsertPin(programId: number, body: NewPin): Observable<ProgramPin> {
    return this.http.post<ProgramPin>(`/api/programs/${programId}/pins`, body);
  }
  deletePin(programId: number, pinId: number): Observable<void> {
    return this.http.delete<void>(`/api/programs/${programId}/pins/${pinId}`);
  }

  // Micro-log
  sets(limit = 50): Observable<WorkoutSet[]> {
    return this.http.get<WorkoutSet[]>(`/api/sets?limit=${limit}`);
  }
  logSet(body: NewSet): Observable<WorkoutSet> {
    return this.http.post<WorkoutSet>("/api/sets", body);
  }
  deleteSet(id: number): Observable<void> {
    return this.http.delete<void>(`/api/sets/${id}`);
  }

  // Pacing settings + the live verdict
  settings(): Observable<Settings> {
    return this.http.get<Settings>("/api/settings");
  }
  patchSettings(body: SettingsPatch): Observable<Settings> {
    return this.http.patch<Settings>("/api/settings", body);
  }
  /** The pacing verdict; pass a location id to make the suggestion location-aware. */
  pacingNow(locationId?: number): Observable<PacingNow> {
    const q = locationId != null ? `?locationId=${locationId}` : "";
    return this.http.get<PacingNow>(`/api/pacing/now${q}`);
  }
}
