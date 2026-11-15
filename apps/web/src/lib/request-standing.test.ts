import { describe, expect, it } from 'vitest';
import { dayLabel, requestStanding } from './request-standing.ts';

describe('dayLabel', () => {
	const now = Date.parse('2026-11-06T12:00:00Z');

	it('leaves the year off this year and says it otherwise', () => {
		expect(dayLabel('2026-11-03T12:00:00Z', now)).toBe('3 Nov');
		expect(dayLabel('2025-11-03T12:00:00Z', now)).toBe('3 Nov 2025');
	});
});

describe('requestStanding', () => {
	const now = Date.parse('2026-11-06T12:00:00Z');
	const decision = { at: '2026-11-04T12:00:00Z', by: null, reason: null };
	const updatedAt = '2026-11-03T12:00:00Z';

	it('says who has it and since when, the last time it changed hands', () => {
		expect(requestStanding({ status: 'pending', updatedAt, decision: null }, now)).toBe(
			'With staff since 3 Nov. Nothing needed from you.'
		);
		// Revised after staff asked: it's theirs again from the revision, not the old decision.
		expect(requestStanding({ status: 'pending', updatedAt, decision }, now)).toBe(
			'With staff since 3 Nov. Nothing needed from you.'
		);
	});

	it('says what became of it once staff answered or it was withdrawn', () => {
		expect(requestStanding({ status: 'changes_requested', updatedAt, decision }, now)).toBe(
			'Staff asked for changes on 4 Nov.'
		);
		expect(requestStanding({ status: 'approved', updatedAt, decision }, now)).toBe('Approved on 4 Nov.');
		expect(requestStanding({ status: 'rejected', updatedAt, decision }, now)).toBe('Turned down on 4 Nov.');
		expect(requestStanding({ status: 'withdrawn', updatedAt, decision: null }, now)).toBe('Withdrawn on 3 Nov.');
	});
});
