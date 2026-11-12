import { render } from 'vitest-browser-svelte';
import { DEMO_PROFILE } from '../../../../scripts/seed-data.ts';
import Page from '../../../routes/@[handle]/+page.svelte';
import { NEON, NEON_DJ, NEON_TIDE, PAT, SET_ARTIST, type LinkChecks } from '#lib/testing/link-matrix.ts';
import { PAGE_META } from '#lib/testing/page-meta.ts';

const ACTS = 'Acts they play in or manage';

const ARTISTS = 'Artists in their sets';

const INQUIRY = 'Send an inquiry';

export default {
	pageType: 'Profile',
	// The page shows one tab at a time, so each row's profile has only the section its links are in, which then shows
	// under its heading: the acts they play in, or their sets. Only a listed DJ takes inquiries, and only from someone
	// signed in.
	render: (row) =>
		render(Page, {
			params: { handle: NEON.handle },
			data: {
				viewer: row === INQUIRY ? PAT : null,
				pendingRequests: 0,
				profile: {
					did: NEON.did,
					uri: `at://${NEON.did}/computer.localnetwork.profile/self`,
					cid: 'bafyprofile',
					record: {
						basics: { name: DEMO_PROFILE.basics.name },
						createdAt: '2026-01-01T00:00:00Z',
						updatedAt: '2026-01-01T00:00:00Z'
					},
					avatarUrl: null
				},
				name: DEMO_PROFILE.basics.name,
				account: NEON,
				bookable: row === INQUIRY,
				meta: PAGE_META,
				dj: Promise.resolve(row === ARTISTS ? [{ ...NEON_DJ, profile: null }] : []),
				sets: undefined,
				playsIn: Promise.resolve(row === ACTS ? [NEON_TIDE] : [])
			},
			form: null
		}),
	links: {
		[ACTS]: [{ name: 'Neon Tide', href: '/projects/neon-tide', within: 'Plays in' }],
		[ARTISTS]: [{ ...SET_ARTIST, within: 'Sets' }],
		[INQUIRY]: [{ name: 'Send inquiry', href: `/dashboard/bookings/new?dj=${NEON.did}` }]
	}
} satisfies LinkChecks;
