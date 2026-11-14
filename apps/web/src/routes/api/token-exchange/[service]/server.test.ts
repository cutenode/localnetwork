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

it("pauses a service while the request's flags have its pause flag on", async () => {
	const response = await callRoute(POST, {
		params: { service: 'mixcloud' },
		url: '/api/token-exchange/mixcloud',
		form: { grant_type: 'authorization_code', code: 'abc' },
		flags: { pauseMixcloudTokenExchange: true }
	});

	expect([response.status, await response.json()]).toEqual([
		503,
		{ error: 'temporarily_unavailable', error_description: 'Local Network has paused signing in to Mixcloud' }
	]);
});
