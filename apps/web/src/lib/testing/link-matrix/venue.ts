import { render } from 'vitest-browser-svelte';
import Page from '../../../routes/venue/[key]/[[name]]/+page.svelte';
import { listedEvent } from '#lib/testing/listed-events.ts';
import { PAT, type LinkChecks } from '#lib/testing/link-matrix.ts';
import { PAGE_META } from '#lib/testing/page-meta.ts';

/** A venue with an upcoming event, as a signed-in visitor sees it. */
export default {
	pageType: 'Venue',
	render: () =>
		render(Page, {
			params: { key: '3tresor', name: 'tresor' },
			data: {
				viewer: PAT,
				pendingRequests: 0,
				meta: PAGE_META,
				venue: { name: 'Tresor', address: { country: 'DE', locality: 'Berlin' }, geo: null },
				events: [listedEvent('3mwinter')],
				dashboard: []
			},
			form: null
		}),
	links: {
		'Its upcoming events': [
			{ name: 'Winter Opening', href: '/party/3lzparty/3mwinter/winter-opening', within: 'Upcoming events' }
		],
		'Parties that run nights there': [
			{ name: 'Night Shift', href: '/party/3lzparty/night-shift', within: 'Upcoming events' }
		]
	}
} satisfies LinkChecks;
