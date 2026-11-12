import { render } from 'vitest-browser-svelte';
import { artistSupports } from '#lib/components/supports/fixtures.ts';
import Page from '../../../routes/artists/[id]/+page.svelte';
import type { LinkChecks } from '#lib/testing/link-matrix.ts';
import { PAGE_META } from '#lib/testing/page-meta.ts';

export default {
	pageType: 'Artist',
	render: () =>
		render(Page, {
			params: { id: '2a8e4f60-1c3b-4d7e-9a05-6b1f0c2d3e41' },
			data: { viewer: null, pendingRequests: 0, meta: PAGE_META, supports: artistSupports },
			form: null
		}),
	links: {
		'Acts who played their tracks': [{ name: 'Pirate Signal', href: '/projects/pirate-signal' }]
	}
} satisfies LinkChecks;
