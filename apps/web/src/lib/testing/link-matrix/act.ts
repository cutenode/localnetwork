import { render } from 'vitest-browser-svelte';
import Page from '../../../routes/projects/[slug]/+page.svelte';
import { NEON, NEON_DJ, NEON_TIDE, PAT, SET_ARTIST, SET_PAGE, type LinkChecks } from '#lib/testing/link-matrix.ts';
import { PAGE_META } from '#lib/testing/page-meta.ts';

const MEMBERS = "Members' profiles";

/** A second member who publishes stats, so How they play names each member. */
const KAI = { ...NEON_DJ, did: 'did:plc:kai', handle: 'kai.test', sets: [] };

// On main an act's page is still /projects/<slug>. PAT is signed in, since only signed-in visitors get Send inquiry.
export default {
	pageType: 'Act',
	render: (row) =>
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
				dj: Promise.resolve(row === MEMBERS ? [NEON_DJ, KAI] : [NEON_DJ]),
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
		[MEMBERS]: [{ name: '@kai.test', href: '/@kai.test', within: 'How they play' }],
		"Managers' profiles": [{ name: '@neon.test', href: '/@neon.test', within: 'Managed by' }],
		'Its press kits': [{ name: 'Press kits', href: '/projects/neon-tide/press-kits' }],
		'Its mixes': [{ name: 'Tracklist', href: '/projects/neon-tide/mixes/1', within: 'Mixes' }],
		'Its sets': [SET_PAGE],
		'Artists in its sets': [SET_ARTIST],
		'Send an inquiry': [{ name: 'Send inquiry', href: '/dashboard/bookings/new?project=neon-tide' }],
		locallibrary: [{ name: 'locallibrary', href: '/tools/locallibrary', within: 'How they play' }]
	}
} satisfies LinkChecks;
