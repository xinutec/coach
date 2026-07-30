/**
 * Reading an {@link Ask} — the tagged prescription the verdict carries.
 *
 * The card used to receive `repLow`, `repHigh`, `loadKg` and `holdS` as four
 * independent nullable fields and rebuild the prescription from them by null
 * testing, which is how `assessInstruction` ended up with a `repLow ?? 5`: a
 * fallback for a case the engine cannot produce, invented because the type
 * could not rule it out. The union rules it out.
 *
 * These helpers are *derived* from the variant rather than stored beside it, so
 * unlike the fields they replace they cannot contradict the rest of the ask —
 * there is no weighted lift here that has lost its load. Prefer switching on
 * `ask.kind` where the display genuinely differs per variant; reach for these
 * only when a single number is wanted regardless of shape, as when prefilling
 * the log sheet's fields.
 */
import type { Ask } from "../models";

/** The weight this ask names, if it names one. */
export function askLoadKg(ask: Ask): number | null {
	switch (ask.kind) {
		case "weighted":
		case "weightedHold":
			return ask.loadKg;
		case "buildUp":
		case "loadedCarry":
			return ask.startKg;
		default:
			return null;
	}
}

/** The number of reps actually asked for, if this ask is counted in reps. */
export function askRepLow(ask: Ask): number | null {
	switch (ask.kind) {
		case "weighted":
		case "bodyweight":
			return ask.repLow;
		case "buildUp":
			return ask.reps;
		default:
			return null;
	}
}

/** The top of the rep range. A calibration build-up is a single number, so its
 *  low and high are the same — the card then reads "8 reps", not "aim 8". */
export function askRepHigh(ask: Ask): number | null {
	switch (ask.kind) {
		case "weighted":
		case "bodyweight":
			return ask.repHigh;
		case "buildUp":
			return ask.reps;
		default:
			return null;
	}
}

/** The seconds this ask names, if it is timed. */
export function askHoldS(ask: Ask): number | null {
	switch (ask.kind) {
		case "hold":
		case "weightedHold":
			return ask.holdS;
		default:
			return null;
	}
}
