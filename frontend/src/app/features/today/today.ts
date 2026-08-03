import { Component, computed, effect, inject, signal } from "@angular/core";
import { MatBottomSheet } from "@angular/material/bottom-sheet";
import { MatButtonModule } from "@angular/material/button";
import { MatCardModule } from "@angular/material/card";
import { MatIconModule } from "@angular/material/icon";
import { MatMenuModule } from "@angular/material/menu";
import { MatProgressBarModule } from "@angular/material/progress-bar";
import { RouterLink } from "@angular/router";
import { CoachApi } from "../../coach-api";
import type {
	Band,
	Confidence,
	EstimateSource,
	Explanation,
	PacingNow,
	Substitution,
	Suggestion,
} from "../../models";
import { askDistanceM, askHoldS, askLoadKg, askRepHigh, askRepLow } from "../../shared/ask";
import { numberField, stringField } from "../../shared/narrow";
import { ExercisesStore, LocationsStore } from "../../stores/catalog";
import { ExerciseSheet } from "../library/exercise-sheet";
import { LogSheet, type LogPrefill, type LogSheetData } from "../log/log-sheet";

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

	/** Today's manual location pick, if one was made (per-day, localStorage).
	 *
	 *  The stored blob is checked, not asserted: it is written by whichever
	 *  version of this app the device last ran, and an `id` that isn't a number
	 *  would be handed on as a location id and silently select nothing. A blob
	 *  that doesn't hold up reads as "no pick made", which is the same answer a
	 *  missing key gives. */
	private pickedToday(): number | null {
		try {
			const raw = localStorage.getItem("coach.pickedLocation");
			if (!raw) return null;
			const pick: unknown = JSON.parse(raw);
			const id = numberField(pick, "id");
			const day = stringField(pick, "day");
			if (id === null || day === null) return null;
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

	/** The set whose removal is awaiting confirmation, if any. Removing a set is
	 *  destructive, so it takes a second deliberate tap — but it stays inline, so
	 *  correcting a number is still a two-tap job rather than a hunt through
	 *  history for a set logged weeks ago. */
	readonly confirmRemoveSetId = signal<number | null>(null);

	/** The opening weight an assessment names, for the template. */
	readonly askLoadKg = askLoadKg;

	/** The set behind the estimate, in the terms it was logged in. */
	describeSource(src: EstimateSource): string {
		const when = new Date(src.loggedAt + "Z").toLocaleDateString(undefined, {
			day: "numeric",
			month: "short",
		});
		const bits: string[] = [];
		if (src.loadKg !== null) bits.push(`${src.loadKg} kg`);
		if (src.reps !== null) bits.push(`× ${src.reps}`);
		if (src.holdS !== null) bits.push(`${src.holdS}s`);
		return `${when} · ${bits.join(" ")}`;
	}

	/** Drop the set the estimate rests on. The next verdict re-derives from what
	 *  is left — nothing is patched by hand, so the engine stays the only thing
	 *  that computes a number. */
	removeSourceSet(setId: number): void {
		this.confirmRemoveSetId.set(null);
		this.api.deleteSet(setId).subscribe({
			next: () => this.reloadPacing(),
			error: () => this.reloadPacing(),
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
	readonly locationName = computed(() => {
		const id = this.selectedLocationId();
		const name = id == null ? undefined : this.locations().find((l) => l.id === id)?.name;
		return name ?? "No location";
	});

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
			// Keyed by `Confidence`, not `string`: the lookup below is then total by
			// construction, and adding a variant on the Rust side fails to compile
			// here rather than silently pushing an `undefined` line into the UI.
			const conf: Record<Confidence, string> = {
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

	/**
	 * Whether a plan item renders as a one-line row rather than a full card.
	 *
	 * A card earns its height by holding something still to decide: the dose, the
	 * reasoning, the picture to check your form against. Two kinds hold none of
	 * that. A **finished** item is a receipt — it stays on the plan (it's the
	 * commitment's record) but the decision is spent. A **warm-up** is one line of
	 * prep, not a prescription; its whole content is "10 arm circles".
	 *
	 * Rendered full, they crowded the work off the screen: measured at Pixel 7
	 * width, every card was 166px regardless of state, so three finished warm-ups
	 * filled 498px of a ~780px viewport and the first work card — the reason the
	 * page exists — sat below the fold for the whole session.
	 */
	isCompact(s: Suggestion): boolean {
		return s.logged.length >= s.sets || s.kind === "warmup";
	}

	/**
	 * What you actually did, in the order you did it.
	 *
	 * The count ("1 / 2 sets") was standing in for this, and answered the wrong
	 * question: on set two you want to know what set one was, and that lived only
	 * in History. Reps alone read better with the unit said once at the end
	 * ("9 · 6 reps"); anything carrying a load or a clock names its own
	 * ("22.5 kg × 7 · 24 kg × 6").
	 */
	loggedSummary(s: Suggestion): string {
		if (!s.logged.length) return "";
		const bits = s.logged.map((d) => {
			const parts: string[] = [];
			if (d.loadKg !== null) parts.push(`${d.loadKg} kg`);
			if (d.reps !== null) parts.push(d.loadKg !== null ? `× ${d.reps}` : `${d.reps}`);
			if (d.holdS !== null) parts.push(`${d.holdS}s`);
			return parts.join(" ");
		});
		const repsOnly = s.logged.every((d) => d.loadKg === null && d.holdS === null);
		const per = this.perSide(s.exerciseId) ? " each side" : "";
		return `${bits.join(" · ")}${repsOnly ? " reps" : ""}${per}`;
	}

	/** The dose on a compact row: short enough for one line beside the name.
	 *  Only ever the *ask* — once there's something logged, `loggedSummary` says
	 *  what happened instead, which is the more useful of the two. */
	compactDose(s: Suggestion): string {
		const bits: string[] = [];
		const repLow = askRepLow(s.ask);
		if (repLow !== null) {
			// A warm-up's range is a single number; only a work item aims.
			bits.push(
				s.kind === "warmup" || repLow === askRepHigh(s.ask)
					? `${repLow} reps`
					: `aim ${repLow}`,
			);
		}
		const loadKg = askLoadKg(s.ask);
		const holdS = askHoldS(s.ask);
		const distanceM = askDistanceM(s.ask);
		if (loadKg !== null) bits.push(`${loadKg} kg`);
		if (holdS !== null) bits.push(`${holdS}s`);
		if (distanceM !== null) bits.push(`${distanceM} m`);
		if (bits.length && this.perSide(s.exerciseId)) bits.push("each side");
		// A loaded warm-up is a ramp-in on the movement itself, not a mobility
		// drill — that changes what you do with it, so it survives the shortening.
		if (s.kind === "warmup" && loadKg !== null) bits.unshift("Ramp-in");
		const dose = bits.join(" · ");
		const done = s.logged.length;
		if (done >= s.sets) {
			const sets = `${done} set${done === 1 ? "" : "s"}`;
			return dose ? `${sets} · ${dose}` : sets;
		}
		return dose || "Mobility";
	}

	/**
	 * The calibration instruction for an `assess` suggestion — what to actually do
	 * so the logged set measures your ability.
	 *
	 * This used to infer the instruction from the exercise's *metric* in the
	 * catalog, because the wire suggestion didn't say which calibration had been
	 * asked for; the rep count came with a `?? 5` for the case where the fields
	 * didn't line up. The ask names the calibration, so both the lookup and the
	 * invented default are gone.
	 */
	assessInstruction(s: Suggestion): string {
		const ex = this.exercises().find((e) => e.id === s.exerciseId);
		const side = ex?.unilateral ? " Both sides — the numbers are per side." : "";
		switch (s.ask.kind) {
			case "maxHold":
				return `Hold as long as your form stays clean — one honest max, then log the seconds.${side}`;
			case "loadedCarry":
				return `Carry it as far as your form stays clean, then log the weight and the seconds — both are the measurement.${side}`;
			case "loadedDistance":
				return `Carry it as far as your form stays clean, then log the weight and the distance — both are the measurement.${side}`;
			// What happened, not how it felt: the instruction asks for the load and
			// the reps, never for a self-rating out of ten. See docs/trainer.md.
			case "buildUp":
				return `Build up to a hard-but-clean set of ${s.ask.reps}, then log the load and the reps.${side}`;
			default:
				return `As many clean reps as you can — stop at form breakdown, then log it.${side}`;
		}
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

	/** One prefill per planned exercise — its next-undone item's numbers (the
	 *  ramp-in before the work sets, the work sets after) — so switching the
	 *  sheet to a planned movement lands on its prescription instead of
	 *  whatever the last movement's fields held. */
	private planPrefills(): LogPrefill[] {
		const by = new Map<number, Suggestion>();
		for (const s of this.pacing()?.plan ?? []) {
			const cur = by.get(s.exerciseId);
			if (!cur || (cur.logged.length >= cur.sets && s.logged.length < s.sets))
				by.set(s.exerciseId, s);
		}
		return [...by.values()].map((s) => ({
			exerciseId: s.exerciseId,
			reps: askRepLow(s.ask),
			loadKg: askLoadKg(s.ask),
			holdS: askHoldS(s.ask),
			distanceM: askDistanceM(s.ask),
		}));
	}

	/** Open the log sheet, optionally prefilled from a specific plan item. The
	 *  bare + button prefills from the next unfinished plan item — mid-session
	 *  that's almost always the set being logged (and it's changeable), where an
	 *  alphabetical default meant scrolling past "Arm circles" every time. */
	openLog(from?: Suggestion): void {
		const source = from ?? this.nextUp() ?? undefined;
		const data: LogSheetData = {
			exercises: this.exercises(),
			planPrefills: this.planPrefills(),
			// Each set refreshes the plan underneath; the sheet itself stays up
			// for the rest of the run (it never self-dismisses — see LogSheet).
			onLogged: () => this.reloadPacing(),
		};
		if (source) {
			data.prefill = {
				exerciseId: source.exerciseId,
				reps: askRepLow(source.ask),
				loadKg: askLoadKg(source.ask),
				holdS: askHoldS(source.ask),
				distanceM: askDistanceM(source.ask),
			};
		}
		this.sheet.open(LogSheet, { data });
	}

	/** The header's arithmetic is the plan's own — summed from the cards, never
	 *  the engine's day-size estimate, which once said 13 while the cards held 16
	 *  sets so finishing them all read "14 / 13" (field-test R2-2).
	 *
	 *  Work sets only. Warm-ups credit no volume and count toward nothing the
	 *  coach scores, so including them let the page report "3 / 10 done" on a day
	 *  the engine read as untrained — the counter said a third of a session had
	 *  happened when none of it had. They still show as their own checked-off
	 *  rows; they just aren't the session's measure. */
	private work(p: PacingNow): Suggestion[] {
		return p.plan.filter((s) => s.kind !== "warmup");
	}
	planSets(p: PacingNow): number {
		return this.work(p).reduce((a, s) => a + s.sets, 0);
	}
	planDone(p: PacingNow): number {
		return this.work(p).reduce((a, s) => a + s.logged.length, 0);
	}

	/** The first plan item with sets still to do — what "Next up" points at and
	 *  what the bare + defaults to. Warm-ups count: done ones stop leading. */
	nextUp(): Suggestion | null {
		const p = this.pacing();
		return p?.plan.find((s) => s.logged.length < s.sets) ?? null;
	}

	/** Whether this plan item is the one to do now (by position, not id — a
	 *  ramp-in warm-up shares its exercise id with the work item after it). */
	isNextUp(index: number): boolean {
		const p = this.pacing();
		if (p?.window !== "within") return false;
		return p.plan.findIndex((s) => s.logged.length < s.sets) === index;
	}
}
