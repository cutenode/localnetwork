// src/lib/lookup-errors.ts — what Manager and artists say when a lookup stops short.
import { describe, it, expect } from 'vitest';
import { LOOKUP_ERRORS } from '../../src/lib/lookup-errors.ts';

describe('the lookup errors', () => {
	it('has words for what the content script says went wrong', () => {
		expect(LOOKUP_ERRORS['no-client-id']).toMatch(/API key/);
		expect(LOOKUP_ERRORS.unauthorized).toMatch(/rejected/);
		expect(LOOKUP_ERRORS['']).toBeUndefined();
	});
});
