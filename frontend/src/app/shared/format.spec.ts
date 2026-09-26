import { describe, expect, it } from 'vitest';

import { capitalise } from './format';

describe('capitalise', () => {
  it('upper-cases the first letter and keeps the rest', () => {
    expect(capitalise('shoulders')).toBe('Shoulders');
    expect(capitalise('')).toBe('');
  });
});
