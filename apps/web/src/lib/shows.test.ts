import { describe, expect, it } from 'vitest';
import {
	bookingValues,
	EVENT_STATUS,
	inquiryDefaults,
	inquiryValues,
	isCancelled,
	showFormDefaults,
	showHours
} from './shows';

describe('showHours', () => {
	it("reads the venue's clock off the times, whatever the offset", () => {
		expect(showHours({ startsAt: '2026-11-07T23:00:00+01:00', endsAt: '2026-11-08T06:00:00+01:00' })).toBe(
			'23:00 to 06:00'
		);
		expect(showHours({ startsAt: '2026-11-07T21:30:00-05:00' })).toBe('21:30');
	});
});

describe('isCancelled', () => {
	it('is true only for a cancelled event', () => {
		expect(isCancelled({ status: EVENT_STATUS.cancelled })).toBe(true);
		expect(isCancelled({ status: EVENT_STATUS.rescheduled })).toBe(false);
		expect(isCancelled({})).toBe(false);
	});
});

describe('bookingValues', () => {
	it("fills the announce form from a booking's show, whose date is the venue's clock stored as UTC", () => {
		const location = { city: 'Berlin', region: 'Berlin', country: 'DE', latE6: 52_520_000, lngE6: 13_405_000 };

		expect(
			bookingValues({ title: 'Basement Sessions', date: '2026-12-12T23:30:00.000Z', venue: 'The Lot', location })
		).toEqual({
			...showFormDefaults,
			name: 'Basement Sessions',
			date: '2026-12-12',
			start: '23:30',
			venue: 'The Lot',
			city: 'Berlin',
			region: 'Berlin',
			country: 'DE',
			latE6: '52520000',
			lngE6: '13405000'
		});
		expect(bookingValues({ title: 'Someday' })).toEqual({ ...showFormDefaults, name: 'Someday' });
	});
});

describe('inquiryValues', () => {
	it("fills the inquiry form from a planner show, whose date is the venue's clock stored as UTC", () => {
		expect(
			inquiryValues({
				title: 'Rooftop Rave',
				date: '2026-12-12T23:30:00.000Z',
				venue: 'The Lot',
				capacity: 300,
				budget: { amount: 800, currency: 'EUR' },
				budgetTier: 'established',
				setType: 'peak',
				setMinutes: 120,
				genres: ['techno', 'electro'],
				notes: 'Only for the planner'
			})
		).toEqual({
			title: 'Rooftop Rave',
			date: '2026-12-12',
			time: '23:30',
			setMinutes: '120',
			venue: 'The Lot',
			capacity: '300',
			budgetAmount: '800',
			currency: 'EUR',
			budgetTier: 'established',
			setType: 'peak',
			genres: 'techno, electro'
		});
		expect(inquiryValues({ title: 'Someday' })).toEqual({ ...inquiryDefaults, title: 'Someday' });
	});
});
