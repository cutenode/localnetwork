import { expect, it } from 'vitest';
import { RateLimiter } from '#lib/server/rate-limit.ts';
import type { Viewer } from '#lib/types.ts';
import { callRoute, loadRoute } from '#lib/testing/route-event.ts';
import type { PageServerData } from './$types';
import { actions, load } from './+page.server.ts';

const FAN: Viewer = {
	did: 'did:plc:fan',
	handle: 'fan.test',
	roles: [],
	isAdmin: false,
	isStaff: false,
	spacesCapable: true
};

/** A sign-up started at a host, as the stand-in authorization server saw it. */
type Started = { service: string; state: string; prompt?: 'create' };

/** An authorization server that starts sign-ups at the hosts it can reach, and keeps each one. */
class FakeOAuth {
	readonly started: Started[] = [];

	constructor(private readonly unreachable: string[] = []) {}

	async authorize(service: string, state: string, options: { prompt?: 'create' } = {}): Promise<URL> {
		if (this.unreachable.includes(service)) throw new Error(`fetch failed: ${service}`);
		this.started.push({ service, state, prompt: options.prompt });

		return new URL(`${service}/oauth/authorize?prompt=create`);
	}
}

/** The app on a production network, with sign-up attempts limited to `attempts` a minute per address. */
const app = (oauth = new FakeOAuth(), attempts = 10) => ({
	config: { dev: false, root: { identifier: 'localnetwork.computer', password: 'root-password' } },
	oauth,
	limits: { login: new RateLimiter(attempts, attempts / 60) }
});

it('sends someone already signed in on to where they were going', async () => {
	await expect(callRoute(load, { app: app(), viewer: FAN, url: '/login/create?next=/shows' })).rejects.toMatchObject({
		status: 303,
		location: '/shows'
	});
});

it('offers the hosts to make an account at', async () => {
	const data = await loadRoute<PageServerData>(load, { app: app(), url: '/login/create?next=/for-djs' });

	expect(data).toMatchObject({
		next: '/for-djs',
		hosts: expect.arrayContaining([
			{
				name: 'Bluesky',
				domain: 'bsky.social',
				service: 'https://bsky.social',
				country: 'United States',
				note: 'most people start here'
			}
		])
	});
});

it('starts sign-up at the chosen host, carrying where to go after', async () => {
	const oauth = new FakeOAuth();

	await expect(
		callRoute(actions.default, {
			app: app(oauth),
			url: '/login/create',
			form: { service: 'https://bsky.social', next: '/for-djs' }
		})
	).rejects.toMatchObject({ status: 303, location: 'https://bsky.social/oauth/authorize?prompt=create' });

	expect(oauth.started).toEqual([
		{ service: 'https://bsky.social', state: JSON.stringify({ next: '/for-djs' }), prompt: 'create' }
	]);
});

it('refuses a host that is not on the list', async () => {
	const result = await callRoute(actions.default, {
		app: app(),
		url: '/login/create',
		form: { service: 'https://pds.evil.test' }
	});

	expect(result).toMatchObject({ status: 400, data: { error: 'Pick a host from the list.' } });
});

it('turns away too many attempts from one address', async () => {
	const oauth = new FakeOAuth();

	const result = await callRoute(actions.default, {
		app: app(oauth, 0),
		url: '/login/create',
		form: { service: 'https://bsky.social' }
	});

	expect(result).toMatchObject({ status: 429, data: { error: 'Too many sign-in attempts. Try again in a minute.' } });
	expect(oauth.started).toEqual([]);
});

it("says so when the host can't be reached", async () => {
	const result = await callRoute(actions.default, {
		app: app(new FakeOAuth(['https://blacksky.app'])),
		url: '/login/create',
		form: { service: 'https://blacksky.app' }
	});

	expect(result).toMatchObject({
		status: 502,
		data: { error: "We couldn't reach Blacksky. Try again, or pick another host." }
	});
});
