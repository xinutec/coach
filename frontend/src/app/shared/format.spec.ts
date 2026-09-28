import { describe, expect, it } from 'vitest';

import { capitalise, repUnit } from './format';

describe('capitalise', () => {
  it('upper-cases the first letter and keeps the rest', () => {
    expect(capitalise('shoulders')).toBe('Shoulders');
    expect(capitalise('')).toBe('');
  });
});

describe('repUnit', () => {
  it('says one rep, and reps for every other count or a range', () => {
    expect(repUnit(1)).toBe('rep');
    expect(repUnit(3)).toBe('reps');
    expect(repUnit(0)).toBe('reps');
    expect(repUnit('4–6')).toBe('reps');
  });
});
