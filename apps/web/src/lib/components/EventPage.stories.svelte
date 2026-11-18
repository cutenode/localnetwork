<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { expect, within } from 'storybook/test';
	import { capabilitiesFor } from '#lib/capabilities.ts';
	import type { EventPageView } from '#lib/events.ts';
	import { EVENT_STATUS } from '#lib/shows.ts';
	import { light } from '#lib/testing/light.ts';
	import { desktop, phone } from '#lib/testing/viewports.ts';
	import EventPage from './EventPage.svelte';

	/**
	 * An event's page, built in place: as a visitor sees it while it waits for the venue and once it's confirmed,
	 * cancelled, private with its link for its people, in edit mode, asking how its times move to another clock, and
	 * empty as it's made, on its own time zone. Each at 1280px in the dark
	 * palette and at 390px in the light one.
	 */
	const { Story } = defineMeta({
		title: 'Pages/Event',
		component: EventPage,
		parameters: { layout: 'fullscreen' }
	});

	const party = { name: 'Night Shift', href: '/party/3lzparty/night-shift' };

	/** Stands in for the app's navigation, its form sending and its leave guard, which a story doesn't have. */
	const harness = {
		url: new URL('https://localnetwork.test/party/3lzparty/3mwinter/winter-opening'),
		viewer: { did: 'did:plc:neon' },
		navigate: async () => {},
		guardNavigation: () => {},
		enhance: () => ({ destroy: () => {} }),
		version: 'bafyreiopened',
		party
	};

	const event: EventPageView = {
		name: 'Winter Opening',
		description: 'The back room opens for the winter. Residents all night, one guest from Leipzig.\nDoors at eleven, no list.',
		startsAt: '2026-12-05T23:00:00+01:00',
		endsAt: '2026-12-06T06:00:00+01:00',
		tickets: 'https://tickets.example/winter',
		status: EVENT_STATUS.scheduled,
		audience: 'public',
		venue: { space: 'at://did:plc:root/computer.localnetwork.space.venue/3tresor', skey: '3tresor', name: 'Tresor', locality: 'Berlin', country: 'DE' },
		timeZone: 'Europe/Berlin',
		confirmed: false,
		lineup: [
			{ name: 'Crew', href: null, value: '/act/3crew/crew', start: '23:00', end: '02:00' },
			{ name: '@neon.test', href: '/@neon.test', value: 'neon.test', start: '02:00', end: '04:00' },
			{ name: '@sam.test', href: '/@sam.test', value: 'sam.test', start: '04:00' }
		]
	};

	const owner = {
		...harness,
		subject: { kind: 'event' as const, uri: 'at://did:plc:root/space/party/did:plc:root/computer.localnetwork.event/3mwinter' },
		can: capabilitiesFor('party', 'host'),
		role: 'owner' as const
	};

	const waiting = { ...harness, event, status: 'listed' as const };

	const confirmed = { ...harness, event: { ...event, confirmed: true }, status: 'listed' as const };

	const cancelled = { ...harness, event: { ...event, status: EVENT_STATUS.cancelled }, status: 'listed' as const };

	const privateEvent = {
		...owner,
		event: { ...event, audience: 'private' as const, confirmed: true },
		status: 'private' as const,
		link: 'https://localnetwork.test/party/3lzparty/3mwinter/winter-opening?t=b64tokenb64tokenb64token'
	};

	/** A few of the time zones the server sends while editing. */
	const timeZones = ['UTC', 'Europe/Berlin', 'Europe/London', 'America/New_York'];

	const editing = { ...owner, timeZones, event, status: 'listed' as const, editing: true };

	/** A draft whose times were on London's clock, asked how they move once the venue in Berlin is picked. */
	const asked = {
		...editing,
		status: 'draft' as const,
		form: {
			error: 'Pick which times to keep',
			reauth: false,
			field: { name: 'zoneChange', row: 0 },
			choice: {
				name: 'zoneChange',
				question: 'The venue is in Europe/Berlin. Keep 23:00 on its clock (23:00 Berlin), or keep the same moment (00:00 Berlin the next day)?',
				options: [
					{ value: 'clock', label: 'Keep 23:00 on its clock' },
					{ value: 'instant', label: 'Keep the same moment' }
				]
			}
		}
	};

	const made = { ...owner, timeZone: 'Europe/London', timeZones, status: 'draft' as const, editing: true, version: '' };

	const shows = (text: string) => async ({ canvasElement }: { canvasElement: HTMLElement }) => {
		await expect(within(canvasElement).getByText(text)).toBeInTheDocument();
	};
</script>

<Story name="Waiting for the venue" args={waiting} globals={desktop} play={shows('Waiting for the venue')} />

<Story name="Waiting for the venue, phone, light" args={waiting} globals={phone} beforeEach={light} play={shows('Waiting for the venue')} />

<Story name="Confirmed" args={confirmed} globals={desktop} play={shows('Confirmed')} />

<Story name="Confirmed, phone, light" args={confirmed} globals={phone} beforeEach={light} play={shows('Confirmed')} />

<Story name="Cancelled" args={cancelled} globals={desktop} play={shows('Cancelled')} />

<Story name="Cancelled, phone, light" args={cancelled} globals={phone} beforeEach={light} play={shows('Cancelled')} />

<Story name="Private, with its link" args={privateEvent} globals={desktop} play={shows('Private link')} />

<Story name="Private, with its link, phone, light" args={privateEvent} globals={phone} beforeEach={light} play={shows('Private link')} />

<Story name="Edit" args={editing} globals={desktop} play={shows('Add to lineup')} />

<Story name="Edit, phone, light" args={editing} globals={phone} beforeEach={light} play={shows('Add to lineup')} />

<Story name="Asks how the times move" args={asked} globals={desktop} play={shows('Keep the same moment')} />

<Story name="Asks how the times move, phone, light" args={asked} globals={phone} beforeEach={light} play={shows('Keep the same moment')} />

<Story name="New event, draft" args={made} globals={desktop} play={shows("This is a draft. Only the party's people can see it.")} />

<Story name="New event, draft, phone, light" args={made} globals={phone} beforeEach={light} play={shows("This is a draft. Only the party's people can see it.")} />
