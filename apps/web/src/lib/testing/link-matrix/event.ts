import { render } from 'vitest-browser-svelte';
import { EVENT_STATUS } from '#lib/shows.ts';
import Page from '../../../routes/party/[key]/[...rest]/+page.svelte';
import { NEON, PAT, type LinkChecks } from '#lib/testing/link-matrix.ts';
import { PAGE_META } from '#lib/testing/page-meta.ts';

/** A published event of a party's, as a signed-in visitor sees it. */
export default {
	pageType: 'Event',
	render: () =>
		render(Page, {
			params: { key: '3lzparty', rest: '3mwinter/winter-opening' },
			data: {
				viewer: PAT,
				pendingRequests: 0,
				meta: PAGE_META,
				editing: false,
				timeZones: [],
				version: 'bafyreievent',
				status: 'listed',
				event: {
					name: 'Winter Opening',
					startsAt: '2026-12-05T23:00:00+01:00',
					tickets: 'https://tickets.example/winter',
					status: EVENT_STATUS.scheduled,
					audience: 'public',
					venue: null,
					timeZone: 'Europe/Berlin',
					confirmed: false,
					lineup: [{ name: `@${NEON.handle}`, href: `/@${NEON.handle}`, value: NEON.handle }]
				},
				party: { name: 'Night Shift', href: '/party/3lzparty/night-shift' },
				link: null
			},
			form: null
		}),
	links: {
		Party: [{ name: 'Night Shift', href: '/party/3lzparty/night-shift' }],
		'Each performer on the lineup': [{ name: '@neon.test', href: '/@neon.test', within: 'Lineup' }],
		'Ticket sites': [{ name: 'Tickets', href: 'https://tickets.example/winter' }]
	}
} satisfies LinkChecks;
