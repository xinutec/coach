/**
 * Narrowing an `unknown` that came from outside the app — `JSON.parse`,
 * `localStorage`, a native bridge's return value.
 *
 * `JSON.parse(s) as Shape` checks nothing, and a wrong claim surfaces far away,
 * as `undefined` or "[object Object]" on screen. Parse to `unknown` and read
 * fields through these checks instead.
 */

/** A value that can be indexed by string — i.e. worth asking about a field. */
export function isRecord(v: unknown): v is Record<string, unknown> {
  return typeof v === 'object' && v !== null;
}

/** The named field, only if it really is a string. */
export function stringField(v: unknown, key: string): string | null {
  if (!isRecord(v)) return null;
  const value = v[key];
  return typeof value === 'string' ? value : null;
}

/** The named field, only if it really is a number. */
export function numberField(v: unknown, key: string): number | null {
  if (!isRecord(v)) return null;
  const value = v[key];
  return typeof value === 'number' ? value : null;
}
