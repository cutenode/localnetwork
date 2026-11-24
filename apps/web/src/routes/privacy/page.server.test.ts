import { expect, it } from 'vitest';
import { loadRoute } from '#lib/testing/route-event.ts';
import type { PageServerData } from './$types';
import { load } from './+page.server.ts';

it('says how long press kit views and sign-ins are kept, under the canonical /privacy address', async () => {
	const app = {
		epkViews: { retentionDays: 90 },
		config: { publicUrl: 'https://localnetwork.computer', sessionDays: 30 }
	};

	const data = await loadRoute<PageServerData>(load, { app, url: '/privacy' });

	expect(data.retentionDays).toBe(90);
	expect(data.sessionDays).toBe(30);
	expect(data.meta).toMatchObject({ title: 'Privacy', url: 'https://localnetwork.computer/privacy' });
});
