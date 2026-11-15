import { render } from 'vitest-browser-svelte';
import Page from '../../../routes/for/+page.svelte';
import type { LinkChecks } from '#lib/testing/link-matrix.ts';
import { PAGE_META } from '#lib/testing/page-meta.ts';

export default {
	pageType: 'For',
	render: () =>
		render(Page, {
			params: {},
			data: {
				viewer: null,
				pendingRequests: 0,
				meta: PAGE_META,
				stats: Promise.resolve({ djs: 3, cities: 2, unclaimed: 0 })
			},
			form: null
		}),
	links: {
		'For promoters': [{ name: 'I put on shows.', href: '/for/promoters' }],
		'For DJs': [{ name: 'I play out.', href: '/for/djs' }],
		'Find DJs': [{ name: 'Find DJs', href: '/projects' }]
	}
} satisfies LinkChecks;
