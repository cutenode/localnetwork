import { render } from 'vitest-browser-svelte';
import { DEMO_PROFILE } from '../../../../scripts/seed-data.ts';
import Page from '../../../routes/projects/+page.svelte';
import { NEON, NEON_TIDE, type LinkChecks } from '#lib/testing/link-matrix.ts';
import { PAGE_META } from '#lib/testing/page-meta.ts';

// Search isn't on main: Find DJs (/projects) searches acts and listed DJs.
export default {
	pageType: 'Search',
	render: () =>
		render(Page, {
			params: {},
			data: {
				viewer: null,
				pendingRequests: 0,
				meta: PAGE_META,
				filters: {
					q: '',
					place: '',
					lat: '',
					lng: '',
					radiusKm: 50,
					genres: [],
					kinds: [],
					formats: [],
					setTypes: [],
					feeTiers: [],
					country: '',
					sort: '',
					unclaimed: false
				},
				placeMissed: false,
				geocoder: true,
				result: {
					items: [
						{ kind: 'project', project: NEON_TIDE },
						{
							kind: 'dj',
							dj: {
								handle: NEON.handle,
								genres: ['techno'],
								profile: {
									did: NEON.did,
									uri: `at://${NEON.did}/computer.localnetwork.profile/self`,
									cid: 'bafyprofile',
									record: { ...DEMO_PROFILE, createdAt: '2026-01-01T00:00:00Z', updatedAt: '2026-01-01T00:00:00Z' },
									avatarUrl: null
								}
							}
						}
					],
					total: 2,
					page: 1,
					pageSize: 24
				}
			},
			form: null
		}),
	links: {
		// Each card is one link named for everything on it, starting with the performer's name.
		'Acts and listed DJs': [
			{ name: /^Neon Tide/, href: '/projects/neon-tide' },
			{ name: /^Neon(?! Tide)/, href: '/@neon.test' }
		]
	}
} satisfies LinkChecks;
