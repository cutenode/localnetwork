import { render } from 'vitest-browser-svelte';
import { sampleReport } from '#lib/mix/sample.ts';
import Page from '../../../routes/projects/[slug]/mixes/[n]/+page.svelte';
import { NEON, NEON_TIDE, type LinkChecks } from '#lib/testing/link-matrix.ts';
import { PAGE_META } from '#lib/testing/page-meta.ts';
import type { DjSetView } from '#lib/types.ts';

/** The set the mix links, as the directory has it. */
const set: DjSetView = {
	uri: `at://${NEON.did}/computer.localnetwork.dj.set/0`,
	cid: 'bafyset',
	title: 'Tidal Hours live',
	date: '2026-09-01',
	trackCount: 12
};

// Checked on a mix's tracklist report, which links the set it was recorded at.
export default {
	pageType: 'Set and mix',
	render: () =>
		render(Page, {
			params: { slug: NEON_TIDE.slug, n: '1' },
			data: {
				viewer: null,
				pendingRequests: 0,
				meta: PAGE_META,
				project: { slug: NEON_TIDE.slug, name: NEON_TIDE.profile.name, avatarUrl: null, status: 'active' },
				mix: { title: 'Tidal Hours', url: 'https://soundcloud.com/neon/tidal-hours', set },
				sets: Promise.resolve(new Map([[set.uri, set]])),
				source: Promise.resolve(null),
				n: 1,
				report: sampleReport
			},
			form: null
		}),
	links: {
		'Credited performers': [{ name: 'Neon Tide', href: '/projects/neon-tide' }],
		// The directory knows the set, but not its DJ's handle, so the link goes by DID and the page sends it on.
		'The set itself, from a mix': [{ name: 'Tidal Hours live', href: '/@did:plc:neon/sets/0' }],
		"Each tracklist row's artist": [{ name: 'Ferrule', href: '/artists/name/Ferrule' }]
	}
} satisfies LinkChecks;
