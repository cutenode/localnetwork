import { page } from 'vitest/browser';
import { describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { EVENT_STATUS } from '#lib/shows.ts';
import { listedEvent } from '#lib/testing/listed-events.ts';
import { PAGE_META } from '#lib/testing/page-meta.ts';
import Page from './+page.svelte';
import type { PageData } from './$types';

const venue: PageData['venue'] = {
	name: 'Tresor',
	address: { country: 'DE', locality: 'Berlin', street: 'Köpenicker Str. 70' },
	geo: { latitude: '52.51', longitude: '13.42' }
};

const data = (overrides = {}) => ({
	viewer: null,
	pendingRequests: 0,
	meta: PAGE_META,
	venue,
	events: [],
	dashboard: [],
	...overrides
});

const params = { key: '3tresor', name: 'tresor' };

describe('venue page', () => {
	it('shows where the venue is, with a map link, and says when nothing is on', async () => {
		await render(Page, { data: data(), params, form: null });

		await expect.element(page.getByRole('heading', { level: 1, name: 'Tresor' })).toBeVisible();
		await expect.element(page.getByText('Köpenicker Str. 70, Berlin, DE')).toBeVisible();
		await expect
			.element(page.getByRole('link', { name: 'Map' }))
			.toHaveAttribute('href', 'https://www.openstreetmap.org/?mlat=52.51&mlon=13.42#map=17/52.51/13.42');
		await expect.element(page.getByText('No upcoming events here yet.')).toBeVisible();
		expect(page.getByRole('region', { name: 'Dashboard' }).elements()).toHaveLength(0);
	});

	it('lists its events with their pages and parties, marked waiting for the venue, Confirmed or with a new date', async () => {
		const events = [
			listedEvent('3mwinter'),
			listedEvent('3mnew', {
				name: 'New Year',
				confirmed: true,
				status: EVENT_STATUS.rescheduled,
				href: '/party/3lzparty/3mnew/new-year'
			}),
			listedEvent('3mgone', {
				name: 'Called Off',
				status: EVENT_STATUS.cancelled
			})
		];

		await render(Page, { data: data({ events }), params, form: null });

		const row = (name: string) => page.getByRole('listitem').filter({ hasText: name });
		await expect
			.element(row('Winter Opening').getByRole('link', { name: 'Winter Opening' }))
			.toHaveAttribute('href', '/party/3lzparty/3mwinter/winter-opening');
		await expect
			.element(row('Winter Opening').getByRole('link', { name: 'Night Shift' }))
			.toHaveAttribute('href', '/party/3lzparty/night-shift');
		await expect
			.element(row('Winter Opening'))
			.toMatchTextContent(/Waiting for the venue\s*Sat 5 Dec\s*23:00 to 06:00\s*·\s*Night Shift/);
		await expect.element(row('New Year')).toMatchTextContent(/New Year\s*New date\s*Confirmed/);
		await expect.element(row('Called Off')).toMatchTextContent(/Called Off\s*Cancelled\s*Sat 5 Dec/);
		// Every event here is at this venue, so no row names it.
		expect(page.getByRole('link', { name: 'Tresor' }).elements()).toHaveLength(0);
	});

	it('leaves the map link out without coordinates, and links the dashboard pages it is given', async () => {
		const dashboard = [{ label: 'Events to confirm', href: '/dashboard/venues/3tresor/events' }];
		await render(Page, { data: data({ venue: { ...venue, geo: null }, dashboard }), params, form: null });

		expect(page.getByRole('link', { name: 'Map' }).elements()).toHaveLength(0);
		await expect
			.element(page.getByRole('region', { name: 'Dashboard' }).getByRole('link', { name: 'Events to confirm' }))
			.toHaveAttribute('href', '/dashboard/venues/3tresor/events');
	});
});
