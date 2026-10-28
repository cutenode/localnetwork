import { describe, expect, it } from 'vitest';
import { parseAnnouncement, parseEdit } from './announce';

const valid = {
	name: 'Tidal Hours',
	date: '2026-11-07',
	start: '23:00',
	end: '06:00',
	venue: 'Halle Nord',
	city: 'Berlin',
	region: 'Berlin',
	country: 'de',
	latE6: '52520000',
	lngE6: '13405000',
	tickets: 'https://tickets.example/tidal'
};

function form(entries: Record<string, string>): FormData {
	const data = new FormData();

	for (const [key, value] of Object.entries(entries)) data.append(key, value);

	return data;
}

describe('parseAnnouncement', () => {
	it('parses every field, uppercasing the country', () => {
		expect(parseAnnouncement(form({ ...valid, cancelled: 'on' }))).toEqual({
			name: 'Tidal Hours',
			date: '2026-11-07',
			start: '23:00',
			end: '06:00',
			venue: 'Halle Nord',
			location: { city: 'Berlin', region: 'Berlin', country: 'DE', latE6: 52_520_000, lngE6: 13_405_000 },
			tickets: 'https://tickets.example/tidal',
			cancelled: true
		});
	});

	it('leaves out what was left blank', () => {
		const { end, venue, region, tickets, ...required } = valid;
		expect([end, venue, region, tickets]).toHaveLength(4);
		expect(parseAnnouncement(form(required))).toEqual({
			name: 'Tidal Hours',
			date: '2026-11-07',
			start: '23:00',
			location: { city: 'Berlin', country: 'DE', latE6: 52_520_000, lngE6: 13_405_000 },
			cancelled: false
		});
	});

	it('says what to fix', () => {
		const fails = (overrides: Record<string, string>) => () => parseAnnouncement(form({ ...valid, ...overrides }));

		expect(fails({ name: ' ' })).toThrow('Give the show a name');
		expect(fails({ date: '' })).toThrow('Pick a date and start time');
		expect(fails({ start: '25:00' })).toThrow('Pick a date and start time');
		expect(fails({ end: '7pm' })).toThrow('Pick a valid end time');

		// The coordinates come with a city picked from the search, and they find the venue's clock.
		for (const missing of [
			['city', ''],
			['country', 'Germany'],
			['latE6', ''],
			['lngE6', '']
		])
			expect(fails(Object.fromEntries([missing]))).toThrow("Pick the venue's city from the search");

		expect(fails({ tickets: 'javascript:alert(1)' })).toThrow(/web address/);
		expect(fails({ tickets: 'tickets.example' })).toThrow(/web address/);
		expect(fails({ name: 'x'.repeat(121) })).toThrow('Show name can be at most 120 characters');
	});
});

describe('parseEdit', () => {
	const cid = 'bafyreie5737gdxlw5i64vzichcalba3z2v5n6icifvx5xytvske7mr3hpm';

	it('reads the versions the edit form showed, with no event as null', () => {
		expect(parseEdit(form({ showCid: cid, eventCid: cid }), 'k')).toEqual({ rkey: 'k', showCid: cid, eventCid: cid });
		expect(parseEdit(form({ showCid: cid, eventCid: '' }), 'k')).toEqual({ rkey: 'k', showCid: cid, eventCid: null });
	});

	it('refuses a form without them, or with anything but CIDs', () => {
		const forms: Record<string, string>[] = [
			{},
			{ showCid: '' },
			{ showCid: 'at://x' },
			{ showCid: cid, eventCid: 'x' }
		];

		for (const fields of forms)
			expect(() => parseEdit(form(fields), 'k')).toThrow('This form is out of date. Reload the show and try again.');
	});
});
