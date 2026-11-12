import { render } from 'vitest-browser-svelte';
import { EVENT_STATUS } from '#lib/shows.ts';
import Page from '../../../routes/shows/+page.svelte';
import { NEON, NEON_TIDE, type LinkChecks } from '#lib/testing/link-matrix.ts';
import { PAGE_META } from '#lib/testing/page-meta.ts';

// Events have no page on main: a show is a row on /shows, which names its act, and on the act's own page.
export default {
	pageType: 'Event',
	render: () =>
		render(Page, {
			params: {},
			data: {
				viewer: null,
				pendingRequests: 0,
				meta: PAGE_META,
				following: false,
				place: null,
				radii: [10, 25, 50, 100, 250],
				geocoder: true,
				shows: Promise.resolve({
					items: [
						{
							uri: `at://${NEON.did}/computer.localnetwork.show/a`,
							cid: 'c',
							did: NEON.did,
							rkey: 'a',
							project: NEON_TIDE,
							record: {
								project: NEON_TIDE.uri,
								event: { uri: `at://${NEON.did}/community.lexicon.calendar.event/a`, cid: 'e' },
								name: 'Tidal Hours',
								startsAt: '2026-11-07T23:00:00+01:00',
								venue: 'Halle Nord',
								location: { city: 'Berlin', country: 'DE' },
								status: EVENT_STATUS.scheduled,
								tickets: 'https://tickets.example/tidal',
								createdAt: '2026-10-01T00:00:00Z'
							}
						}
					]
				})
			},
			form: null
		}),
	links: {
		'Each performer on the lineup': [{ name: 'Neon Tide', href: '/projects/neon-tide' }],
		'Ticket sites': [{ name: 'Tickets', href: 'https://tickets.example/tidal' }]
	}
} satisfies LinkChecks;
