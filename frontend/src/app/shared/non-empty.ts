/**
 * An array that has at least one element, so `[0]` is a value rather than a
 * maybe-value.
 *
 * Grouping code builds these constantly — a map keyed by day or by movement is
 * seeded with `[first]` and pushed to, so every list in it has a first element.
 * Saying that in the type is what lets the reader (and the compiler) index the
 * head without a guard that could never fire, and it makes the one place the
 * invariant is actually established — the seeding `[first]` — the only place
 * that has to be right.
 */
export type NonEmpty<T> = [T, ...T[]];
