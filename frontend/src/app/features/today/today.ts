import { Component, computed, effect, inject, signal } from "@angular/core";
import { MatBottomSheet } from "@angular/material/bottom-sheet";
import { MatButtonModule } from "@angular/material/button";
import { MatCardModule } from "@angular/material/card";
import { MatIconModule } from "@angular/material/icon";
import { MatMenuModule } from "@angular/material/menu";
import { MatProgressBarModule } from "@angular/material/progress-bar";
import { RouterLink } from "@angular/router";
import { CoachApi } from "../../coach-api";
import type { Band, Explanation, PacingNow, Substitution, Suggestion } from "../../models";
import { ExercisesStore, LocationsStore } from "../../stores/catalog";
import { ExerciseSheet } from "../library/exercise-sheet";
import { LogSheet, type LogSheetData } from "../log/log-sheet";

@Component({
	selector: "app-today",
	templateUrl: "./today.html",
	styleUrl: "./today.scss",
	imports: [
		MatButtonModule,
		MatCardModule,
		MatIconModule,
		MatMenuModule,
		MatProgressBarModule,
		RouterLink,
	],
})
export class Today {
	private api = inject(CoachApi);
	private sheet = inject(MatBottomSheet);
	private exercisesStore = inject(ExercisesStore);
	private locationsStore = inject(LocationsStore);

	readonly pacing = signal<PacingNow | null>(null);
	// Shared catalogs, retained across tab switches (see CachedResource).
	readonly exercises = computed(() => this.exercisesStore.value() ?? []);
	readonly locations = computed(() => this.locationsStore.value() ?? []);
	readonly loading = signal(true);
	private didInit = false;

	// The location whose kit bounds the session. Initialised to the default, then
	// upgraded to the auto-detected one (best-effort) unless the user has picked.
	// `null` only while locations are loading, or if there are none at all — the
	// engine then declines to plan rather than guessing what's doable.
	readonly selectedLocationId = signal<number | null>(null);
	readonly autoDetected = signal(false);
	private userPickedLocation = false;

	constructor() {
		this.loadAll();
		// The first pacing verdict needs the locations list (to pick the default
		// location). Wait for it, then initialise once. Retained catalogs make this
		// instant on a revisit; a cold load waits for the fetch. (Stores set
		// `loaded` even on failure, so this still fires and clears `loading`.)
		effect(() => {
			if (this.didInit || !this.locationsStore.loaded()) return;
			this.didInit = true;
			// A location picked by hand holds for the rest of the day — a reload
			// must not silently revert to the detected one and change the plan's
			// loads under the athlete mid-session.
			const picked = this.pickedToday();
			if (picked !== null && this.locations().some((l) => l.id === picked)) {
				this.userPickedLocation = true;
				this.selectedLocationId.set(picked);
				this.reloadPacing();
				return;
			}
			const def = this.locations().find((l) => l.isDefault);
			this.selectedLocationId.set(def ? def.id : null);
			this.reloadPacing();
			this.autoSelect();
		});
	}

	/** Today's manual location pick, if one was made (per-day, localStorage). */
	private pickedToday(): number | null {
		try {
			const raw = localStorage.getItem("coach.pickedLocation");
			if (!raw) return null;
			const { id, day } = JSON.parse(raw) as { id: number; day: string };
			return day === new Date().toDateString() ? id : null;
		} catch {
			return null;
		}
	}

	private rememberPick(id: number): void {
		try {
			localStorage.setItem(
				"coach.pickedLocation",
				JSON.stringify({ id, day: new Date().toDateString() }),
			);
		} catch {
			// Storage unavailable (private mode) — the pick just won't survive a reload.
		}
	}

	loadAll(): void {
		this.loading.set(true);
		this.exercisesStore.refresh();
		this.locationsStore.refresh();
	}

	/** Best-effort: switch to the auto-detected current location once it resolves. */
	private autoSelect(): void {
		this.api.locationCurrent().subscribe({
			next: (cur) => {
				if (cur.locationId == null || this.userPickedLocation) return;
				this.autoDetected.set(true);
				if (cur.locationId !== this.selectedLocationId()) {
					this.selectedLocationId.set(cur.locationId);
					this.reloadPacing();
				}
			},
			error: () => {},
		});
	}

	reloadPacing(): void {
		this.api.pacingNow(this.selectedLocationId() ?? undefined).subscribe({
			next: (p) => {
				this.pacing.set(p);
				this.loading.set(false);
			},
			error: () => this.loading.set(false),
		});
	}

	onLocationChange(id: number): void {
		this.userPickedLocation = true;
		this.autoDetected.set(false);
		this.selectedLocationId.set(id);
		this.rememberPick(id);
		this.reloadPacing();
	}

	/** Display name of the selected location for the status line. */
	locationName(): string {
		const id = this.selectedLocationId();
		const name = id == null ? undefined : this.locations().find((l) => l.id === id)?.name;
		return name ?? "No location";
	}

	// Which plan items have their "why this?" reasoning expanded (by exercise id).
	private readonly whyOpen = signal<ReadonlySet<number>>(new Set());
	isWhyOpen(id: number): boolean {
		return this.whyOpen().has(id);
	}
	toggleWhy(id: number): void {
		const next = new Set(this.whyOpen());
		if (next.has(id)) next.delete(id);
		else next.add(id);
		this.whyOpen.set(next);
	}

	/**
	 * Human-readable "why this?" lines from a suggestion's structured trace — the
	 * factors the engine actually weighed (deficit, recovery, ability, readiness).
	 */
	explanationLines(e: Explanation): string[] {
		const lines: string[] = [];
		if (e.confirming) {
			// Its muscles are already covered this week — it's here to turn a shaky
			// first estimate into a trusted one, which is worth more right now than a
			// brand-new movement. Say that; a near-zero deficit line would just read
			// as "why is this even here?".
			lines.push("Locking in your baseline — a couple more clean sessions and I'll trust this number");
		} else {
			const conf: Record<string, string> = {
				high: "You've trained this recently — confident estimate",
				medium: "A little recent data — estimate firming up",
				low: "Rusty here — working off older data",
				none: "New to you — calibrating from scratch",
			};
			lines.push(conf[e.confidence]);
			// Plain speech, not maths-speak: "100% below target" reads like an
			// error; what it means is the group hasn't been trained this week.
			lines.push(
				e.deficit >= 0.995
					? "Untrained this week — the whole target is still to come"
					: `${Math.round(e.deficit * 100)}% of this week's target still to go`,
			);
		}
		lines.push(
			e.recovery >= 0.99 ? "Fully recovered" : `${Math.round(e.recovery * 100)}% recovered`,
		);
		if (e.e1rm !== null) lines.push(`Estimated 1-rep max ≈ ${Math.round(e.e1rm)} kg`);
		// A run of missed sessions is why the number eased off — say so, or "lighter
		// than last week" reads as the coach forgetting rather than listening.
		if (e.misses === 1) lines.push("Last session came up short — holding here rather than adding");
		else if (e.misses >= 2)
			lines.push(`${e.misses} sessions under target — backed the load off to rebuild`);
		if (e.readiness) {
			// States the reading; never urges intensity ("push") — the athlete
			// decides how hard, same rule as the headline (see engine day_note).
			const r: Record<Band, string> = {
				high: "Biometrics say recovered",
				normal: "Steady readiness",
				low: "Low readiness — easing the volume off",
			};
			lines.push(r[e.readiness]);
		}
		return lines;
	}

	/** One-line description for a warm-up item: a ramp-in set vs a mobility
	 *  drill — with its dose, so "warm up" is an instruction, not a vibe. */
	warmupNote(s: Suggestion): string {
		if (s.loadKg !== null) {
			const reps = s.repLow !== null ? `${s.repLow} × ` : "";
			return `Ramp-in set · ${reps}${s.loadKg} kg — groove the movement`;
		}
		if (s.holdS !== null) return `${s.holdS}s ${this.perSide(s.exerciseId) ? "each side " : ""}— easy, controlled`;
		if (s.repLow !== null)
			return `${s.repLow} slow reps${this.perSide(s.exerciseId) ? " each side" : ""} — loosen up ${s.group}`;
		return "Mobility — loosen up the muscles you're about to train";
	}

	/**
	 * The calibration instruction for an `assess` suggestion — what to actually do
	 * so the logged set measures your ability. Metric comes from the catalog (the
	 * wire suggestion doesn't carry it), so assess-reps and assess-hold differ.
	 */
	assessInstruction(exerciseId: number, repLow: number | null): string {
		const ex = this.exercises().find((e) => e.id === exerciseId);
		const side = ex?.unilateral ? " Both sides — the numbers are per side." : "";
		const metric = ex?.metric;
		if (metric === "hold")
			return `Hold as long as your form stays clean — one honest max.${side}`;
		if (metric === "weighted_hold")
			return `Carry it as far as your form stays clean, then log the weight and the seconds — both are the measurement.${side}`;
		// What happened, not how it felt: the instruction asks for the load and the
		// reps, never for a self-rating out of ten. See docs/trainer.md.
		if (metric === "weighted_reps")
			return `Build up to a hard-but-clean set of ${repLow ?? 5}, then log the load and the reps.${side}`;
		return `As many clean reps as you can — stop at form breakdown, then log it.${side}`;
	}

	imageUrl(id: number): string {
		return this.api.exerciseImageUrl(id);
	}
	/** A movement can be catalogued before anyone has found a picture of it. Asking
	 *  for the image anyway renders a broken-image glyph on the plan card, which
	 *  reads as a bug rather than as "not photographed yet". */
	hasImage(id: number): boolean {
		return this.exercises().find((e) => e.id === id)?.hasImage ?? false;
	}

	/**
	 * A single-arm movement's numbers are **per side**: one set is both arms, and
	 * "10 reps" means ten with each. That's the convention the log follows, so it's
	 * the convention the prescription has to state — "3 × 10" on a suitcase carry is
	 * otherwise half a session or a double one, depending on how you read it, and
	 * the athlete is the one holding the kettlebell.
	 */
	perSide(id: number): boolean {
		return this.exercises().find((e) => e.id === id)?.unilateral ?? false;
	}

	/** What the coach would have given you, and what stopped it — naming the kit, so
	 *  the swap is something you can fix rather than a shrug. The two blockers want
	 *  different actions: buy/bring the kit, or go and register its weights. */
	substitutionNote(sub: Substitution): string {
		const kit = sub.blocker.kit.join(", ");
		return sub.blocker.kind === "absent"
			? `Swapped in for ${sub.ideal} — no ${kit} here`
			: `Swapped in for ${sub.ideal} — no weights registered for ${kit}`;
	}

	/**
	 * Show the movement in full — picture, muscles, demo video. The same sheet the
	 * Library opens: "what does this look like again?" is asked standing in the gym
	 * mid-warm-up, not while browsing the catalog, so it has to be reachable from
	 * the plan card itself.
	 */
	openDetail(s: Suggestion): void {
		this.sheet.open(ExerciseSheet, { data: { exerciseId: s.exerciseId } });
	}

	/** Open the log sheet, optionally prefilled from a specific plan item. The
	 *  bare + button prefills from the next unfinished plan item — mid-session
	 *  that's almost always the set being logged (and it's changeable), where an
	 *  alphabetical default meant scrolling past "Arm circles" every time. */
	openLog(from?: Suggestion): void {
		const source = from ?? this.nextUp() ?? undefined;
		const data: LogSheetData = {
			exercises: this.exercises(),
			// Each set refreshes the plan underneath; the sheet itself stays up
			// for the rest of the run (it never self-dismisses — see LogSheet).
			onLogged: () => this.reloadPacing(),
		};
		if (source) {
			data.prefill = {
				exerciseId: source.exerciseId,
				reps: source.repLow,
				loadKg: source.loadKg,
				holdS: source.holdS,
			};
		}
		this.sheet.open(LogSheet, { data });
	}

	/** The first plan item with sets still to do — what "Next up" points at and
	 *  what the bare + defaults to. Warm-ups count: done ones stop leading. */
	nextUp(): Suggestion | null {
		const p = this.pacing();
		return p?.plan.find((s) => s.done < s.sets) ?? null;
	}

	/** Whether this plan item is the one to do now (by position, not id — a
	 *  ramp-in warm-up shares its exercise id with the work item after it). */
	isNextUp(index: number): boolean {
		const p = this.pacing();
		if (!p?.withinWindow) return false;
		return p.plan.findIndex((s) => s.done < s.sets) === index;
	}
}
