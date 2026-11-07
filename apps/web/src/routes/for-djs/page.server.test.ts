import { expect, it } from 'vitest';
import { loadRoute } from '#lib/testing/route-event.ts';
import type { PageServerData } from './$types';
import { load } from './+page.server.ts';

it("describes the page for search engines and link previews at the site's own address", async () => {
	const data = await loadRoute<PageServerData>(load, {
		app: { config: { publicUrl: 'https://localnetwork.test' } },
		url: '/for-djs'
	});

	expect(data).toMatchObject({
		meta: {
			title: 'For DJs',
			description: 'List or claim your DJ project so promoters near you can find and book you.',
			url: 'https://localnetwork.test/for-djs'
		}
	});
});
