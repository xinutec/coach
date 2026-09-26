/** A lower-case catalog word as a label: `push` → `Push`. The value itself is
 *  left alone; only what is shown changes. */
export function capitalise(word: string): string {
  return word.charAt(0).toUpperCase() + word.slice(1);
}
