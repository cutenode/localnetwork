import { expect, it } from 'vitest';
import { callRoute } from '#lib/testing/route-event.ts';
import { POST } from './+server.ts';

it("refuses a service Local Network doesn't sign in to", async () => {
	const response = await callRoute(POST, {
		params: { service: 'myspace' },
		url: '/api/token-exchange/myspace',
		form: { grant_type: 'authorization_code', code: 'abc' }
	});

	expect(response.status).toBe(404);
	expect(await response.json()).toMatchObject({ error: 'invalid_request' });
});
