import { render } from 'vitest-browser-svelte';
import { sampleReport } from '#lib/mix/sample.ts';
import Page from '../../../routes/projects/[slug]/mixes/[n]/+page.svelte';
import { NEON_TIDE, type LinkChecks } from '#lib/testing/link-matrix.ts';
import { PAGE_META } from '#lib/testing/page-meta.ts';

// Sets have no page on main: the closest is a mix's tracklist report.
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
				mix: { title: 'Tidal Hours', url: 'https://soundcloud.com/neon/tidal-hours', set: undefined },
				sets: undefined,
				source: Promise.resolve(null),
				n: 1,
				report: sampleReport
			},
			form: null
		}),
	links: {
		'Credited performers': [{ name: 'Neon Tide', href: '/projects/neon-tide' }]
	}
} satisfies LinkChecks;
