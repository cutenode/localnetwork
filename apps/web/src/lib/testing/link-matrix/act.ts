import { render } from 'vitest-browser-svelte';
import Page from '../../../routes/projects/[slug]/+page.svelte';
import { NEON, NEON_DJ, NEON_TIDE, PAT, SET_ARTIST, type LinkChecks } from '#lib/testing/link-matrix.ts';
import { PAGE_META } from '#lib/testing/page-meta.ts';

// On main an act's page is still /projects/<slug>. PAT is signed in, since only signed-in visitors get Send inquiry.
export default {
	pageType: 'Act',
	render: () =>
		render(Page, {
			params: { slug: NEON_TIDE.slug },
			data: {
				viewer: PAT,
				pendingRequests: 0,
				meta: PAGE_META,
				project: NEON_TIDE,
				sets: undefined,
				details: null,
				managers: [NEON],
				dj: Promise.resolve([NEON_DJ]),
				isManager: false,
				saved: null,
				pendingClaim: null,
				pendingUpdate: null,
				shows: Promise.resolve([]),
				follow: Promise.resolve({ followers: 0, following: false })
			},
			form: null
		}),
	links: {
		"Managers' profiles": [{ name: '@neon.test', href: '/@neon.test', within: 'Managed by' }],
		'Its press kits': [{ name: 'Press kits', href: '/projects/neon-tide/press-kits' }],
		'Its mixes': [{ name: 'Tracklist', href: '/projects/neon-tide/mixes/1', within: 'Mixes' }],
		'Artists in its sets': [SET_ARTIST],
		'Send an inquiry': [{ name: 'Send inquiry', href: '/dashboard/bookings/new?project=neon-tide' }]
	}
} satisfies LinkChecks;
