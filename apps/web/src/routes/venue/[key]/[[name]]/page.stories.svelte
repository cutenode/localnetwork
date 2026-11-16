<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { expect, within } from 'storybook/test';
	import { EVENT_STATUS } from '#lib/shows.ts';
	import { listedEvent } from '#lib/testing/listed-events.ts';
	import { PAGE_META } from '#lib/testing/page-meta.ts';
	import VenuePage from './+page.svelte';
	import type { PageData } from './$types';

	/**
	 * A venue's page: where it is, and its upcoming events, waiting for the venue until it confirms them. With events and
	 * dashboard links, with none, and without coordinates, each at 1280px in the dark palette and at 390px in the light
	 * one.
	 */
	const { Story } = defineMeta({
		title: 'Pages/Venue',
		parameters: {
			layout: 'fullscreen',
			viewport: {
				options: {
					phone: { name: 'Phone', styles: { width: '390px', height: '844px' }, type: 'mobile' },
					desktop: { name: 'Desktop', styles: { width: '1280px', height: '800px' }, type: 'desktop' }
				}
			}
		}
	});

	const desktop = { viewport: { value: 'desktop', isRotated: false } };

	const phone = { viewport: { value: 'phone', isRotated: false } };

	const light = () => {
		document.documentElement.classList.add('light');

		return () => document.documentElement.classList.remove('light');
	};

	const venue: PageData['venue'] = {
		name: 'Tresor',
		address: { country: 'DE', locality: 'Berlin', street: 'Köpenicker Str. 70' },
		geo: { latitude: '52.51', longitude: '13.42' }
	};

	const events = [
		listedEvent('3mwinter'),
		listedEvent('3mlong', { name: 'Closing Party With a Very Long Name That Runs On Past the Edge of the Card', startsAt: '2026-12-12T22:00:00+01:00', endsAt: undefined, confirmed: true }),
		listedEvent('3mnew', { name: 'New Year', startsAt: '2026-12-31T23:00:00+01:00', status: EVENT_STATUS.rescheduled, confirmed: true }),
		listedEvent('3mgone', { name: 'Called Off', startsAt: '2027-01-09T23:00:00+01:00', status: EVENT_STATUS.cancelled })
	];

	const dashboard = [
		{ label: 'Events to confirm', href: '/dashboard/venues/3tresor/events' },
		{ label: 'Settings', href: '/dashboard/venues/3tresor/settings' }
	];

	const base = { viewer: null, pendingRequests: 0, meta: PAGE_META, venue, events, dashboard: [] };

	const params = { key: '3tresor', name: 'tresor' };

	const shows = (text: string) => async ({ canvasElement }: { canvasElement: HTMLElement }) => {
		await expect(within(canvasElement).getAllByText(text)[0]).toBeInTheDocument();
	};
</script>

<Story name="With events" asChild globals={desktop} play={shows('Waiting for the venue')}><VenuePage data={{ ...base, dashboard }} {params} form={null} /></Story>

<Story name="With events, phone, light" asChild globals={phone} beforeEach={light} play={shows('Waiting for the venue')}><VenuePage data={{ ...base, dashboard }} {params} form={null} /></Story>

<Story name="No events" asChild globals={desktop} play={shows('No upcoming events here yet.')}><VenuePage data={{ ...base, events: [] }} {params} form={null} /></Story>

<Story name="No events, phone, light" asChild globals={phone} beforeEach={light} play={shows('No upcoming events here yet.')}><VenuePage data={{ ...base, events: [] }} {params} form={null} /></Story>

<Story name="No coordinates" asChild globals={desktop} play={shows('Köpenicker Str. 70, Berlin, DE')}><VenuePage data={{ ...base, venue: { ...venue, geo: null } }} {params} form={null} /></Story>

<Story name="No coordinates, phone, light" asChild globals={phone} beforeEach={light} play={shows('Köpenicker Str. 70, Berlin, DE')}><VenuePage data={{ ...base, venue: { ...venue, geo: null } }} {params} form={null} /></Story>
