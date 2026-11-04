import { expect, it } from 'vitest';
import { callRoute } from '#lib/testing/route-event.ts';
import { GET } from './+server.ts';

it('sends the old press kit list address on to the new one, keeping its query', async () => {
	await expect(
		callRoute(GET, { url: '/projects/neon-tide/epk?utm_source=email', params: { slug: 'neon-tide' } })
	).rejects.toMatchObject({ status: 308, location: '/projects/neon-tide/press-kits?utm_source=email' });
});
