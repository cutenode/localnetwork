import { render } from 'vitest-browser-svelte';
import { EVENT_STATUS } from '#lib/shows.ts';
import Page from '../../../routes/party/[key]/[...rest]/+page.svelte';
import { NEON, PAT, type LinkChecks } from '#lib/testing/link-matrix.ts';
import { PAGE_META } from '#lib/testing/page-meta.ts';

/** A published party, as a signed-in visitor sees it. */
export default {
	pageType: 'Party',
	render: () =>
		render(Page, {
			params: { key: '3lzparty', rest: 'night-shift' },
			data: {
				viewer: PAT,
				pendingRequests: 0,
				meta: PAGE_META,
				editing: false,
				version: 'bafyreiparty',
				status: 'listed',
				party: { basics: { name: 'Night Shift' }, avatarUrl: null },
				events: [
					{
						href: '/party/3lzparty/3mwinter/winter-opening',
						name: 'Winter Opening',
						startsAt: '2026-12-05T23:00:00+01:00',
						status: EVENT_STATUS.scheduled,
						venue: 'Tresor',
						draft: false,
						private: false,
						neverConfirmed: false
					}
				],
				newEventHref: null,
				people: [{ did: NEON.did, handle: NEON.handle, role: 'host' }],
				geocoder: false
			},
			form: null
		}),
	links: {
		'Its upcoming events': [
			{ name: 'Winter Opening', href: '/party/3lzparty/3mwinter/winter-opening', within: 'Upcoming events' }
		],
		"Hosts' and cohosts' profiles": [{ name: '@neon.test', href: '/@neon.test', within: 'Hosted by' }]
	}
} satisfies LinkChecks;
