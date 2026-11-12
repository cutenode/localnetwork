import { render } from 'vitest-browser-svelte';
import { DEMO_PROFILE } from '../../../../scripts/seed-data.ts';
import Page from '../../../routes/@[handle]/+page.svelte';
import { NEON, NEON_DJ, NEON_TIDE, SET_ARTIST, type LinkChecks } from '#lib/testing/link-matrix.ts';
import { PAGE_META } from '#lib/testing/page-meta.ts';

export default {
	pageType: 'Profile',
	render: () =>
		render(Page, {
			params: { handle: NEON.handle },
			data: {
				viewer: null,
				pendingRequests: 0,
				profile: {
					did: NEON.did,
					uri: `at://${NEON.did}/computer.localnetwork.profile/self`,
					cid: 'bafyprofile',
					record: { ...DEMO_PROFILE, createdAt: '2026-01-01T00:00:00Z', updatedAt: '2026-01-01T00:00:00Z' },
					avatarUrl: null
				},
				name: DEMO_PROFILE.basics.name,
				account: NEON,
				meta: PAGE_META,
				dj: Promise.resolve([NEON_DJ]),
				sets: undefined,
				playsIn: Promise.resolve([NEON_TIDE])
			},
			form: null
		}),
	links: {
		'Acts they play in or manage': [{ name: 'Neon Tide', href: '/projects/neon-tide', within: 'Plays in' }],
		'Artists in their sets': [SET_ARTIST]
	}
} satisfies LinkChecks;
