<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { expect, within } from 'storybook/test';
	import { capabilitiesFor } from '#lib/capabilities.ts';
	import { EVENT_STATUS } from '#lib/shows.ts';
	import { light } from '#lib/testing/light.ts';
	import { desktop, phone } from '#lib/testing/viewports.ts';
	import PartyPage from './PartyPage.svelte';

	/**
	 * A party's page, built in place: as everyone sees it, with only a name, in edit mode, and empty as a party is
	 * started. Each at 1280px in the dark palette and at 390px in the light one.
	 */
	const { Story } = defineMeta({
		title: 'Pages/Party',
		component: PartyPage,
		parameters: { layout: 'fullscreen' }
	});

	/** Stands in for the app's navigation, its form sending and its leave guard, which a story doesn't have. */
	const harness = {
		url: new URL('https://localnetwork.test/party/3lzparty/night-shift'),
		viewer: { did: 'did:plc:neon' },
		navigate: async () => {},
		guardNavigation: () => {},
		enhance: () => ({ destroy: () => {} }),
		geocoder: false,
		version: 'bafyreiopened'
	};

	const party = {
		basics: {
			name: 'Night Shift',
			bio: 'Techno on Thursdays, in the back room until six.\nResidents, one guest a month, no list.',
			location: { city: 'Berlin', country: 'DE' },
			links: [
				{ kind: 'website', url: 'https://nightshift.example' },
				{ kind: 'soundcloud', url: 'https://soundcloud.com/nightshift' }
			]
		},
		avatarUrl: null
	};

	const people = [
		{ did: 'did:plc:neon', handle: 'neon.test', role: 'host' },
		{ did: 'did:plc:sam', handle: 'sam.test', role: 'cohost' }
	];

	const owner = {
		...harness,
		subject: { kind: 'party' as const, uri: 'at://did:plc:root/space/party' },
		can: capabilitiesFor('party', 'host'),
		role: 'owner' as const,
		newEventHref: '/party/3lzparty/new'
	};

	/** Its next nights as its people see them: a public one, a private one and a draft without a date yet. */
	const events = [
		{ href: '/party/3lzparty/3mwinter/winter-opening', name: 'Winter Opening', startsAt: '2026-12-05T23:00:00+01:00', status: EVENT_STATUS.scheduled, venue: 'Tresor', draft: false, private: false },
		{ href: '/party/3lzparty/3mback/back-room', name: 'Back Room', startsAt: '2026-12-12T23:00:00+01:00', status: EVENT_STATUS.scheduled, venue: 'Tresor', draft: false, private: true },
		{ href: '/party/3lzparty/3mspring/spring', name: 'Spring Opening', status: EVENT_STATUS.scheduled, venue: null, draft: true, private: false }
	];

	const view = { ...owner, party, people, events, status: 'listed' as const };

	const empty = { ...harness, party: { basics: { name: 'Night Shift' }, avatarUrl: null }, status: 'unlisted' as const };

	const editing = { ...view, editing: true };

	const started = { ...owner, newEventHref: null, editing: true, status: 'draft' as const, version: '' };

	const shows = (text: string) => async ({ canvasElement }: { canvasElement: HTMLElement }) => {
		await expect(within(canvasElement).getByText(text)).toBeInTheDocument();
	};
</script>

<Story name="View" args={view} globals={desktop} play={shows('Winter Opening')} />

<Story name="View, phone, light" args={view} globals={phone} beforeEach={light} play={shows('Hosted by')} />

<Story name="Empty party" args={empty} globals={desktop} play={shows('Night Shift')} />

<Story name="Empty party, phone, light" args={empty} globals={phone} beforeEach={light} play={shows('Night Shift')} />

<Story name="Edit" args={editing} globals={desktop} play={shows('Upcoming events')} />

<Story name="Edit, phone, light" args={editing} globals={phone} beforeEach={light} play={shows('Upcoming events')} />

<Story name="New party, draft" args={started} globals={desktop} play={shows('This is a draft. Only its people can see it.')} />

<Story name="New party, draft, phone, light" args={started} globals={phone} beforeEach={light} play={shows('This is a draft. Only its people can see it.')} />
