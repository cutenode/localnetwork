import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { SIGNED_OUT_COOKIE } from '#lib/signed-out-drafts.ts';
import { SESSION_COOKIE, WebSessions } from '#lib/server/auth/session.ts';
import { createTestServices } from '#lib/server/testing/core.ts';
import { callRoute, routeEvent } from '#lib/testing/route-event.ts';
import { POST } from './+server.ts';

const ADMIN = 'did:plc:admin';

const MOD = 'did:plc:mod';

const DJ = 'did:plc:dj';

/** An OAuth client that keeps which accounts' sessions it revoked. */
class FakeOAuth {
	readonly revoked: string[] = [];

	async revoke(did: string): Promise<void> {
		this.revoked.push(did);
	}
}

let t: Awaited<ReturnType<typeof createTestServices>>;

let oauth: FakeOAuth;

let sessions: WebSessions;

beforeEach(async () => {
	t = await createTestServices({ admins: [ADMIN] });
	oauth = new FakeOAuth();
	sessions = new WebSessions(t.db, 30);
	await t.staff.grant(await t.viewer(ADMIN), MOD, 'moderator');
});

afterEach(async () => {
	vi.useRealTimers();
	await t.audit.idle();
	await t.db.destroy();
});

/** The app at `publicUrl`. */
const app = (publicUrl = 'https://localnetwork.test') => ({ ...t, oauth, sessions, config: { publicUrl } });

/** `did` signs out of a fresh session at `publicUrl`, in a browser that holds `cookies` besides the session's. */
async function signOut(did: string | null, { cookies = {}, publicUrl = 'https://localnetwork.test' } = {}) {
	const token = await sessions.create(did ?? DJ, 'Firefox');

	const event = routeEvent({
		app: app(publicUrl),
		viewer: did ? await t.viewer(did) : null,
		sessionToken: token,
		cookies: { ...cookies, [SESSION_COOKIE]: token },
		form: {}
	});

	await expect(event.call(POST)).rejects.toMatchObject({ status: 303, location: '/' });

	return { ...event, session: await sessions.resolve(token) };
}

/** The logout entries in the audit log, as actor and role. */
const logouts = async () =>
	t.db
		.selectFrom('auditEntry')
		.select(['actorDid', 'actorRole', 'summary'])
		.where('action', '=', 'auth.logout')
		.execute();

it('ends the session, revokes the OAuth grant, and logs the sign-out', async () => {
	const { cookies, session } = await signOut(DJ);

	expect(session).toBeNull();
	expect(cookies.deleted).toContain(SESSION_COOKIE);
	expect(oauth.revoked).toEqual([DJ]);
	expect(await logouts()).toEqual([{ actorDid: DJ, actorRole: 'member', summary: 'Signed out' }]);
});

it("tells the browser's scripts whose drafts to drop, adding to accounts signed out before", async () => {
	const { cookies } = await signOut(DJ, { cookies: { [SIGNED_OUT_COOKIE]: 'did:plc:earlier' } });

	expect(cookies.get(SIGNED_OUT_COOKIE)).toBe(`did:plc:earlier ${DJ}`);
	expect(cookies.options.get(SIGNED_OUT_COOKIE)).toEqual({
		path: '/',
		httpOnly: false,
		sameSite: 'lax',
		secure: true,
		maxAge: 365 * 86_400
	});
});

it('sets the signed-out cookie without `secure` on a plain http site', async () => {
	const { cookies } = await signOut(DJ, { publicUrl: 'http://127.0.0.1:5173' });

	expect(cookies.get(SIGNED_OUT_COOKIE)).toBe(DJ);
	expect(cookies.options.get(SIGNED_OUT_COOKIE)).toMatchObject({ secure: false });
});

it('logs staff signing out with their role', async () => {
	await signOut(ADMIN);
	await signOut(MOD);

	expect(await logouts()).toEqual([
		{ actorDid: ADMIN, actorRole: 'admin', summary: 'Signed out' },
		{ actorDid: MOD, actorRole: 'moderator', summary: 'Signed out' }
	]);
});

it('clears a session that has to sign in again without logging or revoking anything', async () => {
	const { cookies, session } = await signOut(null);

	expect(session).toBeNull();
	expect(cookies.deleted).toContain(SESSION_COOKIE);
	expect(cookies.get(SIGNED_OUT_COOKIE)).toBeUndefined();
	expect(oauth.revoked).toEqual([]);
	expect(await logouts()).toEqual([]);
});

it('signs out after five seconds when the audit copies never arrive', async () => {
	vi.useFakeTimers({ toFake: ['setTimeout'] });

	const done = callRoute(POST, {
		app: { ...app(), audit: { record: () => new Promise(() => {}) } },
		viewer: await t.viewer(DJ),
		form: {}
	});

	let settled = false;
	void done.catch(() => {
		settled = true;
	});

	await vi.advanceTimersByTimeAsync(4_999);
	expect(settled).toBe(false);

	await vi.advanceTimersByTimeAsync(1);
	await expect(done).rejects.toMatchObject({ status: 303, location: '/' });
	expect(oauth.revoked).toEqual([DJ]);
});
