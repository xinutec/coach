/**
 * Narrowing an `unknown` that came from outside the app — `JSON.parse`,
 * `localStorage`, a native bridge's return value.
 *
 * The alternative is `JSON.parse(s) as Shape`, which does not check anything: it
 * tells the compiler what came back, and every read downstream is typed off that
 * claim. When the claim is wrong the failure surfaces far from the line that
 * made it — as `undefined` where the types promised a value, or as
 * "[object Object]" on the screen where they promised a string. Nothing in the
 * toolchain can catch that, because the assertion is the thing that lied to it.
 *
 * So: parse to `unknown`, come through here, and read fields with a real check.
 */

/** A value that can be indexed by string — i.e. worth asking about a field. */
export function isRecord(v: unknown): v is Record<string, unknown> {
	return typeof v === "object" && v !== null;
}

/** The named field, only if it really is a string. */
export function stringField(v: unknown, key: string): string | null {
	if (!isRecord(v)) return null;
	const value = v[key];
	return typeof value === "string" ? value : null;
}

/** The named field, only if it really is a number. */
export function numberField(v: unknown, key: string): number | null {
	if (!isRecord(v)) return null;
	const value = v[key];
	return typeof value === "number" ? value : null;
}
