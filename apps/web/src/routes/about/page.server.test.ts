import { expect, it } from 'vitest';
import { loadRoute } from '#lib/testing/route-event.ts';
import type { PageServerData } from './$types';
import { load } from './+page.server.ts';

it('describes the page under the canonical /about address', async () => {
	const app = { config: { publicUrl: 'https://localnetwork.computer' } };

	const data = await loadRoute<PageServerData>(load, { app, url: '/about' });

	expect(data.meta).toMatchObject({ title: 'How it works', url: 'https://localnetwork.computer/about' });
});
