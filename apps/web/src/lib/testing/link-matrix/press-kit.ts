import { render } from 'vitest-browser-svelte';
import { DEFAULT_ACCESS } from '#lib/epk.ts';
import Page from '../../../routes/projects/[slug]/press-kits/[rkey]/+page.svelte';
import { NEON, NEON_DJ, NEON_TIDE, PAT, SET_ARTIST, type LinkChecks } from '#lib/testing/link-matrix.ts';
import { PAGE_META } from '#lib/testing/page-meta.ts';

const [mix] = NEON_TIDE.profile.mixes ?? [];

// PAT has been let in, so the page shows the private part too.
export default {
	pageType: 'Press kit',
	render: () =>
		render(Page, {
			params: { slug: NEON_TIDE.slug, rkey: 'clubs' },
			data: {
				viewer: PAT,
				pendingRequests: 0,
				meta: PAGE_META,
				project: NEON_TIDE,
				details: {},
				managers: [NEON],
				isManager: false,
				dj: Promise.resolve([NEON_DJ]),
				epk: {
					uri: 'at://did:plc:root/computer.localnetwork.epk/clubs',
					did: 'did:plc:root',
					rkey: 'clubs',
					cid: 'bafyepk',
					space: 'ats://did:plc:root/computer.localnetwork.space/clubs',
					legacy: false,
					record: {
						project: NEON_TIDE.uri,
						title: 'Clubs',
						mixes: [{ title: mix.title, url: mix.url }],
						access: DEFAULT_ACCESS,
						createdAt: '2026-01-01T00:00:00Z'
					}
				},
				access: { kind: 'granted', since: '2026-01-01T00:00:00Z' },
				content: { updatedAt: '2026-01-01T00:00:00Z' },
				tracklists: { [mix.url]: 1 }
			},
			form: null
		}),
	links: {
		'Its mixes': [{ name: 'Tracklist', href: '/projects/neon-tide/mixes/1' }],
		'Artists in its sets': [SET_ARTIST]
	}
} satisfies LinkChecks;
