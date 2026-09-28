import { describe, expect, it } from 'vitest';

import { provideHttpClient } from '@angular/common/http';
import { TestBed } from '@angular/core/testing';

import { CoachApi } from './coach-api';

/** Pictures and loops are cached as immutable for a year, so the URL is the only
 *  thing that can bring a new render to a phone that has seen the old one. */
describe('media URLs', () => {
  it('give a new render a new URL', () => {
    TestBed.configureTestingModule({ providers: [provideHttpClient()] });
    const api = TestBed.inject(CoachApi);
    expect(api.exerciseLoopUrl(7, 'a')).not.toBe(api.exerciseLoopUrl(7, 'b'));
    expect(api.exerciseImageUrl(7, 'a')).not.toBe(api.exerciseImageUrl(7, 'b'));
  });
});
