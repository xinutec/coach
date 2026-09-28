/** A lower-case catalog word as a label: `push` → `Push`. The value itself is
 *  left alone; only what is shown changes. */
export function capitalise(word: string): string {
  return word.charAt(0).toUpperCase() + word.slice(1);
}

/** The unit after a rep count: "1 rep", "3 reps", and "4–6 reps" for a range. */
export function repUnit(count: number | string): string {
  return count === 1 || count === '1' ? 'rep' : 'reps';
}
