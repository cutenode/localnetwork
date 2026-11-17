import { afterEach, beforeEach, expect, it } from 'vitest';
import { z } from 'zod';
import { SCOPES } from '#lib/server/atproto/oauth.ts';
import { SESSION_COOKIE, WebSessions } from '#lib/server/auth/session.ts';
import { silentLogger } from '#lib/server/logger.ts';
import { NotificationChannels } from '#lib/server/notifications/channels.ts';
import { RateLimiter } from '#lib/server/rate-limit.ts';
import { createTestServices, ROOT } from '#lib/server/testing/core.ts';
import { FakeNetwork } from '#lib/server/testing/fakes.ts';
import { routeEvent } from '#lib/testing/route-event.ts';
import { GET } from './+server.ts';

const ADMIN = 'did:plc:admin';

const MOD = 'did:plc:mod';

const DJ = 'did:plc:dj';

const PDS = 'https://pds.test';

const createRecordInput = z.object({
	space: z.string(),
	collection: z.string(),
	rkey: z.string().optional(),
	record: z.record(z.string(), z.json())
});

let t: Awaited<ReturnType<typeof createTestServices>>;

let sessions: WebSessions;

let oauth: FakeOAuth;

/** The PDS behind the OAuth sessions, apart from the network the app's own clients write to. */
let pds: FakeNetwork;

beforeEach(async () => {
	t = await createTestServices({ admins: [ADMIN], capable: [DJ] });
	pds = new FakeNetwork();
	sessions = new WebSessions(t.db, 30);
	oauth = new FakeOAuth();
	await t.staff.grant(await t.viewer(ADMIN), MOD, 'moderator');
});

afterEach(async () => {
	await t.audit.idle();
	await t.db.destroy();
});

/** An authorization the stand-in PDS approved, which the callback's `code` names. */
type Approval = { did: string; state: string | null; scope: string };

/**
 * An authorization server that approves what a test tells it to, handing back an OAuth session whose requests go to
 * the fake network as the account. The state it keeps for declined ones is what OAuthService.failedState reads.
 */
class FakeOAuth {
	private readonly approvals = new Map<string, Approval>();
	private readonly declined = new Map<string, string>();

	/** Approves `approval`, and returns the callback's query. */
	approve(approval: Approval): string {
		const code = `code-${this.approvals.size + 1}`;
		this.approvals.set(code, approval);

		return `code=${code}&iss=${encodeURIComponent(PDS)}`;
	}

	/** The user turned down an authorization started with `state`; returns the callback's query. */
	decline(state: string, error = 'access_denied'): string {
		const id = `state-${this.declined.size + 1}`;
		this.declined.set(id, state);

		return `error=${error}&state=${id}`;
	}

	async callback(params: URLSearchParams) {
		const approval = this.approvals.get(params.get('code') ?? '');

		if (!approval) throw new Error('Unknown authorization session');

		return { session: oauthSession(approval.did), state: approval.state, scope: approval.scope };
	}

	async failedState(params: URLSearchParams): Promise<string | undefined> {
		return this.declined.get(params.get('state') ?? '');
	}
}

/** An OAuth session for `did` on `pds`, whose account has a confirmed email address. */
const oauthSession = (did: string) => ({
	did,
	fetchHandler: async (path: string, init: RequestInit) => {
		const { pathname } = new URL(path, PDS);

		if (pathname === '/xrpc/com.atproto.server.getSession')
			return Response.json({ did, handle: 'dj.test', email: 'dj@example.com', emailConfirmed: true });

		if (pathname === '/xrpc/com.atproto.space.createRecord')
			return Response.json(
				await pds.client(did).createSpaceRecord(createRecordInput.parse(await new Response(init.body).json()))
			);

		return Response.json({ error: 'MethodNotImplemented' }, { status: 501 });
	}
});

/** The app at `publicUrl`, whose identity resolver knows DJ but no one else. */
const app = (publicUrl = 'https://localnetwork.test') => ({
	...t,
	rootDid: ROOT,
	oauth,
	sessions,
	config: { publicUrl },
	identity: {
		resolve: async (did: string) => {
			if (did !== DJ) throw new Error(`could not resolve ${did}`);

			return { did, handle: 'dj.test', pds: PDS };
		}
	},
	notificationChannels: new NotificationChannels({
		db: t.db,
		logger: silentLogger,
		limits: {
			account: new RateLimiter(5, 5 / 86_400),
			accountAddress: new RateLimiter(3, 3 / 86_400),
			address: new RateLimiter(10, 10 / 86_400),
			all: new RateLimiter(100, 100 / 3600)
		}
	})
});

/** The browser lands on the callback with `query`, holding `cookies`: where the callback sends it, and its cookies. */
function land(query: string, { cookies = {}, publicUrl = 'https://localnetwork.test' } = {}) {
	const event = routeEvent({
		app: app(publicUrl),
		url: `/oauth/callback?${query}`,
		cookies,
		headers: { 'user-agent': 'Mozilla/5.0 Firefox/144.0' }
	});

	return { landed: event.call(GET), cookies: event.cookies };
}

/** The redirect to `location` the callback ends with. */
const redirectTo = (location: string) => ({ status: 303, location });

/** The sign-in entries in the audit log, once every copy of them was delivered. */
async function logins() {
	await t.audit.idle();

	return t.db
		.selectFrom('auditEntry')
		.select(['actorDid', 'actorRole', 'summary', 'details', 'actorStatus'])
		.where('action', '=', 'auth.login')
		.execute();
}

it('signs in an account that granted spaces, then sends it where it was going', async () => {
	const query = oauth.approve({ did: DJ, state: JSON.stringify({ next: '/dashboard/planner' }), scope: SCOPES.full });
	const { landed, cookies } = land(query);

	await expect(landed).rejects.toMatchObject(redirectTo('/dashboard/planner'));
	expect(await sessions.resolve(cookies.get(SESSION_COOKIE))).toBe(DJ);
	expect(cookies.options.get(SESSION_COOKIE)).toEqual({
		path: '/',
		httpOnly: true,
		sameSite: 'lax',
		secure: true,
		maxAge: 30 * 86_400
	});
	expect(await t.accounts.get(DJ)).toMatchObject({
		handle: 'dj.test',
		pdsUrl: PDS,
		grantedScope: SCOPES.full,
		spacesCapable: 1,
		publicWrites: 1
	});
});

it("logs the sign-in, with the account's own copy written through its new session", async () => {
	await expect(land(oauth.approve({ did: DJ, state: null, scope: SCOPES.full })).landed).rejects.toMatchObject(
		redirectTo('/dashboard')
	);

	expect(await logins()).toEqual([
		{
			actorDid: DJ,
			actorRole: 'member',
			summary: 'Signed in as @dj.test',
			details: JSON.stringify({ spacesCapable: true, pds: PDS }),
			actorStatus: 'written'
		}
	]);
	expect(pds.spaceRecordsIn(t.spaces.audit, DJ)).toHaveLength(1);
	expect(t.net.spaceRecordsIn(t.spaces.audit, DJ)).toEqual([]);
});

it('signs in an account without spaces, which the root writes for, at the dashboard', async () => {
	const { landed } = land(oauth.approve({ did: ADMIN, state: null, scope: 'atproto' }), {
		publicUrl: 'http://127.0.0.1:5173'
	});

	await expect(landed).rejects.toMatchObject(redirectTo('/dashboard'));
	expect(await t.accounts.get(ADMIN)).toMatchObject({ handle: null, pdsUrl: null, spacesCapable: 0, publicWrites: 0 });
	expect(await logins()).toMatchObject([{ actorDid: ADMIN, actorRole: 'admin', summary: 'Signed in' }]);
	expect(pds.spaceRecordsIn(t.spaces.audit, ADMIN)).toEqual([]);
});

it("sets the session cookie without `secure` on a plain http site, and logs a moderator's role", async () => {
	const { landed, cookies } = land(oauth.approve({ did: MOD, state: '{"next":"/admin"}', scope: 'atproto' }), {
		publicUrl: 'http://127.0.0.1:5173'
	});

	await expect(landed).rejects.toMatchObject(redirectTo('/admin'));
	expect(cookies.options.get(SESSION_COOKIE)).toMatchObject({ secure: false });
	expect(await logins()).toMatchObject([{ actorDid: MOD, actorRole: 'moderator' }]);
});

it('ends the session the browser had before', async () => {
	const previous = await sessions.create(ADMIN, 'Firefox');

	const { landed, cookies } = land(oauth.approve({ did: DJ, state: null, scope: SCOPES.full }), {
		cookies: { [SESSION_COOKIE]: previous }
	});

	await expect(landed).rejects.toMatchObject(redirectTo('/dashboard'));
	expect(await sessions.resolve(previous)).toBeNull();
	expect(await sessions.resolve(cookies.get(SESSION_COOKIE))).toBe(DJ);
});

it("goes to the dashboard when the state isn't the app's own", async () => {
	for (const state of ['not json', '{"next":"//evil.test"}'])
		await expect(land(oauth.approve({ did: DJ, state, scope: SCOPES.full })).landed).rejects.toMatchObject(
			redirectTo('/dashboard')
		);
});

it('says when an address someone turned email on for waits on a confirmation the site could not send', async () => {
	const state = JSON.stringify({ next: '/dashboard/account/notifications', intent: 'email' });

	await expect(
		land(oauth.approve({ did: DJ, state, scope: `${SCOPES.full} ${SCOPES.email}` })).landed
	).rejects.toMatchObject(redirectTo('/dashboard/account/notifications?email=limited'));
	expect(await app().notificationChannels.get(DJ, 'email')).toMatchObject({
		address: 'dj@example.com',
		pausedReason: 'email_unverified'
	});
});

it('says when someone turning email on signed in without sharing their address', async () => {
	const state = JSON.stringify({ next: '/dashboard/account/notifications', intent: 'email' });

	await expect(land(oauth.approve({ did: DJ, state, scope: SCOPES.full })).landed).rejects.toMatchObject(
		redirectTo('/dashboard/account/notifications?email=declined')
	);
});

it('sends someone who turned down sharing their email back to the settings, still signed in', async () => {
	const { landed, cookies } = land(
		oauth.decline(JSON.stringify({ next: '/dashboard/account/notifications', intent: 'email' }))
	);

	await expect(landed).rejects.toMatchObject(redirectTo('/dashboard/account/notifications?email=declined'));
	expect(cookies.get(SESSION_COOKIE)).toBeUndefined();
});

it('sends anyone else who turned sign-in down back to sign in', async () => {
	for (const query of [
		oauth.decline(JSON.stringify({ next: '/shows' })),
		oauth.decline('not json'),
		'error=access_denied'
	])
		await expect(land(query).landed).rejects.toMatchObject(redirectTo('/login?error=denied'));
});

it('says sign-in failed when the authorization server reports an error', async () => {
	await expect(land(oauth.decline(JSON.stringify({ intent: 'email' }), 'server_error')).landed).rejects.toMatchObject(
		redirectTo('/login?error=login')
	);
});

it('says sign-in failed, and signs no one in, for a callback the client cannot finish', async () => {
	const { landed, cookies } = land('code=forged&iss=https%3A%2F%2Fpds.test');

	await expect(landed).rejects.toMatchObject(redirectTo('/login?error=login'));
	expect(cookies.get(SESSION_COOKIE)).toBeUndefined();
	expect(await logins()).toEqual([]);
});
