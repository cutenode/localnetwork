import { afterEach, beforeEach, expect, it } from 'vitest';
import type { SignedInAccount } from '#lib/server/atproto/oauth.ts';
import { RateLimiter } from '#lib/server/rate-limit.ts';
import { createTestServices } from '#lib/server/testing/core.ts';
import { callRoute, loadRoute } from '#lib/testing/route-event.ts';
import type { PageServerData } from './$types';
import { actions, load } from './+page.server.ts';

const ADMIN = 'did:plc:admin';

const DJ = 'did:plc:dj';

/** A started authorization, as the stand-in PDS saw it. */
type Started = { identifier: string; state: string; account?: SignedInAccount };

/** An authorization server that knows only handles ending in `.test`, and keeps each authorization it started. */
class FakeOAuth {
	readonly started: Started[] = [];

	async authorize(identifier: string, state: string, options: { account?: SignedInAccount } = {}): Promise<URL> {
		if (!identifier.endsWith('.test')) throw new Error(`could not resolve ${identifier}`);
		this.started.push({ identifier, state, account: options.account });

		return new URL(`https://pds.test/oauth/authorize?request_uri=urn:request:${this.started.length}`);
	}
}

let t: Awaited<ReturnType<typeof createTestServices>>;

let oauth: FakeOAuth;

beforeEach(async () => {
	t = await createTestServices({ admins: [ADMIN] });
	oauth = new FakeOAuth();
});

afterEach(async () => {
	await t.audit.idle();
	await t.db.destroy();
});

/** The app, with sign-in attempts limited to `attempts` a minute per address. */
const app = (attempts = 10) => ({
	...t,
	oauth,
	limits: { login: new RateLimiter(attempts, attempts / 60) },
	identity: {
		resolveIdentifier: async (identifier: string) => {
			if (identifier === 'dj.test') return DJ;
			throw new Error(`no such handle ${identifier}`);
		}
	}
});

/** Posts the sign-in form. */
const signIn = (form: Record<string, string>, attempts?: number) =>
	callRoute(actions.default, { app: app(attempts), url: '/login', form });

it('sends someone already signed in on to where they were going', async () => {
	await expect(
		callRoute(load, { app: app(), viewer: await t.viewer(DJ), url: '/login?next=/dashboard/planner' })
	).rejects.toMatchObject({ status: 303, location: '/dashboard/planner' });
});

it('says why someone was sent to sign in, and whether their session needs it again', async () => {
	const data = await loadRoute<PageServerData>(load, {
		app: app(),
		url: '/login?next=/shows?view=following&error=denied'
	});

	expect(data).toEqual({
		next: '/shows?view=following',
		purpose: 'see shows from DJs you follow',
		error: 'denied',
		reauth: false
	});
});

it('names the project someone was about to book, and says their session has to sign in again', async () => {
	const project = await t.projectAdmin.create({
		actor: await t.viewer(ADMIN),
		profile: { name: 'Neon Tide', kind: 'solo', genres: ['techno'], location: { city: 'Berlin', country: 'DE' } }
	});

	const data = await loadRoute<PageServerData>(load, {
		app: app(),
		reauth: true,
		url: `/login?next=/projects/${project.slug}`
	});

	expect(data).toMatchObject({ purpose: 'book Neon Tide', error: null, reauth: true });
});

it('starts sign-in at the account host, carrying where to go after', async () => {
	await expect(signIn({ handle: '@fan.test', next: '/projects/neon-tide' })).rejects.toMatchObject({
		status: 303,
		location: 'https://pds.test/oauth/authorize?request_uri=urn:request:1'
	});

	expect(oauth.started).toEqual([
		{ identifier: 'fan.test', state: JSON.stringify({ next: '/projects/neon-tide' }), account: undefined }
	]);
});

it('passes on the scopes an account approved before', async () => {
	await t.accounts.recordLogin({
		did: DJ,
		handle: 'dj.test',
		pdsUrl: 'https://pds.test',
		grantedScope: 'atproto account:email',
		spacesCapable: false,
		publicWrites: false
	});

	await expect(signIn({ handle: 'dj.test', next: 'https://evil.test' })).rejects.toMatchObject({ status: 303 });

	expect(oauth.started).toMatchObject([
		{
			identifier: 'dj.test',
			state: JSON.stringify({ next: '/dashboard' }),
			account: { grantedScope: 'atproto account:email' }
		}
	]);
});

it('asks for a handle when there is none', async () => {
	expect(await signIn({ handle: '  @ ' })).toMatchObject({
		status: 400,
		data: { error: 'Enter your handle, like alice.bsky.social', handle: '' }
	});
});

it('turns away too many attempts from one address', async () => {
	expect(await signIn({ handle: 'fan.test' }, 0)).toMatchObject({
		status: 429,
		data: { error: 'Too many sign-in attempts. Try again in a minute.', handle: 'fan.test' }
	});
	expect(oauth.started).toEqual([]);
});

it("says so when the handle's host can't be found", async () => {
	expect(await signIn({ handle: 'nobody.example' })).toMatchObject({
		status: 400,
		data: {
			error: "We couldn't find @nobody.example or reach its host. Check the handle, or make an account.",
			handle: 'nobody.example'
		}
	});
});
