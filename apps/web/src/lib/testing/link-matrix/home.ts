import { render } from 'vitest-browser-svelte';
import Page from '../../../routes/+page.svelte';
import { NEON_TIDE, type LinkChecks } from '#lib/testing/link-matrix.ts';
import { PAGE_META } from '#lib/testing/page-meta.ts';

const berlin = { place: 'Berlin', lat: 52.52, lng: 13.405, radiusKm: 50 };

// The home page with the socialHome flag on, signed out, with a place picked and Neon Tide playing there.
export default {
	pageType: 'Home',
	render: () =>
		render(Page, {
			params: {},
			data: {
				social: true,
				viewer: null,
				pendingRequests: 0,
				meta: PAGE_META,
				geocoder: false,
				picked: berlin,
				network: null,
				area: Promise.resolve({ place: berlin, shows: [], djs: [{ project: NEON_TIDE, follow: null }] }),
				tracks: Promise.resolve([])
			},
			form: null
		}),
	links: {
		'DJs and mixes from your area': [
			{ name: 'Neon Tide', href: '/projects/neon-tide', within: "Who's playing near Berlin" },
			{ name: /^Tidal Hours/, href: '/projects/neon-tide/mixes/1' }
		]
	}
} satisfies LinkChecks;
