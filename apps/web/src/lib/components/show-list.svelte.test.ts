import { page } from 'vitest/browser';
import { describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { EVENT_STATUS } from '#lib/shows.ts';
import { listedEvent } from '#lib/testing/listed-events.ts';
import { NEON_TIDE } from '#lib/testing/link-matrix.ts';
import type { ShowView } from '#lib/types.ts';
import ShowList from './ShowList.svelte';

const show: ShowView = {
	uri: 'at://did:plc:neon/computer.localnetwork.show/3s',
	cid: 'c',
	did: 'did:plc:neon',
	rkey: '3s',
	project: NEON_TIDE,
	record: {
		project: NEON_TIDE.uri,
		event: { uri: 'at://did:plc:neon/community.lexicon.calendar.event/3s', cid: 'e' },
		name: 'Tidal Hours',
		startsAt: '2026-12-04T23:00:00+01:00',
		venue: 'Halle Nord',
		location: { city: 'Berlin', country: 'DE' },
		status: EVENT_STATUS.scheduled,
		createdAt: '2026-10-01T00:00:00Z'
	}
};

const empty = { title: 'Nothing on' };

describe('ShowList', () => {
	it("lists a party's event beside a show, linking its page, party and venue, with the venue's standing", async () => {
		const events = [
			listedEvent('3mwinter', { tickets: 'https://tickets.example/winter', distanceKm: 1.5 }),
			listedEvent('3mnew', { name: 'New Year', confirmed: true, status: EVENT_STATUS.rescheduled })
		];

		await render(ShowList, { shows: [show, ...events], empty });

		const row = (name: string) => page.getByRole('listitem').filter({ hasText: name });
		const winter = row('Winter Opening');
		await expect
			.element(winter.getByRole('link', { name: 'Winter Opening' }))
			.toHaveAttribute('href', '/party/3lzparty/3mwinter/winter-opening');
		await expect
			.element(winter.getByRole('link', { name: 'Night Shift' }))
			.toHaveAttribute('href', '/party/3lzparty/night-shift');
		await expect.element(winter.getByRole('link', { name: 'Tresor' })).toHaveAttribute('href', '/venue/3tresor/tresor');
		await expect
			.element(winter)
			.toMatchTextContent(/Waiting for the venue.*23:00 to 06:00\s*·\s*Tresor, Berlin\s*·\s*1\.5 km\s*Tickets/);
		await expect.element(row('New Year')).toMatchTextContent(/New Year\s*New date\s*Confirmed/);
		// A show names its project and venue as before, with no link of its own and no standing.
		await expect
			.element(row('Tidal Hours'))
			.toMatchTextContent(/Tidal Hours\s*Neon Tide\s*23:00\s*·\s*Halle Nord, Berlin$/);
		expect(row('Tidal Hours').getByRole('link', { name: 'Tidal Hours' }).elements()).toHaveLength(0);
	});

	it('marks a cancelled event Cancelled, without its standing or tickets', async () => {
		await render(ShowList, {
			shows: [listedEvent('3mgone', { status: EVENT_STATUS.cancelled, tickets: 'https://tickets.example/x' })],
			empty
		});

		await expect.element(page.getByText('Cancelled')).toBeInTheDocument();
		expect(page.getByText('Waiting for the venue').elements()).toHaveLength(0);
		expect(page.getByRole('link', { name: 'Tickets' }).elements()).toHaveLength(0);
	});
});
