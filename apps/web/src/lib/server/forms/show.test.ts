import { afterEach, describe, expect, it, vi } from 'vitest';
import { parseShow, showDate } from './show';

function form(entries: Record<string, string>): FormData {
	const data = new FormData();

	for (const [key, value] of Object.entries(entries)) data.append(key, value);

	return data;
}

describe('showDate', () => {
	afterEach(() => vi.unstubAllEnvs());

	it('keeps the wall-clock time as entered, whatever time zone the server is in', () => {
		vi.stubEnv('TZ', 'America/Los_Angeles');
		expect(showDate('2026-12-12', '23:30')).toBe('2026-12-12T23:30:00.000Z');
		expect(showDate('2026-12-12')).toBe('2026-12-12T20:00:00.000Z');
	});

	it('refuses dates and times that do not exist', () => {
		expect(() => showDate('2026-02-30')).toThrow(/valid date/);
		expect(() => showDate('2026-13-01')).toThrow(/valid date/);
		expect(() => showDate('2026-12-12', '25:00')).toThrow(/valid date/);
	});
});

describe('parseShow', () => {
	it('parses the inquiry and planner fields', () => {
		expect(
			parseShow(
				form({
					title: 'Basement Sessions',
					date: '2026-12-12',
					time: '23:30',
					venue: 'The Lot',
					capacity: '200',
					budgetAmount: '500',
					currency: 'eur',
					budgetTier: 'emerging',
					setType: 'peak',
					setMinutes: '90',
					genres: 'techno, house',
					notes: 'Bring records'
				})
			)
		).toEqual({
			title: 'Basement Sessions',
			date: '2026-12-12T23:30:00.000Z',
			venue: 'The Lot',
			capacity: 200,
			budget: { amount: 500, currency: 'EUR' },
			budgetTier: 'emerging',
			setType: 'peak',
			setMinutes: 90,
			genres: ['techno', 'house'],
			notes: 'Bring records'
		});
		expect(parseShow(form({ title: 'Plain', currency: 'USD' }))).toEqual({ title: 'Plain' });
	});

	it('refuses a budget it would have to drop', () => {
		expect(() => parseShow(form({ title: 'x', budgetAmount: '500', currency: 'EU' }))).toThrow(/three-letter currency/);
		expect(() => parseShow(form({ title: 'x', budgetAmount: '500', currency: '' }))).toThrow(/currency for the budget/);
	});

	it('checks the lexicon length caps', () => {
		expect(() => parseShow(form({ title: '' }))).toThrow(/Give the show a name/);
		expect(() => parseShow(form({ title: 'x'.repeat(121) }))).toThrow(/Show name can be at most 120/);
		expect(() => parseShow(form({ title: 'x', venue: 'x'.repeat(121) }))).toThrow(/Venue can be at most 120/);
		expect(() => parseShow(form({ title: 'x', notes: 'x'.repeat(2001) }))).toThrow(/Notes can be at most 2000/);
	});
});
