import { render } from 'vitest-browser-svelte';
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
				people: [{ did: NEON.did, handle: NEON.handle, role: 'host' }],
				geocoder: false
			},
			form: null
		}),
	links: {
		"Hosts' and cohosts' profiles": [{ name: '@neon.test', href: '/@neon.test', within: 'Hosted by' }]
	}
} satisfies LinkChecks;
